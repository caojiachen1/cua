// UBB service worker entry: wires agent link ⇄ dispatcher ⇄ tools,
// content-script messages (interrupt/confirm), alarms keepalive.

import { AgentLink } from './lib/ws-client.js';
import { Dispatcher } from './lib/protocol.js';
import { registerTools } from './lib/tools.js';
import { registerExtras } from './lib/tools-extra.js';
import { registerMoreTools } from './lib/tools-more.js';
import { registerResearchTools } from './lib/tools-research.js';
import { registerInstitutionTools } from './lib/institution.js';
import { cdp } from './lib/cdp.js';
import { refs } from './lib/refs.js';
import { dropCapture } from './lib/snapshot.js';
import { sessions } from './lib/tabs.js';
import { tabRegistry } from './lib/tab-registry.js';
import { audit } from './lib/audit.js';
import { downloadLedger } from './lib/tools-downloads.js';
import { pendingConfirms, shield, rearmFrame, dropPasses } from './lib/shield-ctl.js';
import { clearBridgeWedge } from './lib/bridge-ctl.js';
import { STORAGE_KEYS, DEFAULTS, BROWSER_NAME_RE, err, randomHex } from './lib/config.js';
import { recordTabEvent, noteTabMeta, tabMetaOf } from './lib/tools-more.js';
import { redactUrl } from './lib/redact.js';
import { oopif } from './lib/oopif.js';
import { secReq } from './lib/secreq.js';
import { clearSessionGrants, revokeOrigin } from './lib/cdp-gate.js';

async function loadConfig() {
  const st = await chrome.storage.local.get([
    STORAGE_KEYS.token,
    STORAGE_KEYS.port,
    STORAGE_KEYS.enabled,
    STORAGE_KEYS.browserName,
    STORAGE_KEYS.workspace,
  ]);
  return {
    token: st[STORAGE_KEYS.token],
    port: st[STORAGE_KEYS.port] || DEFAULTS.port,
    enabled: st[STORAGE_KEYS.enabled] !== false && !!st[STORAGE_KEYS.token],
    browser_name: BROWSER_NAME_RE.test(st[STORAGE_KEYS.browserName] || '') ? st[STORAGE_KEYS.browserName] : DEFAULTS.browserName,
    // daemon-launched workspaces identify themselves so nameless routing can
    // prefer the user's real logged-in browser over an isolated profile
    workspace: st[STORAGE_KEYS.workspace] === true,
  };
}

const link = new AgentLink(loadConfig);
const dispatcher = new Dispatcher();
registerTools(dispatcher);
registerExtras(dispatcher);
registerMoreTools(dispatcher);
registerResearchTools(dispatcher);
registerInstitutionTools(dispatcher);

// Persistent UBB tab lifecycle: load the registry and reconcile it with the
// live tab strip once the session state is restored. Orphan UBB-group tabs
// are remembered as `orphaned` instead of being forgotten.
sessions.ready().then(() => tabRegistry.reconcile({ activeSessionId: sessions.active()?.id || null })).catch(() => {});

// session.stop 清 session 档的原始 CDP origin 批准（契约 §4）。tabs.js 不在本次
// 改动范围 —— 这里在 background 包一层：stop 真正停掉会话时清 grant。
// CORE-2 若在 tabs.js 的 stop() 内接入等价清理，删掉这个包装即可。
const _sessionsStop = sessions.stop.bind(sessions);
sessions.stop = async function (opts) {
  const r = await _sessionsStop(opts);
  if (r && r.stopped) await clearSessionGrants().catch(() => {});
  return r;
};

// audit.seq must be re-hydrated from chrome.storage.session BEFORE the daemon
// link opens: a tool dispatched against the pre-hydrate zero would hand the
// tailer a fresh-from-0 seq and the CLI would read it as a ring wraparound.
// (hydrateSeq is idempotent — safe to await from every link.start entry point.)
function startLinkHydrated() {
  audit.hydrateSeq().catch(() => {}).then(() => link.start());
}

dispatcher.onFinish = (name, params, ok, ms, errorCode) => {
  audit.log({ action: name, ok, ms, error: errorCode, tab_id: params?.tab_id });
  // a refusal caused BY the pause must not repaint the group as "running"
  if (!ok && sessions.active() && !sessions.isPaused()) sessions.setStatus('error').then(() => sessions.setStatus('running'));
};

// inbound from daemon → dispatch → reply
// The restore barrier is part of request admission: a call that arrives while
// chrome.storage.session is still being read must not race a half-restored
// session that is replaced microseconds later.
link.handler = async (msg) => {
  await sessions.ready();
  const reply = await dispatcher.handle(msg);
  if (reply) link.send(reply);
};

// ---------------------------------------------------------------- lifecycle

chrome.runtime.onInstalled.addListener(async () => {
  const st = await chrome.storage.local.get([STORAGE_KEYS.token]);
  if (!st[STORAGE_KEYS.token]) {
    await chrome.storage.local.set({ [STORAGE_KEYS.token]: randomHex(32) });
  }
});

chrome.runtime.onStartup.addListener(startLinkHydrated);
// 浏览器冷启动：清掉 session 档的原始 CDP origin 批准（once 用后即删、
// forever 保留；session.stop 处也会清 —— 见下方 sessions.stop 包装）
chrome.runtime.onStartup.addListener(() => {
  clearSessionGrants().catch(() => {});
});
chrome.alarms.create('ubb-keepalive', { periodInMinutes: 0.5 });

// ── native auto-pairing (channel-bound trust) ────────────
// `ubb install` registers a native messaging host whose manifest pins THIS
// extension's id — the browser refuses any other origin, so nothing on the
// machine can impersonate us. At every service-worker start we ask the host
// for the current token/port (it also ensures the daemon runs). The host is
// the source of truth; when the host isn't installed the manual token flow
// from the popup keeps working unchanged.
let nativePairInFlight = false;
async function nativePair() {
  if (nativePairInFlight) return;
  nativePairInFlight = true;
  try {
    await nativePairInner();
  } finally {
    nativePairInFlight = false;
  }
}

async function nativePairInner() {
  if (typeof chrome.runtime.connectNative !== 'function') return; // no nativeMessaging permission
  const port = chrome.runtime.connectNative('com.ubb.bridge');
  try {
    const reply = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('pair timeout')), 5000);
      port.onMessage.addListener((m) => { clearTimeout(timer); resolve(m); });
      port.onDisconnect.addListener(() => { clearTimeout(timer); reject(new Error(chrome.runtime.lastError?.message || 'host gone')); });
      port.postMessage({ op: 'pair' });
    });
    if (reply?.ok && typeof reply.token === 'string' && reply.token.length >= 32) {
      const st = await chrome.storage.local.get([STORAGE_KEYS.token, STORAGE_KEYS.port]);
      if (st[STORAGE_KEYS.token] !== reply.token || st[STORAGE_KEYS.port] !== (reply.port || 8765)) {
        await chrome.storage.local.set({
          [STORAGE_KEYS.token]: reply.token,
          [STORAGE_KEYS.port]: reply.port || 8765,
        });
        audit.log({ action: 'native.pair', note: 'token/port synced from native host' });
        link.restart(); // reconnect with the paired credentials
      }
    }
  } catch (e) {
    audit.log({ action: 'native.pair', note: String(e?.message || e).slice(0, 80) });
  } finally {
    try { port.disconnect(); } catch {}
  }
}
// ── agent-workspace window hygiene ───────────────────────
// A browser.launch / auto-spawned instance is an agent workspace: it must not
// steal the user's focus and must not leave the pairing page behind. The
// daemon spawns it with --start-minimized; these two paths are the fallback
// for builds that ignore that switch, and the pair-tab cleanup in both cases.

/** Run at the moment a pair lands (fresh profile): minimize the window the
 *  pair page sits in if it took focus, then clear the page itself. */
async function workspaceHygiene(sender) {
  try {
    const tab = sender?.tab;
    if (tab?.windowId == null) return;
    const w = await chrome.windows.get(tab.windowId).catch(() => null);
    if (w?.focused) await chrome.windows.update(tab.windowId, { state: 'minimized' }).catch(() => {});
    if (tab.id != null) await chrome.tabs.update(tab.id, { url: 'about:blank' }).catch(() => {});
  } catch {}
}

/** Run at every service-worker start of a workspace profile: if a window of
 *  this instance came up focused (relaunch of an existing profile), minimize
 *  it once. The storage.session flag makes it ONCE PER BROWSER RUN — a
 *  window the user deliberately raises afterwards is never fought again.
 *  No window seen yet on a cold start → retried on the next SW wake (the
 *  keepalive alarm fires every 30s) until one appears. */
let relaxInFlight = false;
async function relaxWorkspaceWindow() {
  if (relaxInFlight) return;
  relaxInFlight = true;
  try {
    const st = await chrome.storage.local.get(STORAGE_KEYS.workspace);
    if (st[STORAGE_KEYS.workspace] !== true) return;
    const ss = await chrome.storage.session.get('ubb_ws_relaxed');
    if (ss.ubb_ws_relaxed) return;
    for (let attempt = 0; attempt < 5; attempt++) {
      const wins = await chrome.windows.getAll({ populate: false, windowTypes: ['normal'] }).catch(() => []);
      if (wins.length) {
        for (const w of wins) {
          if (w.focused) await chrome.windows.update(w.id, { state: 'minimized' }).catch(() => {});
        }
        await chrome.storage.session.set({ ubb_ws_relaxed: true });
        return;
      }
      await new Promise((r) => setTimeout(r, 500));
    }
  } catch {} finally {
    relaxInFlight = false;
  }
}

nativePair();

startLinkHydrated();
sessions.restore(); // survive SW restarts
relaxWorkspaceWindow();

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === 'ubb-keepalive') {
    loadConfig().then((c) => {
      if (c.enabled && link.status === 'off') link.restart();
    });
    nativePair(); // self-heal ≤30s after a token rotation — a failing-auth WS
    // never idles the SW out, so a start-time-only pair would never re-run
  }
});

// ---------------------------------------------------------------- debugger events

chrome.debugger.onEvent.addListener((source, method, params) => {
  cdp.handleEvent(source, method, params);
});

cdp.onNavigation = (tabId) => {
  refs.bump(tabId);
  link.emit('tab.navigated', { tab_id: tabId });
};

// mid-command detach reported by the CDP hub (no chrome.debugger event may
// have fired, e.g. a stale "not attached" reply) — refs are stale either way
cdp.onDetach = (tabId) => {
  refs.invalidate(tabId);
  dropCapture(tabId);
  oopif.detachTab(tabId); // 该 tab 的 OOPIF target 会话一并清掉（30s 空闲清扫兜底）
};

chrome.debugger.onDetach.addListener(async (source) => {
  cdp.sessions.delete(source.tabId);
  refs.invalidate(source.tabId);
});

// Re-arm the shield after every completed navigation of a session tab:
// content scripts reset on navigation, and tab.create arms too early (the
// script is not loaded yet) — onCompleted is the reliable point.
chrome.webNavigation.onCompleted.addListener(async ({ tabId, frameId }) => {
  if (frameId !== 0) return;
  clearBridgeWedge(tabId, 0); // fresh document → the bridge gets a fresh chance
  const s = sessions.active();
  if (s && sessions.owns(tabId) && s.paused !== true) {
    await shield.set(tabId, { mode: 'armed', label: 'UBB agent control' });
  }
});

// Late-loaded iframe closure (log-D §5): an SPA-inserted subframe commits
// without any top-frame navigation, so the arm broadcast above never reaches
// its frameblock listener — the shield stays lit while the user could type
// into the new frame. Address exactly that frameId with the QUERIED mode
// (rearmFrame never invents an arm: a paused tab stays paused).
chrome.webNavigation.onCommitted.addListener(({ tabId, frameId }) => {
  if (frameId === 0) return; // main-frame arming stays on the onCompleted path
  // a committed document is a fresh content script: lift any bridge cooldown
  clearBridgeWedge(tabId, frameId);
  const s = sessions.active();
  if (!s || !sessions.owns(tabId)) return;
  // fix5 §F3-⑥: this used to end in `.catch(() => {})`, which threw away the one
  // thing rearmFrame exists to produce — whether that frame is actually armed.
  // The outcome (sent / reason / mode / error) goes to the audit ring, so
  // `ubb audit tail` shows an un-armed frame instead of a silent nothing-happened.
  Promise.resolve()
    .then(() => rearmFrame(tabId, frameId))
    .then((r) => audit.log({ action: 'shield.rearm_frame', tab_id: tabId, frame_id: frameId, sent: !!(r && r.sent), reason: r && r.sent ? 'armed' : String((r && r.reason) || 'no_result'), mode: (r && r.mode) || null, error: (r && r.error) || undefined }))
    .catch((e) => audit.log({ action: 'shield.rearm_frame', tab_id: tabId, frame_id: frameId, sent: false, reason: 'rearm_threw', error: String(e?.message || e).slice(0, 160) }));
});

chrome.tabs.onRemoved.addListener((tabId) => {
  cdp.forget(tabId);
  refs.invalidate(tabId);
  clearBridgeWedge(tabId);
  // a tab that no longer exists cannot be told to re-block: forget its open pass
  // transactions instead of leaving timers that will address a dead tab
  dropPasses(tabId);
  const s = sessions.active();
  if (s) {
    s.created.delete(tabId);
    s.claimed.delete(tabId);
    if (s.lastTabId === tabId) s.lastTabId = [...s.created][s.created.size - 1] ?? [...s.claimed][0] ?? null;
  }
  // steer feed: a closed tab (agent's or user's) is exactly the fact a running
  // plan must reconcile against — emit to the daemon AND ring it for tab.events.
  // Metadata comes from the onUpdated cache (a removed tab cannot be asked).
  const meta = tabMetaOf(tabId);
  const ev = { tab_id: tabId, url: meta.url ? redactUrl(meta.url) : null, title: meta.title || null };
  recordTabEvent('closed', ev);
  link.emit('tab.closed', ev);
});

// ---------------------------------------------------------------- download watch
// The agent's own `download` tool only sees explicit url/href downloads. Chrome's
// downloads API also fires for blob:/server-generated ones the agent never asked
// for, so keep a ring the download.* tools can read and push lifecycle events.
// The callback argument is a DownloadItem, and that type HAS NO tabId (only
// DownloadDelta does, and this build never filled it either — LIVE-RESULTS-3B §3b),
// so nothing here may expect a tab from the browser: attribution comes from the
// download tool's claim, from a delta that really carries one, or from the
// heuristic. Which signals this browser actually supplies is counted in the ledger
// and shown by download.list as `browser_signals`, not guessed in a description.
downloadLedger.setEmitter((event, params) => link.emit(event, params));
chrome.downloads.onCreated.addListener((item) => downloadLedger.created(item));
chrome.downloads.onChanged.addListener((delta) => {
  downloadLedger.changed(delta).catch(() => {});
});

// Tabs opened from a session tab (target=_blank / window.open / a page's
// "open in new tab") are not session-owned, but they were caused by our
// actions. Remember them so tab.close_spawned / session.stop {close_spawned}
// can put the browser back the way it was.
chrome.tabs.onCreated.addListener((tab) => {
  if (tab?.openerTabId != null && sessions.owns(tab.openerTabId)) {
    sessions.noteSpawned(tab.id).catch(() => {});
    const ev = { tab_id: tab.id, opener_tab_id: tab.openerTabId, url: tab.url ? redactUrl(tab.url) : null, title: tab.title || null };
    recordTabEvent('created', ev);
    link.emit('tab.opened', ev);
  } else {
    noteTabMeta(tab?.id, { url: tab?.url, title: tab?.title });
  }
});

// Keep the persistent registry in step with the browser.
chrome.tabs.onRemoved.addListener((tabId) => {
  tabRegistry.unregister(tabId).catch(() => {});
});
chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.url || change.title || change.groupId) {
    tabRegistry.touch(tabId, { url: tab.url, title: tab.title, groupId: tab.groupId }).catch(() => {});
  }
  // steer-feed metadata cache: what tab.events can honestly say about a tab
  // once it is gone (a closed tab cannot be asked for its own title)
  if (change.url || change.title) {
    noteTabMeta(tabId, { url: tab.url, title: tab.title });
  }
});

// ---------------------------------------------------------------- extension messages

// Web-page channel: manifest externally_connectable admits http://localhost/* and
// http://127.0.0.1/* pages only, and every message must carry the same token the
// daemon uses — so a page served from any local port can drive UBB without the CLI.
chrome.runtime.onMessageExternal.addListener((msg, sender, sendResponse) => {
  (async () => {
    const origin = sender.origin || sender.url || '';
    if (!/^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(origin)) {
      return { ok: false, error: 'origin not allowed: ' + origin };
    }
    if (!msg) return { ok: false, error: 'no message' };

    // ---- pairing for `browser.launch` instances ─────────────────────────────
    // The daemon serves /__ubb_pair?nonce=<N> (nonce single-use, 120s TTL,
    // minted only by `browser.launch` and the workspace auto-launch). The page
    // it renders carries the daemon's own token/port and the slot name for the
    // freshly launched profile. The token arrives here over the SAME channel
    // the daemon's /shutdown?token= gate already trusts: loopback + a 128-bit
    // single-use nonce. A page without the nonce gets a 404 from the daemon,
    // so the credentials never reach an uninvited origin.
    if (msg.type === 'UBB_PAIR_LAUNCH') {
      const st = await chrome.storage.local.get([STORAGE_KEYS.enabled]);
      if (st[STORAGE_KEYS.enabled] === false) return { ok: false, error: 'UBB disabled' };
      const okShape =
        BROWSER_NAME_RE.test(msg.name || '') &&
        /^[0-9a-f]{32,128}$/i.test(msg.token || '') &&
        Number.isInteger(msg.port) && msg.port > 0 && msg.port < 65536 &&
        /^[0-9a-f]{32,128}$/i.test(msg.nonce || '');
      if (!okShape) return { ok: false, error: 'bad pair payload shape' };
      const patch = {
        [STORAGE_KEYS.browserName]: msg.name.toLowerCase(),
        [STORAGE_KEYS.port]: msg.port,
        // this profile is a daemon-launched agent workspace: it must stay in
        // the background (relaxWorkspaceWindow runs once per browser start)
        [STORAGE_KEYS.workspace]: true,
      };
      const st2 = await chrome.storage.local.get([STORAGE_KEYS.token]);
      if (st2[STORAGE_KEYS.token] !== msg.token.toLowerCase()) {
        // fresh profile: the onInstalled random token is replaced by the
        // daemon's real one (nativePair converges on the same value anyway)
        patch[STORAGE_KEYS.token] = msg.token.toLowerCase();
      }
      await chrome.storage.local.set(patch);
      await link.restart();
      await workspaceHygiene(sender);
      audit.log({ action: 'pair.launch', browser: msg.name.toLowerCase(), origin });
      return { ok: true, paired_as: msg.name.toLowerCase() };
    }

    if (msg.type !== 'UBB_WEB_RPC') {
      return { ok: false, error: 'unknown message type' };
    }
    const st = await chrome.storage.local.get([STORAGE_KEYS.token, STORAGE_KEYS.enabled]);
    if (st[STORAGE_KEYS.enabled] === false) return { ok: false, error: 'UBB disabled' };
    if (!st[STORAGE_KEYS.token] || msg.token !== st[STORAGE_KEYS.token]) {
      return { ok: false, error: 'UNAUTHORIZED: token mismatch' };
    }
    const method = msg.method || (msg.tool ? 'tool.' + msg.tool : null);
    if (!method) return { ok: false, error: 'method/tool required' };
    audit.log({ action: 'web.rpc', origin, tool: method.replace(/^tool\./, '') });
    await sessions.ready();
    const reply = await dispatcher.handle({ id: 'web-' + Date.now() + '-' + randomHex(4), method, params: msg.params || {} });
    if (reply && reply.error) return { ok: false, error: reply.error.message || String(reply.error) };
    return { ok: true, result: reply && reply.result };
  })().then((r) => sendResponse(r)).catch((e) => sendResponse({ ok: false, error: String(e && e.message || e) }));
  return true; // async sendResponse
});

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  (async () => {
    switch (msg?.type) {
      // --- from content/shield.js ---
      case 'UBB_INTERRUPT': {
        const n = dispatcher.cancelAll();
        const tabId = sender.tab?.id;
        // user revoked control: the session stays paused (targetTab refuses)
        // until a human re-arms; a navigation completing must not resume it
        await sessions.pause();
        if (tabId != null) {
          // user took over: pause blocking on that tab until the next arm
          // (shield is statically imported at the top of this file; dynamic
          // import() is not allowed on ServiceWorkerGlobalScope)
          await shield.set(tabId, { mode: 'paused', label: '已由用户中断' });
        }
        link.emit('session.interrupted', { tab_id: tabId, cancelled: n });
        sendResponse({ ok: true, cancelled: n });
        break;
      }
      case 'UBB_RESUME': {
        const tabId = sender.tab?.id;
        const resumed = await sessions.resume();
        if (tabId != null) await shield.set(tabId, { mode: 'armed', label: 'UBB agent control' });
        link.emit('session.resumed', { tab_id: tabId });
        sendResponse({ ok: true, resumed });
        break;
      }
      case 'UBB_CONFIRM_RESULT': {
        const p = pendingConfirms.get(msg.reqId);
        if (p) {
          pendingConfirms.delete(msg.reqId);
          p.resolve(!!msg.ok);
        }
        sendResponse({ ok: true });
        break;
      }
      case 'UBB_HELP_RESULT': {
        shield.helpResult(msg.reqId, msg.ok);
        sendResponse({ ok: true });
        break;
      }
      case 'UBB_REC_SAVE': {
        // content-script recorder handoff: survives cross-origin navigations
        (async () => {
          try {
            if (!msg.rec) await chrome.storage.session.remove('ubb_rec_last');
            else if (JSON.stringify(msg.rec).length <= 2 * 1024 * 1024) {
              await chrome.storage.session.set({ ubb_rec_last: msg.rec });
            }
            sendResponse({ ok: true });
          } catch { sendResponse({ ok: false }); }
        })();
        return true;
      }
      case 'UBB_REC_LOAD': {
        (async () => {
          try {
            const st = await chrome.storage.session.get('ubb_rec_last');
            const rec = st.ubb_rec_last || null;
            if (rec) await chrome.storage.session.remove('ubb_rec_last');
            sendResponse({ ok: true, rec });
          } catch { sendResponse({ ok: false }); }
        })();
        return true;
      }

      // --- from popup ---
      case 'UBB_POPUP_STATE': {
        const cfg = await loadConfig();
        await sessions.ready();
        // Codex absorb wave（契约 §3）：守卫/光标/原始CDP/严格档 + origin 批准表
        // + secret.request 元数据（无值）
        const wave = await chrome.storage.local.get([
          'ubb_guard_mode', 'ubb_cursor_enabled', 'ubb_cdp_raw', 'ubb_policy_strict', 'ubb_cdp_origins',
        ]);
        sendResponse({
          connected: link.status === 'connected',
          status: link.status,
          instance_id: link.instanceId,
          browser_name: cfg.browser_name,
          session: sessions.active()
            ? { id: sessions.active().id, created: sessions.active().created.size, claimed: sessions.active().claimed.size }
            : null,
          port: cfg.port,
          enabled: cfg.enabled,
          has_token: !!cfg.token,
          token: cfg.token || '(not generated)',
          shield_mode: (await chrome.storage.local.get(STORAGE_KEYS.shieldMode))[STORAGE_KEYS.shieldMode] || 'block',
          human_mode: (await chrome.storage.local.get('ubb_human_mode')).ubb_human_mode === true,
          guard_mode: wave.ubb_guard_mode || 'session',
          cursor_enabled: wave.ubb_cursor_enabled !== false,
          cdp_raw: wave.ubb_cdp_raw === true,
          policy_strict: wave.ubb_policy_strict === true,
          cdp_origins: wave.ubb_cdp_origins && typeof wave.ubb_cdp_origins === 'object' ? wave.ubb_cdp_origins : {},
          sec_requests: secReq.listMeta(),
          audit_tail: audit.list(5),
        });
        break;
      }
      case 'UBB_POPUP_CONFIG': {
        const patch = {};
        if (msg.port) patch[STORAGE_KEYS.port] = Number(msg.port) || DEFAULTS.port;
        if (typeof msg.enabled === 'boolean') patch[STORAGE_KEYS.enabled] = msg.enabled;
        if (typeof msg.human === 'boolean') patch.ubb_human_mode = msg.human;
        if (msg.shield_mode) patch[STORAGE_KEYS.shieldMode] = msg.shield_mode;
        // Codex absorb wave（契约 §3）五个新 patch 键：只写 storage，不碰 daemon 链路
        if (msg.guard_mode === 'session' || msg.guard_mode === 'always' || msg.guard_mode === 'off') {
          patch.ubb_guard_mode = msg.guard_mode;
        }
        if (typeof msg.cursor_enabled === 'boolean') patch.ubb_cursor_enabled = msg.cursor_enabled;
        if (typeof msg.cdp_raw === 'boolean') patch.ubb_cdp_raw = msg.cdp_raw;
        if (typeof msg.policy_strict === 'boolean') patch.ubb_policy_strict = msg.policy_strict;
        if (typeof msg.browser_name === 'string') {
          const name = msg.browser_name.trim().toLowerCase();
          if (!BROWSER_NAME_RE.test(name)) {
            sendResponse({ ok: false, error: '浏览器名只允许 a-z 0-9 _ -（1–32 字符）' });
            break;
          }
          patch[STORAGE_KEYS.browserName] = name;
        }
        if (typeof msg.token === 'string' && /^[0-9a-f]{16,128}$/i.test(msg.token)) {
          patch[STORAGE_KEYS.token] = msg.token.toLowerCase();
        }
        await chrome.storage.local.set(patch);
        // cdp_revoke_origin：从批准表删键（独立于 patch —— 撤销不需要任何落盘顺序保证）
        if (typeof msg.cdp_revoke_origin === 'string' && msg.cdp_revoke_origin) {
          await revokeOrigin(msg.cdp_revoke_origin).catch(() => {});
        }
        // 只有会改变连接身份/行为的旧键才值得重启 daemon 链路；新键（守卫/光标/
        // CDP 开关/严格档）只写 storage 即时生效 —— 空 patch + 单独撤销也不重启
        const RESTART_KEYS = new Set([
          STORAGE_KEYS.port, STORAGE_KEYS.enabled, 'ubb_human_mode',
          STORAGE_KEYS.shieldMode, STORAGE_KEYS.browserName, STORAGE_KEYS.token,
        ]);
        if (Object.keys(patch).some((k) => RESTART_KEYS.has(k))) await link.restart();
        sendResponse({ ok: true });
        break;
      }
      case 'UBB_POPUP_SECREQ_SUBMIT': {
        // secret.request 的值走 popup→background：只在内存流转（secreq.js），
        // 不落盘、不进审计 —— 这里只回执是否终结了一个等待中的请求
        const accepted = secReq.submit(String(msg.id || ''), msg.values && typeof msg.values === 'object' ? msg.values : {});
        sendResponse({ ok: true, accepted });
        break;
      }
      case 'UBB_POPUP_SECREQ_CANCEL': {
        const accepted = secReq.cancel(String(msg.id || ''));
        sendResponse({ ok: true, accepted });
        break;
      }
      case 'UBB_POPUP_REGEN_TOKEN': {
        await chrome.storage.local.set({ [STORAGE_KEYS.token]: randomHex(32) });
        await link.restart();
        sendResponse({ ok: true });
        break;
      }
      default:
        sendResponse({ ok: false, error: 'unknown message' });
    }
  })();
  return true; // async response
});

// expose for debugging on the SW console
self.__ubb = { link, dispatcher, cdp, sessions, err };
