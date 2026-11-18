// Additional tools added after the field rounds:
// closed-shadow perception, current-tab download, window raise, session/tab
// recovery, live page probes. Everything here uses the same dispatcher and the
// same truthfulness rules (a refused input is an error, never a fake success).

import { cdp } from './cdp.js';
import { err } from './config.js';
import { sessions } from './tabs.js';
import { targetTab, evalJs, armTab } from './actions.js';
import { withPass, sleep, shield } from './shield-ctl.js';
import * as human from './human.js';
import { downloadLedger } from './tools-downloads.js';
import { audit } from './audit.js';
import { redactUrl, truncate } from './redact.js';
import { pageProbe } from './probe.js';
import { safeFilename, waitDownload } from './download-util.js';
import { tabRegistry } from './tab-registry.js';
import { setMock, listMocks, clearMocks } from './netmock.js';

function attrsOfNode(n) {
  const a = n.attributes || [];
  const out = {};
  for (let i = 0; i + 1 < a.length; i += 2) out[a[i]] = a[i + 1];
  return out;
}

function boxFromModel(m) {
  const q = m?.model?.border;
  if (!q || q.length < 8) return null;
  const xs = [q[0], q[2], q[4], q[6]];
  const ys = [q[1], q[3], q[5], q[7]];
  const x = Math.min(...xs), y = Math.min(...ys);
  return {
    x: Math.round(x), y: Math.round(y),
    width: Math.round(Math.max(...xs) - x), height: Math.round(Math.max(...ys) - y),
    cx: Math.round(x + (Math.max(...xs) - x) / 2),
    cy: Math.round(y + (Math.max(...ys) - y) / 2),
  };
}

async function visible(tabId) {
  const st = await evalJs(tabId, 'document.visibilityState').catch(() => null);
  if (st === null || st === 'visible') return;
  throw err('INPUT_NOT_DELIVERED', `document.visibilityState = ${JSON.stringify(st)} — hidden documents do not drain trusted input; bring the window forward (window.raise) and retry. Nothing was clicked.`, {
    tab_id: tabId, stage: 'pre-dispatch', visibility_state: st, dispatched: false,
  });
}

async function clickPoint(tabId, x, y) {
  await visible(tabId);
  await withPass(tabId, async () => {
    await human.humanMoveTo(tabId, x + (Math.random() * 12 - 6), y + (Math.random() * 12 - 6));
    await human.humanMoveTo(tabId, x, y);
    await sleep(40 + Math.random() * 80);
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
    await sleep(45 + Math.random() * 70);
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 1, clickCount: 1 });
  }, { at: [x, y] });
}

/** Pierced-DOM scan: closed shadow roots included. Top document only. */
async function shadowScan(tabId, { text, tag, className, max = 40, includeInputs = true } = {}) {
  const flat = await cdp.send(tabId, 'DOM.getFlattenedDocument', { depth: -1, pierce: true });
  const wanted = tag ? String(tag).toUpperCase() : null;
  const out = [];
  const probeTags = new Set(['BUTTON', 'A', 'INPUT', 'TEXTAREA', 'LABEL', 'SPAN', 'DIV', 'LI', 'OPTION', 'SELECT']);
  let scanned = 0;
  let htmlFetches = 0;
  for (const n of flat?.nodes || []) {
    if (!n.nodeId || !n.nodeName || n.nodeName.startsWith('#')) continue;
    scanned += 1;
    if (wanted && n.nodeName !== wanted) continue;
    if (!includeInputs && (n.nodeName === 'INPUT' || n.nodeName === 'TEXTAREA')) continue;
    const a = attrsOfNode(n);
    if (className && !String(a.class || '').includes(className)) continue;
    let txt = '';
    if (text) {
      if (!probeTags.has(n.nodeName)) continue;
      if (htmlFetches++ > 250) continue;
      const html = await cdp.send(tabId, 'DOM.getOuterHTML', { nodeId: n.nodeId }).catch(() => null);
      if (html?.outerHTML) txt = html.outerHTML.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
      txt += ` ${a['aria-label'] || ''} ${a.placeholder || ''} ${a.value || ''}`;
      if (!txt.toLowerCase().includes(String(text).toLowerCase())) continue;
    }
    const box = boxFromModel(await cdp.send(tabId, 'DOM.getBoxModel', { nodeId: n.nodeId }).catch(() => null));
    if (!box || box.width < 2 || box.height < 2) continue;
    out.push({
      key: n.backendNodeId,
      node_id: n.nodeId,
      tag: n.nodeName,
      text: truncate(txt || (a['aria-label'] || a.placeholder || a.value || ''), 120),
      id: a.id || null,
      class: truncate(a.class || '', 120),
      role: a.role || null,
      box,
      shadow: n.shadowRootType || null,
    });
    if (out.length >= max) break;
  }
  return { nodes: out, scanned };
}

function registerMore(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('page.probe', {
    category: 'sensing',
    description: 'One cheap read for "did the page change?": {url,title,doc_id,text_hash,text_len,first_text,ready}. doc_id is per-document (navigation) and text_hash covers innerText changes that keep the URL (SPA pagination). Feed the value to wait_for {condition:"content_changed", from:…}.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const probe = await pageProbe(tabId, evalJs);
    return redactProbe(probe);
  });

  R('shadow.list', {
    category: 'sensing',
    description: 'Scan the pierced DOM (closed shadow roots included) for visible nodes. Filters: text (substring of text/aria-label/placeholder/value), tag, className, max. Returns {key,node_id,tag,text,id,class,role,box,shadow}. Top document only; use challenge.dom/frames for iframes.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    await cdp.ensure(tabId);
    const r = await shadowScan(tabId, {
      text: p.text, tag: p.tag, className: p.className,
      max: Math.min(Math.max(Number(p.max) || 40, 1), 200),
    });
    audit.log({ action: 'shadow.list', tabId, found: r.nodes.length });
    return r;
  });

  R('shadow.click', {
    category: 'interaction',
    timeoutMs: 45000,
    description: 'Click a node found by shadow.list (key), or by {text,tag} search, using real CDP input at the node box. This is the path for closed shadow DOM (the normal ref/locator tools cannot see inside). Refuses before dispatch when the document is hidden.',
  }, async (p) => {
    const tabId = targetTab(p);
    await cdp.ensure(tabId);
    let node = null;
    if (p.key != null) {
      const r = await shadowScan(tabId, { text: p.text, tag: p.tag, className: p.className, max: 200 });
      node = r.nodes.find((n) => String(n.key) === String(p.key)) || null;
      if (!node) node = r.nodes.find((n) => n.node_id === Number(p.key)) || null;
    } else {
      const r = await shadowScan(tabId, { text: p.text, tag: p.tag, className: p.className, max: 20 });
      if (r.nodes.length > 1 && !p.nth && p.nth !== 0) {
        throw err('SHADOW_AMBIGUOUS', `${r.nodes.length} nodes match; pass nth or a narrower text/tag`, { candidates: r.nodes.slice(0, 8).map((n) => ({ key: n.key, tag: n.tag, text: n.text, box: n.box })) });
      }
      node = r.nodes[Number(p.nth) || 0] || null;
    }
    if (!node) throw err('SHADOW_NOT_FOUND', 'no visible pierced-DOM node matched');
    const at = node.box;
    if (p.label && /(delete|remove|pay|purchase|transfer|删除|支付|购买|转账)/i.test(String(p.label)) && p.confirm !== true) {
      throw err('NEEDS_CONFIRM', 'destructive/paid label — retry with confirm:true after asking the user');
    }
    await clickPoint(tabId, at.cx, at.cy);
    audit.log({ action: 'shadow.click', tabId, key: node.key, shadow: node.shadow });
    return { clicked: true, key: node.key, tag: node.tag, text: node.text, at: [at.cx, at.cy], shadow: node.shadow };
  });

  R('shadow.fill', {
    category: 'interaction',
    timeoutMs: 45000,
    description: 'Click a pierced-DOM input (closed shadow roots included), select its content and insert value as trusted input. Find it with the same filters as shadow.list ({text,tag,className} or key).',
  }, async (p) => {
    const tabId = targetTab(p);
    if (p.value == null) throw err('BAD_PARAMS', 'value required');
    await cdp.ensure(tabId);
    let node = null;
    if (p.key != null) {
      const r = await shadowScan(tabId, { text: p.text, tag: p.tag, className: p.className, max: 200 });
      node = r.nodes.find((n) => String(n.key) === String(p.key)) || r.nodes.find((n) => n.node_id === Number(p.key)) || null;
    } else {
      const r = await shadowScan(tabId, { text: p.text, tag: p.tag, className: p.className, max: 20, includeInputs: true });
      node = r.nodes[Number(p.nth) || 0] || null;
    }
    if (!node) throw err('SHADOW_NOT_FOUND', 'no visible pierced-DOM node matched');
    await clickPoint(tabId, node.box.cx, node.box.cy);
    const mod = 2; // Ctrl
    await cdp.send(tabId, 'Input.dispatchKeyEvent', { type: 'keyDown', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, modifiers: mod });
    await cdp.send(tabId, 'Input.dispatchKeyEvent', { type: 'keyUp', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, modifiers: mod });
    await cdp.send(tabId, 'Input.insertText', { text: String(p.value) });
    audit.log({ action: 'shadow.fill', tabId, key: node.key, len: String(p.value).length });
    return { filled: true, key: node.key, tag: node.tag, at: [node.box.cx, node.box.cy], value_len: String(p.value).length };
  });

  R('download.current', {
    category: 'interaction',
    timeoutMs: 90000,
    description: 'Download the URL the target tab is showing (PDF viewer, signed CDN link, blob) via chrome.downloads, with an optional filename RELATIVE to the browser download directory (subdirectories allowed). Returns the final record; waits up to timeout_ms (default 60s).',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const tab = await chrome.tabs.get(tabId).catch(() => null);
    const url = tab?.url || '';
    if (!/^(https?|blob):/i.test(url)) throw err('BAD_URL', `the tab is not showing a downloadable http(s)/blob URL: ${url.slice(0, 120)}`);
    const filename = safeFilename(p.filename);
    const id = await chrome.downloads.download({ url, saveAs: false, ...(filename ? { filename } : {}) });
    downloadLedger.claim(id, tabId);
    audit.log({ action: 'download.current', tabId, url: redactUrl(url), filename: filename || null });
    if (p.wait === false) return { download_id: id, url: redactUrl(url), filename, waiting: false };
    const rec = await waitDownload(id, p.timeout_ms);
    return { ...rec, url: redactUrl(url) };
  });

  R('window.create', {
    category: 'tabs',
    timeoutMs: 20000,
    description: 'Create a NEW OS window holding one tab. A freshly created window lands on the CURRENT virtual desktop, and its creator is exempt from focus-steal protection — so this is the reliable escape hatch when window.raise keeps reporting "another app covers it" (the classic cause: the old window sits on another virtual desktop, where visibilityState stays hidden no matter what you raise). Pair with tab.move to relocate an already-logged-in tab (e.g. a PDF viewer that already cleared its challenge) into the fresh window instead of re-navigating.',
  }, async (p) => {
    const url = String(p.url || 'about:blank');
    const win = await chrome.windows.create({ url, focused: p.focused !== false, state: p.state || 'maximized' });
    const tab = (win.tabs && win.tabs[0]) || null;
    audit.log({ action: 'window.create', windowId: win.id, tabId: tab ? tab.id : null });
    return { window_id: win.id, tab_id: tab ? tab.id : null, focused: !!win.focused, state: win.state };
  });

  R('tab.move', {
    category: 'tabs',
    timeoutMs: 20000,
    description: 'Move a tab into another window (window_id) and/or reposition it (index). Use after window.create to relocate an existing tab that already holds login state or a cleared challenge — moving it preserves everything; re-navigating would lose the signed one-shot URLs.',
  }, async (p) => {
    const tabId = targetTab(p);
    const props = {};
    if (p.window_id != null) props.windowId = Number(p.window_id);
    if (p.index != null) props.index = Number(p.index);
    if (props.windowId == null && props.index == null) throw err('BAD_PARAMS', 'pass window_id and/or index');
    const tab = await chrome.tabs.move(tabId, props);
    audit.log({ action: 'tab.move', tabId, windowId: tab.windowId, index: tab.index });
    return { tab_id: tab.id, window_id: tab.windowId, index: tab.index };
  });

  R('window.raise', {
    category: 'tabs',
    timeoutMs: 20000,
    sensitive: true,
    description: 'Bring the target tab\'s OS window to the foreground (activate the tab, focus the window) and report the resulting document.visibilityState. The window is brought up MAXIMIZED by default: restoring a window that Edge remembers in a tiny geometry made every raise pop a small frame in a screen corner (user complaint). Pass state:"normal" only if you specifically need the remembered size. Needs confirm:true: this steals focus from the user, which is the only way trusted input reaches an occluded window. Honest: if another app still covers the window, it reports raised:false and tells you to use OS-level focus (cua).',
  }, async (p) => {
    if (p.confirm !== true) throw err('NEEDS_CONFIRM', 'raising the window steals focus — retry with confirm:true');
    const tabId = targetTab(p);
    const tab = await chrome.tabs.get(tabId);
    const winState = p.state === 'normal' ? 'normal' : 'maximized';
    await chrome.tabs.update(tabId, { active: true }).catch(() => {});
    await chrome.windows.update(tab.windowId, { state: winState, focused: true }).catch(() => {});
    let state = null;
    for (let i = 0; i < 12; i++) {
      state = await evalJs(tabId, 'document.visibilityState').catch(() => null);
      if (state === 'visible') break;
      await sleep(200);
    }
    audit.log({ action: 'window.raise', tabId, visibility_state: state, window_state: winState });
    return {
      raised: state === 'visible',
      visibility_state: state,
      window_state: winState,
      note: state === 'visible'
        ? 'window is foreground; trusted input can be dispatched'
        : 'the window is still hidden (another app covers it). Only an OS-level foreground (cua activate) can fix that; do not retry input blindly.',
    };
  });

  R('session.recover', {
    category: 'session',
    description: 'Adopt the tabs still sitting in ANY CUA tab group into a fresh session after an extension reload, and re-arm each one. Returns the adopted tab ids. Use after a reload lost the session.',
  }, async () => {
    if (!sessions.active()) await sessions.start();
    const groups = (await chrome.tabGroups.query({}).catch(() => [])).filter((x) => /CUA/i.test(x.title || ''));
    if (!groups.length) throw err('NO_SESSION', 'no CUA tab group found — nothing to recover');
    const adopted = [];
    const seen = new Set();
    for (const g of groups) {
      const tabs = await chrome.tabs.query({ groupId: g.id }).catch(() => []);
      for (const t of tabs) {
        if (t.id == null || seen.has(t.id)) continue;
        seen.add(t.id);
        try {
          await sessions.attachClaimed(t.id, { kind: 'recovered' });
          await armTab(t.id);
          adopted.push(t.id);
        } catch { /* a tab that vanished is not a recovery failure for the rest */ }
      }
    }
    await sessions.setStatus('running');
    audit.log({ action: 'session.recover', count: adopted.length, groups: groups.length });
    return { recovered: adopted.length, tab_ids: adopted, groups: groups.map((g) => g.id) };
  });

  R('tab.repair', {
    category: 'tabs',
    timeoutMs: 60000,
    description: 'Re-inject this extension\'s content scripts into the target tab (all declared frames), re-arm the shield, and verify the shield answers. Use on tabs that show SHIELD_PASS_FAILED / a dead bridge after an extension reload instead of reloading the page by hand.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const manifest = chrome.runtime.getManifest();
    const groups = manifest.content_scripts || [];
    const injected = [];
    const failures = [];
    for (const g of groups) {
      const files = g.js || [];
      if (!files.length) continue;
      try {
        await chrome.scripting.executeScript({
          target: { tabId, allFrames: g.all_frames === true },
          files,
        });
        injected.push({ files, all_frames: g.all_frames === true });
      } catch (e) {
        failures.push({ files, error: String(e?.message || e).slice(0, 200) });
      }
    }
    await armTab(tabId);
    await sleep(350);
    let state = null;
    try {
      state = await Promise.race([
        chrome.tabs.sendMessage(tabId, { type: 'CUA_SHIELD_QUERY' }),
        sleep(2000).then(() => null),
      ]);
    } catch { state = null; }
    audit.log({ action: 'tab.repair', tabId, injected: injected.length, shielded: !!state });
    return {
      repaired: injected.length > 0,
      injected,
      failures,
      shield_state: state ? state.state || state : null,
      note: state
        ? 'content scripts re-injected and the shield answered'
        : 'content scripts injected but the shield did not answer — reload the tab (dev.reload) if input is still refused',
    };
  });

  R('tab.lifecycle', {
    category: 'tabs',
    description: 'Persistent lifecycle of every CUA-owned tab: ownership (created / claimed / spawned / recovered / orphaned), session_id, group, opener, created_at, age_ms, current url/title, open, plus recently closed CUA tabs. Reconciles against the live CUA groups first; survives extension reloads because it is stored in chrome.storage.local.',
  }, async (p = {}) => {
    const tabs = await tabRegistry.listWithTabs(sessions.active()?.id || null);
    const closed = await tabRegistry.historyList(p.history_limit || 20);
    audit.log({ action: 'tab.lifecycle', open: tabs.length });
    return { count: tabs.length, tabs, closed_recent: closed };
  });

  R('tab.gc', {
    category: 'tabs',
    sensitive: true,
    description: 'Close orphaned CUA tabs (ownership orphaned / no live session) older than max_age_ms (0 = all orphaned). dry_run:true lists them; closing needs confirm:true. Session-owned tabs and non-CUA tabs are never touched. Use after tab.lifecycle to confirm a tab is no longer needed.',
  }, async (p = {}) => {
    const maxAge = Math.max(0, Number(p.max_age_ms) || 0);
    const cands = await tabRegistry.gcCandidates(maxAge, sessions.active()?.id || null);
    const view = cands.map((c) => ({
      tab_id: c.tab_id, kind: c.kind, session_id: c.session_id, created_at: c.created_at,
      url: redactUrl(c.url || ''), title: truncate(c.title || '', 90),
    }));
    if (p.dry_run === true) return { dry_run: true, count: view.length, candidates: view };
    if (p.confirm !== true) {
      throw err('NEEDS_CONFIRM', `closing ${view.length} orphaned CUA tab(s) is destructive — call with dry_run:true to inspect, then confirm:true`, { candidates: view.slice(0, 20) });
    }
    const closed = [];
    const failed = [];
    for (const c of cands) {
      try { await chrome.tabs.remove(c.tab_id); closed.push(c.tab_id); }
      catch (e) { failed.push({ tab_id: c.tab_id, error: String(e?.message || e).slice(0, 120) }); }
      await tabRegistry.unregister(c.tab_id, { reason: 'gc' });
    }
    audit.log({ action: 'tab.gc', count: closed.length, failed: failed.length });
    return { closed, count: closed.length, failed };
  });

  R('tab.close_cua', {
    category: 'tabs',
    sensitive: true,
    risk: 'delete', // policy strict 档（ABSORB-CODEX-PLAN §11）：删除类每次调用都要 confirm:true
    description: 'Detect and close every tab that belongs to ANY CUA tab group — including orphan groups left behind by an extension reload (those tabs are not session-owned, so tab.close refuses them). dry_run:true lists without closing; closing needs confirm:true. Non-CUA tabs are untouched.',
  }, async (p = {}) => {
    const groups = (await chrome.tabGroups.query({}).catch(() => [])).filter((g) => /CUA/i.test(g.title || ''));
    const found = [];
    for (const g of groups) {
      const tabs = await chrome.tabs.query({ groupId: g.id }).catch(() => []);
      for (const t of tabs) {
        if (t.id == null) continue;
        found.push({ tab_id: t.id, title: truncate(t.title || '', 90), url: redactUrl(t.url || ''), group_id: g.id, group_title: g.title || '' });
      }
    }
    if (p.dry_run === true) return { dry_run: true, count: found.length, tabs: found };
    if (p.confirm !== true) throw err('NEEDS_CONFIRM', `closing ${found.length} CUA-group tab(s) is destructive — call again with dry_run:true to inspect, then confirm:true`);
    const closed = [];
    const failed = [];
    for (const t of found) {
      try { await chrome.tabs.remove(t.tab_id); closed.push(t.tab_id); }
      catch (e) { failed.push({ tab_id: t.tab_id, error: String(e?.message || e).slice(0, 120) }); }
    }
    const s = sessions.active();
    if (s) {
      for (const id of closed) { s.created.delete(id); s.claimed.delete(id); s.spawned.delete(id); }
      await sessions.persist();
    }
    audit.log({ action: 'tab.close_cua', count: closed.length, groups: groups.length });
    return { closed, count: closed.length, groups: groups.map((g) => ({ id: g.id, title: g.title })), failed };
  });

  R('tab.close_spawned', {
    category: 'tabs',
    description: 'Close tabs this session opened indirectly (links/entries that opened a new tab), tracked by chrome.tabs.onCreated through openerTabId. Does not touch the session tabs themselves. Returns the closed ids.',
  }, async () => {
    const ids = await sessions.closeSpawned();
    audit.log({ action: 'tab.close_spawned', count: ids.length });
    return { closed: ids, count: ids.length };
  });

  // ---- tab lifecycle events (steer feed) --------------------------------------
  // The model finds out a tab died only when a ref/refind fails; the daemon
  // already receives a tab.closed event, and this tool drains the same ring on
  // demand so a long run can reconcile its plan with reality.

  R('tab.events', {
    category: 'tabs',
    description: 'Recent tab lifecycle events this browser produced: closed (any tab, with the last known url/title), created (tabs opened by session pages), navigated (main-frame commits). Ring of the last 100, read with since_seq or drained (drain:true consumes). Use after a multi-step run to see what died/moved before retrying refs against ghosts.',
  }, async (p = {}) => {
    const all = tabEventRing();
    const since = Number(p.since_seq) || 0;
    const events = all.filter((e) => e.seq > since);
    if (p.drain === true) tabEventSeq = all.length ? all[all.length - 1].seq : tabEventSeq;
    return { events: events.slice(-100), last_seq: all.length ? all[all.length - 1].seq : tabEventSeq };
  });

  // ---- per-site content preferences (browser.pref) -----------------------------

  R('browser.pref', {
    category: 'diagnostics',
    timeoutMs: 10000,
    description: 'Per-site content settings via chrome.contentSettings: get/set/clear one of images|javascript|cookies|popups|notifications for a primaryPattern (e.g. "https://example.com" or "https://*.example.com") with setting allow|block|session_only (cookies also ask_before). get returns the effective setting for a site URL. The API is capability-probed: if this browser build does not expose contentSettings the tool says so instead of pretending.',
  }, async (p = {}) => {
    // shape first, capability second — a malformed call must not read as an
    // unsupported browser
    const kinds = ['images', 'javascript', 'cookies', 'popups', 'notifications'];
    const kind = String(p.kind || '').toLowerCase();
    if (!kinds.includes(kind)) throw err('BAD_PARAMS', `kind must be one of ${kinds.join('|')}`);
    const action = String(p.action || 'get').toLowerCase();
    if (!['get', 'set', 'clear'].includes(action)) throw err('BAD_PARAMS', 'action must be get | set | clear');
    if (action === 'set') {
      const setting = String(p.setting || '');
      if (!['allow', 'block', 'session_only', 'ask_before'].includes(setting)) {
        throw err('BAD_PARAMS', 'setting must be allow | block | session_only | ask_before');
      }
    }
    const cs = chrome.contentSettings;
    if (!cs) throw err('CAPABILITY_MISSING', 'chrome.contentSettings is not available in this browser build — per-site content preferences cannot be read or set here');
    const api = cs[kind];
    if (!api) throw err('CAPABILITY_MISSING', `contentSettings.${kind} is not exposed by this browser`);
    if (action === 'get') {
      const url = String(p.site || p.primary_pattern || '');
      if (!url) throw err('BAD_PARAMS', 'get needs site (a URL)');
      const r = await api.get({ primaryUrl: url, incognito: p.incognito === true });
      return { kind, site: url, setting: r?.setting ?? null };
    }
    if (action === 'clear') {
      await api.clear({});
      audit.log({ action: 'browser.pref.clear', kind });
      return { kind, cleared: true };
    }
    const pattern = String(p.site || p.primary_pattern || '');
    if (!pattern) throw err('BAD_PARAMS', 'set needs site (a primary pattern like "https://example.com" or "https://*.example.com")');
    await api.set({ primaryPattern: pattern, setting: String(p.setting) });
    audit.log({ action: 'browser.pref.set', kind, pattern: redactUrl(pattern), setting: String(p.setting) });
    return { kind, site: pattern, setting: String(p.setting), note: 'content settings are browser-profile state — they persist until cleared' };
  });

  // ---- request interception (net.mock.*) ---------------------------------------

  R('net.mock.set', {
    category: 'network',
    timeoutMs: 15000,
    description: 'Intercept requests on the TARGET TAB matching a URL glob (net.mock.set {url_pattern:"https://api.example.com/v1/*", status:200, body:"...", content_type:"application/json", method?, headers?, once?}) and fulfill them locally — stub an API, force an error, block a tracker. Scope discipline: catch-all patterns are refused; the mock lives on the tab\'s debugger session and DIES with it (a navigation kills it — re-set after navigating, or check net.mock.list). A paused request with no matching rule is always continued, never wedged. Binary bodies: body:{b64:"..."}.',
  }, async (p = {}) => {
    // validate the pattern BEFORE demanding a session — a malformed call must
    // fail on its own shape, not on the tab it never reached
    const patternProbe = String(p.url_pattern || '').trim();
    if (!patternProbe) throw err('BAD_PARAMS', 'url_pattern is required and must be a glob like "https://api.example.com/v1/*" — an empty pattern would pause every request on the tab');
    if (/^\*+$/.test(patternProbe)) throw err('BAD_PARAMS', 'refusing a catch-all pattern ("*") — scope the mock to the URLs you actually need');
    const tabId = targetTab(p);
    const r = await setMock(tabId, p);
    audit.log({ action: 'net.mock.set', tabId, pattern: redactUrl(String(p.url_pattern || '')), status: r.status });
    return r;
  });

  R('net.mock.list', {
    category: 'network',
    description: 'Active interception rules for the target tab (id, pattern, method, status, hits). Empty list means no interception is armed — after a navigation that is the expected state.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    return { mocks: listMocks(tabId) };
  });

  R('net.mock.clear', {
    category: 'network',
    description: 'Remove interception rules: net.mock.clear {id} for one, or {} for all on the target tab. Fetch interception is disabled when the last rule goes.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const r = await clearMocks(tabId, p.id || null);
    audit.log({ action: 'net.mock.clear', tabId, cleared: r.cleared });
    return r;
  });

  // ---- credential channel (cookie.export / cookie.import) ----------------------
  // INTERNAL tools: only the daemon's session.transfer may call them — the
  // daemon refuses to forward them from an agent. Values ride the
  // daemon⇄extension link only, never a tool result, never the audit, never a
  // model context. The daemon refuses a transfer without confirm:true.

  R('cookie.export', {
    category: 'session',
    sensitive: true,
    description: 'INTERNAL — used by the daemon session.transfer only. Reads the cookies of ONE origin (CDP Network.getCookies on an attachable tab of this browser) and returns the raw cookie list including values. Direct agent calls are refused upstream (INTERNAL_ONLY).',
  }, async (p = {}) => {
    const origin = String(p.origin || '');
    if (!/^https?:\/\//i.test(origin)) throw err('BAD_PARAMS', 'origin must be an absolute http(s) URL');
    const tabId = await anyAttachableTab(p.tab_id);
    await cdp.ensure(tabId);
    const r = await cdp.send(tabId, 'Network.getCookies', { urls: [origin] });
    return { origin, cookies: r?.cookies || [] };
  });

  R('cookie.import', {
    category: 'session',
    sensitive: true,
    description: 'INTERNAL — used by the daemon session.transfer only. Writes a cookie list into this browser for ONE origin (CDP Network.setCookie per entry). Direct agent calls are refused upstream (INTERNAL_ONLY).',
  }, async (p = {}) => {
    const origin = String(p.origin || '');
    if (!/^https?:\/\//i.test(origin)) throw err('BAD_PARAMS', 'origin must be an absolute http(s) URL');
    const cookies = Array.isArray(p.cookies) ? p.cookies : [];
    if (!cookies.length) throw err('BAD_PARAMS', 'cookies must be a non-empty array');
    const tabId = await anyAttachableTab(p.tab_id);
    await cdp.ensure(tabId);
    let written = 0;
    const failed = [];
    for (const c of cookies) {
      if (!c?.name) continue;
      try {
        const ok = await cdp.send(tabId, 'Network.setCookie', {
          url: origin.replace(/\/+$/, '') + (String(c.path || '/').startsWith('/') ? c.path : '/'),
          name: c.name,
          value: String(c.value ?? ''),
          ...(c.domain ? { domain: c.domain } : {}),
          ...(c.path ? { path: c.path } : {}),
          ...(c.secure ? { secure: true } : {}),
          ...(c.httpOnly ? { httpOnly: true } : {}),
          ...(c.sameSite ? { sameSite: c.sameSite } : {}),
          ...(c.expires ? { expires: c.expires } : {}),
        });
        if (ok?.success !== false) written++;
        else failed.push(c.name);
      } catch {
        failed.push(c.name);
      }
    }
    return { origin, written, failed_names: failed, total: cookies.length };
  });
}

// ---- helpers for the tools above ---------------------------------------------

/** First attachable tab of this browser: an explicit tab_id if usable, else an
 *  active/focused http(s) tab, else the first http(s) tab. Restricted pages
 *  (chrome://) are skipped — cookie operations need a debugger attach. */
async function anyAttachableTab(preferred) {
  const okTab = async (id) => {
    if (id == null) return null;
    const t = await chrome.tabs.get(id).catch(() => null);
    if (!t || cdp.isRestrictedUrl(t.url)) return null;
    return id;
  };
  const explicit = await okTab(preferred);
  if (explicit != null) return explicit;
  const s = sessions.active();
  const owned = await okTab(s?.lastTabId);
  if (owned != null) return owned;
  const tabs = await chrome.tabs.query({ url: ['http://*/*', 'https://*/*'] }).catch(() => []);
  if (!tabs.length) throw err('NO_TAB', 'no http(s) tab in this browser to attach the cookie channel to');
  return (tabs.find((t) => t.active) || tabs[0]).id;
}

// Tab lifecycle ring for tab.events: closed tabs (any tab, url/title from the
// last onUpdated touch), page-opened tabs, main-frame commits. seq is
// monotonic per service-worker run; tab.events reports the seq so a caller can
// resume from where it last read. Metadata is cached at update time because a
// closed tab can no longer be asked for its own title.
export let tabEventSeq = 0;
const TAB_EVENT_MAX = 100;
export const tabEvents = [];
const tabMeta = new Map(); // tabId → { url, title }

export function noteTabMeta(tabId, { url, title } = {}) {
  if (tabId == null) return;
  const prev = tabMeta.get(tabId) || {};
  tabMeta.set(tabId, { url: url ?? prev.url, title: title ?? prev.title });
  if (tabMeta.size > 500) {
    // keep it bounded: drop the oldest third
    const keys = [...tabMeta.keys()].slice(0, Math.ceil(tabMeta.size / 3));
    for (const k of keys) tabMeta.delete(k);
  }
}

export function tabMetaOf(tabId) {
  return tabMeta.get(tabId) || { url: null, title: null };
}

export function recordTabEvent(kind, data) {
  tabEvents.push({ seq: ++tabEventSeq, kind, ...data });
  if (tabEvents.length > TAB_EVENT_MAX) tabEvents.splice(0, tabEvents.length - TAB_EVENT_MAX);
}

function tabEventRing() {
  return tabEvents;
}

function redactProbe(probe) {
  if (!probe || typeof probe !== 'object') return probe;
  return { ...probe, url: redactUrl(probe.url) };
}

export function registerMoreTools(dispatcher) {
  registerMore(dispatcher);
}
