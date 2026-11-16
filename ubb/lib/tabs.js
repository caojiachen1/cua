// Session manager: a session == one browser tab group with status emoji
// Agent tabs are created INTO the group;
// user tabs can be claimed into it after an on-page confirmation.
// No separate Agent window is ever opened.

import { err, GROUP_TITLES, GROUP_COLOR, randomHex } from './config.js';
import { cdp } from './cdp.js';
import { tabRegistry } from './tab-registry.js';

const SESSION_KEY = 'ubb_session';

// marks 语义（ABSORB-CODEX-PLAN §7，Codex 吸收）：deliverable=交付物（✅ 绿徽标）、
// handoff=交接物（⏸ 橙徽标）——这两种标记的 tab 默认活过 session.stop；none=清除。
// 与 Codex turn 域的差异：UBB session 跨轮次，marks 生命周期 = session 生命周期，
// session.start 不清 marks。
const MARKS_ALLOWED = new Set(['deliverable', 'handoff', 'none']);
const KEPT_MARKS = new Set(['deliverable', 'handoff']);

class SessionManager {
  constructor() {
    this.session = null; // { id, groupId, created:Set, claimed:Set, startedAt, lastTabId }
    this._readyPromise = null;
  }

  /**
   * Survive service-worker restarts (not full browser restarts).
   * Returns the SAME promise to every caller: a request that arrives while the
   * saved state is still being read must wait for it, otherwise restore() can
   * overwrite a session that request just created.
   */
  restore() {
    return this.ready();
  }

  ready() {
    if (!this._readyPromise) this._readyPromise = this._doRestore();
    return this._readyPromise;
  }

  async _doRestore() {
    try {
      const st = await chrome.storage.session.get(SESSION_KEY);
      const saved = st?.[SESSION_KEY];
      if (!saved) return;
      const alive = async (id) => {
        try { await chrome.tabs.get(id); return true; } catch { return false; }
      };
      const created = [];
      const claimed = [];
      for (const id of saved.created || []) if (await alive(id)) created.push(id);
      for (const id of saved.claimed || []) if (await alive(id)) claimed.push(id);
      if (!created.length && !claimed.length) {
        await chrome.storage.session.remove(SESSION_KEY);
        return;
      }
      this.session = {
        id: saved.id,
        groupId: saved.groupId,
        created: new Set(created),
        claimed: new Set(claimed),
        spawned: new Set((saved.spawned || []).filter((id) => Number.isInteger(id))),
        lastTabId: saved.lastTabId,
        startedAt: saved.startedAt,
        starterTabId: null,
        name: saved.name,
        ownWindowId: saved.ownWindowId ?? null,
        ownWindowWanted: saved.ownWindowWanted === true,
        paused: saved.paused === true,
        marks: new Map(Object.entries(saved.marks || {})),
      };
      await this._reapStarter();
    } catch {}
  }

  async persist() {
    try {
      if (!this.session) {
        await chrome.storage.session.remove(SESSION_KEY);
        return;
      }
      const s = this.session;
      await chrome.storage.session.set({
        [SESSION_KEY]: {
          id: s.id,
          groupId: s.groupId,
          created: [...s.created],
          claimed: [...s.claimed],
          spawned: [...(s.spawned || [])],
          lastTabId: s.lastTabId,
          startedAt: s.startedAt,
          name: s.name,
          ownWindowId: s.ownWindowId,
          ownWindowWanted: s.ownWindowWanted,
          paused: s.paused === true,
          marks: s.marks ? Object.fromEntries(s.marks) : {},
        },
      });
    } catch {}
  }

  active() {
    return this.session;
  }

  owns(tabId) {
    const s = this.session;
    if (!s) return false;
    return s.created.has(tabId) || s.claimed.has(tabId);
  }

  async _ensureGroup() {
    const s = this.session;
    if (s.groupId != null) return s.groupId;
    // tabGroups need at least one tab; create a temporary starter tab and
    // close it as soon as a real tab joins the group.
    const win = await lastFocusedWindow();
    const tab = await chrome.tabs.create({ url: 'about:blank', active: false, windowId: win });
    s.created.add(tab.id);
    s.starterTabId = tab.id;
    const groupId = await chrome.tabs.group({ tabIds: [tab.id] });
    await chrome.tabGroups.update(groupId, { title: GROUP_TITLES.running, color: GROUP_COLOR });
    s.groupId = groupId;
    s.lastTabId = tab.id;
    return groupId;
  }

  /** Close the temporary starter blank tab once a real tab joined the group. */
  async _reapStarter() {
    const s = this.session;
    if (s.starterTabId == null) return;
    const starter = s.starterTabId;
    s.starterTabId = null;
    s.created.delete(starter);
    if (s.lastTabId === starter) s.lastTabId = null;
    await chrome.tabs.remove(starter).catch(() => {});
  }

  async start({ own_window = false } = {}) {
    await this.ready();
    if (this.session) {
      return { session_id: this.session.id, already_active: true, group_id: this.session.groupId };
    }
    this.session = {
      id: `s-${randomHex(6)}`,
      groupId: null,
      created: new Set(),
      claimed: new Set(),
      spawned: new Set(),
      startedAt: Date.now(),
      lastTabId: null,
      marks: new Map(),
      ownWindowId: null,
      ownWindowWanted: own_window === true,
      paused: false,
    };
    if (!own_window) await this._ensureGroup();
    await this.persist();
    return {
      session_id: this.session.id,
      group_id: this.session.groupId,
      ...(own_window ? { own_window: true, note: 'a dedicated unfocused agent window opens with the first tab.create; raise it with tab.select when the page must be visible' } : { note: 'agent tabs will be created inside the UBB tab group; no new window is opened' }),
    };
  }

  /** Add a tab to the session group; recreate the group if the user closed it. */
  async _addToGroup(tabId) {
    const s = this.session;
    let alive = false;
    if (s.groupId != null) {
      try {
        await chrome.tabGroups.get(s.groupId);
        alive = true;
      } catch {
        alive = false;
      }
    }
    if (!alive) {
      s.groupId = await chrome.tabs.group({ tabIds: [tabId] });
      await chrome.tabGroups.update(s.groupId, { title: GROUP_TITLES.running, color: GROUP_COLOR });
      return;
    }
    await chrome.tabs.group({ tabIds: [tabId], groupId: s.groupId });
  }

  /** Create a tab inside the session group. In own-window mode the first
   *  createTab opens the dedicated window WITH the real url (no placeholder
   *  tab), and the window is re-created on demand if it disappeared. */
  async createTab(url = 'about:blank', { active = false } = {}) {
    await this.ready();
    const s = await this._require();
    if (url !== 'about:blank') url = normalizeUrl(url);
    // one gate for every tab-creating entry (tab.create, search, batch):
    // a restricted URL must die here, not in chrome.tabs.create's error
    if (cdp.isRestrictedUrl(url)) {
      throw err('URL_BLOCKED', `cannot open a restricted page in a session tab: ${url}`);
    }
    if (s.ownWindowWanted) {
      let win = s.ownWindowId;
      if (win != null) {
        const alive = await chrome.windows.get(win).then(() => true).catch(() => false);
        if (!alive) { win = null; s.ownWindowId = null; }
      }
      if (win == null) {
        // windows.create takes focused (not tabs.create's active) — the first
        // tab is inherently its window's active tab
        const w = await chrome.windows.create({ url, focused: false });
        s.ownWindowId = w.id;
        const tab = (w.tabs && w.tabs[0]) || (await chrome.tabs.query({ windowId: w.id }))[0];
        s.created.add(tab.id);
        s.lastTabId = tab.id;
        s.groupId = await chrome.tabs.group({ tabIds: [tab.id] });
        await chrome.tabGroups.update(s.groupId, { title: GROUP_TITLES.running, color: GROUP_COLOR });
        await this.persist();
        await tabRegistry.register(tab.id, { kind: 'created', sessionId: s.id, groupId: s.groupId, url: tab.url || url, title: tab.title });
        return { tab_id: tab.id, url: tab.url || url, in_group: true, own_window: true };
      }
      return this._createInWindow(url, { active }, win);
    }
    const win = await lastFocusedWindow();
    return this._createInWindow(url, { active }, win);
  }

  async _createInWindow(url, { active = false }, win) {
    const s = await this._require();
    const tab = await chrome.tabs.create({ url, active, windowId: win });
    s.created.add(tab.id);
    s.lastTabId = tab.id;
    await this._addToGroup(tab.id);
    await this._reapStarter();
    await this.persist();
    await tabRegistry.register(tab.id, { kind: 'created', sessionId: s.id, groupId: s.groupId, url: tab.url || url, title: tab.title });
    return { tab_id: tab.id, url: tab.url || url, in_group: true };
  }

  /** Claim a user tab into the group (confirmation is handled by the caller). */
  async attachClaimed(tabId, { kind = 'claimed' } = {}) {
    await this.ready();
    const s = await this._require();
    s.claimed.add(tabId);
    s.lastTabId = tabId;
    await this._addToGroup(tabId);
    await this.persist();
    const t = await chrome.tabs.get(tabId).catch(() => null);
    await tabRegistry.register(tabId, { kind, sessionId: s.id, groupId: s.groupId, url: t?.url, title: t?.title });
    return { tab_id: tabId, in_group: true };
  }

  async release(tabId) {
    await this.ready();
    const s = this.session;
    if (!s || !s.claimed.has(tabId)) throw err('TAB_NOT_OWNED', `tab ${tabId} is not a claimed tab`);
    s.claimed.delete(tabId);
    try {
      await chrome.tabs.ungroup([tabId]);
    } catch {}
    await tabRegistry.unregister(tabId, { reason: 'released' });
    await this.persist();
    return { tab_id: tabId, released: true };
  }

  /** A tab opened from one of ours (openerTabId) — tracked for cleanup only. */
  async noteSpawned(tabId) {
    if (!this.session || tabId == null) return false;
    if (this.session.created.has(tabId) || this.session.claimed.has(tabId)) return false;
    this.session.spawned.add(tabId);
    const t = await chrome.tabs.get(tabId).catch(() => null);
    await tabRegistry.register(tabId, {
      kind: 'spawned', sessionId: this.session.id, groupId: this.session.groupId,
      openerTabId: t?.openerTabId ?? null, url: t?.url, title: t?.title,
    });
    await this.persist();
    return true;
  }

  async closeSpawned() {
    const s = this.session;
    if (!s) return [];
    const ids = [...(s.spawned || [])];
    s.spawned.clear();
    for (const id of ids) {
      try { await chrome.tabs.remove(id); } catch { /* already gone */ }
      await tabRegistry.unregister(id, { reason: 'spawned-closed' });
    }
    await this.persist();
    return ids;
  }

  isPaused() {
    return !!(this.session && this.session.paused);
  }

  /** User revoked control: every tool that resolves a session tab is refused
   *  until a HUMAN re-arms (shield resume button, popup, or a new session). */
  async pause() {
    const s = this.session;
    if (!s) return false;
    s.paused = true;
    await this.persist();
    await this.setStatus('paused');
    return true;
  }

  /** Only called from a trusted UI path (shield resume / popup action). */
  async resume() {
    const s = this.session;
    if (!s) return false;
    s.paused = false;
    await this.persist();
    await this.setStatus('running');
    return true;
  }

  async setStatus(kind) {
    const s = this.session;
    if (!s || s.groupId == null) return;
    const base = GROUP_TITLES[kind] || GROUP_TITLES.running;
    const title = s.name ? `${base} · ${s.name}` : base;
    try {
      await chrome.tabGroups.update(s.groupId, { title });
    } catch {}
  }

  /**
   * 给会话 tab 打交付标记（tab.mark 工具的底层）：mark 存
   * ubb_session.marks[tabId] = { mark, at }。'none' 清除标记。
   * 非会话 tab / 非法值在这里拒绝（TAB_NOT_OWNED / BAD_MARK）。
   */
  async setMark(tabId, mark) {
    await this.ready();
    const s = this.session;
    if (!s || !this.owns(tabId)) {
      throw err('TAB_NOT_OWNED', `tab ${tabId} is not a session tab (only created/claimed tabs can be marked)`);
    }
    if (!MARKS_ALLOWED.has(mark)) {
      throw err('BAD_MARK', `mark 只允许 ${[...MARKS_ALLOWED].join('|')}（got ${JSON.stringify(String(mark).slice(0, 24))}）`);
    }
    let entry = null;
    if (mark === 'none') {
      s.marks.delete(tabId);
    } else {
      entry = { mark, at: Date.now() };
      s.marks.set(tabId, entry);
    }
    await this.persist();
    return entry;
  }

  /** 某 tab 的当前标记（'deliverable' | 'handoff' | null）。 */
  getMark(tabId) {
    return this.session?.marks?.get(tabId)?.mark ?? null;
  }

  /** 全部标记的只读快照：Map<tabId, {mark, at}>。 */
  getMarks() {
    return new Map(this.session?.marks || []);
  }

  async stop({ close_created = true, close_marked = false, close_spawned = false } = {}) {
    await this.ready();
    const s = this.session;
    if (!s) return { stopped: false, note: 'no active session' };
    // marks 语义：deliverable/handoff 的 tab 默认活过 stop —— 留在组内（badge 由
    // content/tab-badge.js 自持）、registry 记 orphaned（tab.gc 的可清对象）；
    // close_marked:true 才连它们一起关（旧行为）。
    const keptIds = [];
    const markedKept = (id) => !close_marked && KEPT_MARKS.has(this.getMark(id));
    const spawnedClosed = [];
    if (close_spawned) {
      for (const id of [...(s.spawned || [])]) {
        if (markedKept(id)) {
          keptIds.push(id);
          await tabRegistry.markOrphaned(id, { groupId: s.groupId });
          continue;
        }
        try { await chrome.tabs.remove(id); spawnedClosed.push(id); } catch { /* gone */ }
        await tabRegistry.unregister(id, { reason: 'spawned-closed' });
      }
      s.spawned.clear();
    } else {
      for (const id of [...(s.spawned || [])]) await tabRegistry.markOrphaned(id, { groupId: s.groupId });
    }
    for (const tabId of [...s.claimed]) {
      try {
        await chrome.tabs.ungroup([tabId]);
      } catch {}
      await tabRegistry.unregister(tabId, { reason: 'claimed-released' });
    }
    for (const tabId of [...s.created]) {
      if (close_created && !markedKept(tabId)) {
        try {
          await chrome.tabs.remove(tabId);
        } catch {}
        await tabRegistry.unregister(tabId, { reason: 'created-closed' });
      } else {
        // marked 的保留 tab 不拆组（📌 组要保持有成员）；close_created:false 的
        // 旧行为照旧 —— 未标记的拆组、记孤儿
        if (!markedKept(tabId)) {
          try {
            await chrome.tabs.ungroup([tabId]);
          } catch {}
        }
        await tabRegistry.markOrphaned(tabId, { groupId: s.groupId });
        keptIds.push(tabId);
      }
    }
    try {
      if (s.groupId != null) {
        await chrome.tabGroups.update(s.groupId, { title: keptIds.length ? GROUP_TITLES.kept : GROUP_TITLES.done });
      }
    } catch {}
    if (s.ownWindowId != null && close_created && !keptIds.length) {
      // the dedicated agent window empties out with its tabs — close it too
      //（还有保留 tab 时窗口绝不能关，否则 📌 tab 一起陪葬）
      try { await chrome.windows.remove(s.ownWindowId); } catch {}
    }
    this.session = null;
    await this.persist();
    return { stopped: true, session_id: s.id, closed_created: close_created, closed_spawned: spawnedClosed.length, kept_tabs: keptIds };
  }

  /** call after mutating lastTabId / created outside this module */
  async touch() {
    if (this.session) await this.persist();
  }

  _require() {
    if (!this.session) throw err('NO_SESSION', 'call session.start first (or use tab.create which auto-starts)');
    return this.session;
  }
}

async function lastFocusedWindow() {
  try {
    const wins = await chrome.windows.getAll({ populate: false, windowTypes: ['normal'] });
    const focused = wins.find((w) => w.focused) || wins[0];
    return focused?.id ?? undefined;
  } catch {
    return undefined;
  }
}

export function normalizeUrl(url) {
  if (typeof url !== 'string' || !url) throw err('BAD_URL', 'url must be a non-empty string');
  // Any explicit scheme is kept EXACTLY as written. Prepending https:// to
  // chrome:// (the old shape-only-known-scheme rule) laundered restricted
  // pages past isRestrictedUrl; blob:/javascript:/data: were one keystroke
  // from navigation. Downstream gates see the real scheme. A colon followed
  // by digits is a host:port guess, not a scheme (scheme chars may contain '.').
  if (/^[a-z][a-z0-9+.-]*:(?![0-9])/i.test(url)) return url;
  // scheme-less input is a host[:port][/path…] guess, nothing else
  if (!/^[A-Za-z0-9.\-_]+(:\d{1,5})?([/?#].*)?$/.test(url)) {
    throw err('BAD_URL', `"${url.slice(0, 80)}" is neither a URL with a scheme nor a bare host; refusing to guess https://`);
  }
  return 'https://' + url;
}

export const sessions = new SessionManager();
