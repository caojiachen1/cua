// Persistent CUA tab registry: ownership + lifecycle for every tab CUA owns.
// Lives in chrome.storage.local so an extension reload (which clears the
// in-memory session) does not forget that a tab is ours. Any tab sitting in a
// CUA-titled group is reconciled in as `orphaned`, so it is still known and can
// be garbage-collected after explicit confirmation.
//
// Ownership kinds:
//   created   — tab.create / search / pipeline tab
//   claimed   — a user tab taken over via tab.claim
//   spawned   — a tab opened from one of ours (openerTabId)
//   recovered — a tab adopted by session.recover
//   orphaned  — a CUA-group tab with no live session (reload, stop with keep)

const KEY = 'cua_tab_registry';
const HISTORY_KEY = 'cua_tab_history';
const MAX_ENTRIES = 500;
const MAX_HISTORY = 100;

class TabRegistry {
  constructor() {
    this.entries = new Map(); // tab_id → entry
    this.history = []; // recently closed/released, newest last
    this._ready = null;
  }

  ready() {
    if (!this._ready) this._ready = this._load();
    return this._ready;
  }

  async _load() {
    try {
      const st = await chrome.storage.local.get([KEY, HISTORY_KEY]);
      for (const e of st?.[KEY] || []) if (e?.tab_id != null) this.entries.set(e.tab_id, e);
      this.history = Array.isArray(st?.[HISTORY_KEY]) ? st[HISTORY_KEY] : [];
    } catch { /* registry is diagnostics/safety, never a tool blocker */ }
  }

  async _persist() {
    try {
      await chrome.storage.local.set({
        [KEY]: [...this.entries.values()].slice(-MAX_ENTRIES),
        [HISTORY_KEY]: this.history.slice(-MAX_HISTORY),
      });
    } catch { /* best-effort */ }
  }

  async register(tabId, { kind = 'created', sessionId = null, groupId = null, openerTabId = null, url = null, title = null } = {}) {
    await this.ready();
    if (tabId == null) return null;
    const now = Date.now();
    let e = this.entries.get(tabId);
    if (!e) {
      e = {
        tab_id: tabId, kind, session_id: sessionId, group_id: groupId, opener_tab_id: openerTabId,
        created_at: now, first_seen_at: now, last_seen_at: now, url, title, state: 'open',
      };
    } else {
      Object.assign(e, {
        kind: kind || e.kind,
        session_id: sessionId ?? e.session_id,
        group_id: groupId ?? e.group_id,
        opener_tab_id: openerTabId ?? e.opener_tab_id,
        url: url ?? e.url,
        title: title ?? e.title,
        state: 'open',
        last_seen_at: now,
      });
    }
    this.entries.set(tabId, e);
    await this._persist();
    return { ...e };
  }

  async touch(tabId, { url, title, groupId } = {}) {
    await this.ready();
    const e = this.entries.get(tabId);
    if (!e) return false;
    if (url != null) e.url = url;
    if (title != null) e.title = title;
    if (groupId != null) e.group_id = groupId;
    e.last_seen_at = Date.now();
    await this._persist();
    return true;
  }

  async unregister(tabId, { reason = 'closed' } = {}) {
    await this.ready();
    const e = this.entries.get(tabId);
    if (!e) return false;
    this.entries.delete(tabId);
    this.history.push({ ...e, closed_at: Date.now(), reason });
    await this._persist();
    return true;
  }

  /** Keep the entry but drop the session binding: a tab the session stopped
   *  owning (close_created:false / a reload). It is ours until GC'd. */
  async markOrphaned(tabId, { groupId = null } = {}) {
    await this.ready();
    const e = this.entries.get(tabId);
    if (!e) return false;
    e.kind = 'orphaned';
    e.session_id = null;
    if (groupId != null) e.group_id = groupId;
    e.last_seen_at = Date.now();
    await this._persist();
    return true;
  }

  async list() {
    await this.ready();
    return [...this.entries.values()];
  }

  async historyList(limit = 20) {
    await this.ready();
    const n = Math.max(1, Math.min(Number(limit) || 20, MAX_HISTORY));
    return this.history.slice(-n).reverse();
  }

  /** Reconcile storage with the live tab strip: drop dead ids into history,
   *  adopt any tab in a CUA-titled group that the registry does not know, and
   *  release entries whose tab left the CUA group. */
  async reconcile({ activeSessionId = null } = {}) {
    await this.ready();
    const tabs = await chrome.tabs.query({}).catch(() => []);
    const byId = new Map(tabs.map((t) => [t.id, t]));
    for (const id of [...this.entries.keys()]) {
      if (byId.has(id)) continue;
      const e = this.entries.get(id);
      this.entries.delete(id);
      this.history.push({ ...e, closed_at: Date.now(), reason: 'reconciled-gone' });
    }
    const groups = (await chrome.tabGroups.query({}).catch(() => [])).filter((g) => /CUA/i.test(g.title || ''));
    const groupIds = new Set(groups.map((g) => g.id));
    for (const t of tabs) {
      if (t.id == null || t.groupId == null || !groupIds.has(t.groupId)) continue;
      const e = this.entries.get(t.id);
      if (!e) {
        const now = Date.now();
        this.entries.set(t.id, {
          tab_id: t.id, kind: 'orphaned', session_id: null, group_id: t.groupId,
          opener_tab_id: t.openerTabId ?? null, created_at: now, first_seen_at: now,
          last_seen_at: now, url: t.url || null, title: t.title || null, state: 'open',
        });
      } else {
        e.state = 'open';
        e.group_id = t.groupId;
        e.url = t.url || e.url;
        e.title = t.title || e.title;
        e.last_seen_at = Date.now();
      }
    }
    // A session-bound entry whose session no longer exists (extension reload,
    // SW restart without a restored session) is now an orphan, not a live
    // session tab. Keeping it is the whole point of the registry.
    for (const e of this.entries.values()) {
      if (e.session_id != null && e.session_id !== activeSessionId) {
        e.kind = 'orphaned';
        e.session_id = null;
      }
    }
    // Leaving the group is not forgetting: a tab CUA opened stays in the
    // registry (with its group_id stale) until it is closed or explicitly
    // released. That is what makes an ungrouped orphan still collectable.
    await this._persist();
    return [...this.entries.values()];
  }

  async listWithTabs(activeSessionId = null) {
    await this.reconcile({ activeSessionId });
    const tabs = await chrome.tabs.query({}).catch(() => []);
    const byId = new Map(tabs.map((t) => [t.id, t]));
    const now = Date.now();
    return [...this.entries.values()].map((e) => {
      const t = byId.get(e.tab_id);
      return {
        tab_id: e.tab_id,
        ownership: e.kind,
        kind: e.kind,
        session_id: e.session_id,
        group_id: e.group_id,
        opener_tab_id: e.opener_tab_id,
        created_at: e.created_at,
        age_ms: now - e.created_at,
        last_seen_at: e.last_seen_at,
        open: !!t,
        url: t?.url || e.url,
        title: t?.title || e.title,
        state: e.kind === 'orphaned' || e.session_id == null ? 'orphaned' : 'session',
      };
    });
  }

  /** Orphans older than maxAgeMs (0 = any age). Session-owned tabs are never
   *  candidates, and non-CUA tabs are not in the registry at all. */
  async gcCandidates(maxAgeMs = 0, activeSessionId = null) {
    await this.reconcile({ activeSessionId });
    const now = Date.now();
    return [...this.entries.values()]
      .filter((e) => e.kind === 'orphaned' || e.session_id == null)
      .filter((e) => !maxAgeMs || now - e.created_at >= maxAgeMs)
      .map((e) => ({ ...e }));
  }
}

export const tabRegistry = new TabRegistry();
