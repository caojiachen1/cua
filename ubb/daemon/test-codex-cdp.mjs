// Offline self-check for the Codex-absorb CDP wave (CORE-1) — no browser:
//   node daemon/test-codex-cdp.mjs
// Covers: cdp-gate 黑名单判定矩阵、full 档决策矩阵（开关 × origin 状态 × 方法类）、
// banner 两轮批准/拒绝路径、OOPIF target 匹配与 LRU/空闲清理（stub
// chrome.debugger.getTargets）、cdp.events 游标语义（真实 ring）、secreq 契约。

import { installChromeStub } from '../lib/testharness.mjs';
installChromeStub();

let pass = 0;
let fail = 0;
const ok = (name, cond, detail = '') => {
  if (cond) { pass++; console.log(`  ✓ ${name}`); }
  else { fail++; console.log(`  ✗ ${name}${detail ? ` — ${detail}` : ''}`); }
};
async function grab(f) {
  try { return { r: await f(), e: null }; } catch (e) { return { r: null, e }; }
}
const describe = (x) => (x == null ? 'it returned instead of throwing' : `got ${x.code || 'plain throw'}: ${String(x.message || x).slice(0, 160)}`);

// ── 可控 chrome 表面（installChromeStub 之上替换成可编程版本）─────────────────
const localStore = new Map();
const sessionStore = new Map();
const mapStorage = (store) => ({
  get: async (keys) => {
    const out = {};
    const list = keys == null ? [...store.keys()] : Array.isArray(keys) ? keys : [keys];
    for (const k of list) if (store.has(k)) out[k] = store.get(k);
    return out;
  },
  set: async (obj) => { for (const [k, v] of Object.entries(obj || {})) store.set(k, String(k).includes('origins') || true ? v : v); },
  remove: async (keys) => { for (const k of Array.isArray(keys) ? keys : [keys]) store.delete(k); },
});
chrome.storage.local = mapStorage(localStore);
chrome.storage.session = mapStorage(sessionStore);

const TAB_URL = 'https://parent.example/';
const ORIGIN = 'https://parent.example';

// 扩展 frame 列表（webNavigation 命名空间）：2/4..11 是 OOPIF，3 在页面会话
// 的 frame 树里（同进程）
const extFrames = [
  { frameId: 0, parentFrameId: -1, url: TAB_URL },
  { frameId: 2, parentFrameId: 0, url: 'https://child.example/inner' },
  { frameId: 3, parentFrameId: 0, url: 'https://sameproc.example/sib' },
  ...[4, 5, 6, 7, 8, 9, 10, 11].map((n) => ({ frameId: n, parentFrameId: 0, url: `https://f${n}.example/x` })),
];
const FRAME_TREE = {
  frameTree: {
    frame: { id: 'MAIN', url: TAB_URL },
    childFrames: [{ frame: { id: 'F4CDP', parentId: 'MAIN', url: 'https://sameproc.example/sib' } }],
  },
};

let oopTargets = [];
const iframeTargetFor = (id, url) => ({ targetId: `T-${id}`, type: 'iframe', url, attached: false });
const attachLog = [];
const detachLog = [];
const attachedTargets = new Set();
const tabCmds = [];
const targetCmds = [];
let fileInputNodeIds = [101];
const setFileCalls = [];
const sessToTarget = new Map(); // sessionId → targetId（隧道桩）
let midFlightOrigins = null;

chrome.tabs.get = async (id) => ({ id, windowId: 1, url: id === 77 ? 'chrome://settings/' : TAB_URL, title: 'stub tab' });
let sendMessageCalls = 0;
chrome.tabs.sendMessage = async (tabId, payload) => { sendMessageCalls += 1; return {}; };
chrome.webNavigation.getAllFrames = async () => extFrames.map((f) => ({ ...f }));

chrome.debugger.attach = async (target = {}) => {
  if (target.tabId != null) return; // 页面会话 attach（lib/cdp.js 的路径）
  if (target.targetId) {
    if (target.targetId === 'T-CONFLICT') throw new Error('Another debugger is already attached to this target');
    attachedTargets.add(target.targetId);
    attachLog.push(target.targetId);
    return;
  }
};
chrome.debugger.detach = async (target = {}) => {
  if (target.tabId != null) return;
  if (target.targetId) {
    attachedTargets.delete(target.targetId);
    detachLog.push(target.targetId);
  }
};
chrome.debugger.getTargets = async () => oopTargets;
chrome.debugger.sendCommand = async (source, method, params = {}) => {
  if (source && source.tabId != null) {
    tabCmds.push({ method, params });
    if (method === 'Target.setAutoAttach') {
      // 模拟浏览器对相关 OOPIF 的 auto-attach：逐个发 attachedToTarget 事件
      if (oopTargets.some((t) => t.targetId === 'T-CONFLICT')) throw new Error('Another debugger is already attached to this target');
      for (const t of oopTargets) {
        attachLog.push(t.targetId);
        const sid = 'sess-' + t.targetId;
        sessToTarget.set(sid, t.targetId);
        for (const h of cdp.eventHooks) h(source.tabId, 'Target.attachedToTarget', { sessionId: sid, targetInfo: { targetId: t.targetId, type: t.type, url: t.url } });
      }
      return {};
    }
    if (method === 'Target.attachToTarget') {
      if (params.targetId === 'T-CONFLICT') throw new Error('Another debugger is already attached to this target');
      const sid = 'sess-' + params.targetId;
      sessToTarget.set(sid, params.targetId);
      attachLog.push(params.targetId);
      return { sessionId: sid };
    }
    if (method === 'Target.sendMessageToTarget') {
      const msg = JSON.parse(params.message);
      const tid = sessToTarget.get(params.sessionId) || null;
      targetCmds.push({ method: msg.method, params: msg.params, targetId: tid });
      let result = {};
      if (msg.method === 'Runtime.evaluate') result = { result: { value: 42 } };
      else if (msg.method === 'Page.getFrameTree') result = FRAME_TREE; // isolated world 需要 child 自己的 frameId
      else if (msg.method === 'Page.createIsolatedWorld') { if (msg.params.frameId == null) { for (const h of cdp.eventHooks) h(source.tabId, 'Target.receivedMessageFromTarget', { sessionId: params.sessionId, message: JSON.stringify({ id: msg.id, error: { message: 'Invalid parameters: mandatory field frameId missing' } }) }); return {}; } result = { executionContextId: 99 }; }
      else if (msg.method === 'DOM.getDocument') result = { root: { nodeId: 100 } };
      else if (msg.method === 'DOM.querySelectorAll') result = { nodeIds: fileInputNodeIds.slice() };
      else if (msg.method === 'DOM.getAttributes') result = { attributes: msg.params.nodeId === 101 ? ['type', 'file', 'id', 'resume'] : ['type', 'file'] };
      else if (msg.method === 'DOM.setFileInputFiles') { setFileCalls.push({ ...msg.params, targetId: tid }); result = {}; }
      // 模拟 Target.receivedMessageFromTarget 事件流回（模块尾钩子按 pendingById 分拣）
      for (const h of cdp.eventHooks) h(source.tabId, 'Target.receivedMessageFromTarget', { sessionId: params.sessionId, message: JSON.stringify({ id: msg.id, result }) });
      return {};
    }
    if (method === 'Target.detachFromTarget') {
      detachLog.push(sessToTarget.get(params.sessionId));
      return {};
    }
    if (method === 'Page.getFrameTree') return FRAME_TREE;
    if (method === 'DOM.getDocument') return { root: { nodeId: 7 } };
    if (method === 'Page.createIsolatedWorld') return { executionContextId: 55 };
    if (method === 'Runtime.evaluate') return { result: { value: 'main-tab-eval' } };
    if (method === 'Browser.getVersion') {
      // 命令出线的一刻快照批准表 —— "once 用后即删"的中间态证据
      midFlightOrigins = JSON.parse(JSON.stringify(localStore.get('ubb_cdp_origins') || {}));
      return { product: 'stub-browser' };
    }
    return {};
  }
  if (source && source.targetId) {
    if (!attachedTargets.has(source.targetId)) throw new Error('Not attached to target ' + source.targetId);
    targetCmds.push({ method, params, targetId: source.targetId });
    if (method === 'Runtime.evaluate') return { result: { value: 42 } };
    if (method === 'Page.createIsolatedWorld') return { executionContextId: 99 };
    if (method === 'DOM.getDocument') return { root: { nodeId: 100 } };
    if (method === 'DOM.querySelectorAll') return { nodeIds: fileInputNodeIds.slice() };
    if (method === 'DOM.getAttributes') {
      return { attributes: params.nodeId === 101 ? ['type', 'file', 'id', 'resume'] : ['type', 'file'] };
    }
    if (method === 'DOM.setFileInputFiles') { setFileCalls.push({ ...params, targetId: source.targetId }); return {}; }
    return {};
  }
  throw new Error('bad debugger source: ' + JSON.stringify(source));
};

// ── 被测模块（chrome 表面就绪后再 import）────────────────────────────────────
const gate = await import('../lib/cdp-gate.js');
const oop = await import('../lib/oopif.js');
const { secReq } = await import('../lib/secreq.js');
const { cdp } = await import('../lib/cdp.js');
const { sessions } = await import('../lib/tabs.js');
const { refs } = await import('../lib/refs.js');
const { pendingConfirms } = await import('../lib/shield-ctl.js');

// targetTab 需要一个拥有 tab 1 的活动会话
sessions.session = {
  id: 'stub-session',
  groupId: null,
  created: new Set([1]),
  claimed: new Set(),
  spawned: new Set(),
  lastTabId: 1,
  startedAt: Date.now(),
  starterTabId: null,
  name: 'stub',
  ownWindowId: null,
  ownWindowWanted: false,
  paused: false,
  marks: new Map(),
};

// 工具注册表（只挂 tools.js —— cdp.call / cdp.events 在这里）
const tools = new Map();
{
  const dispatcher = { register: (name, meta, fn) => tools.set(name, { meta, fn }) };
  const { registerTools } = await import('../lib/tools.js');
  registerTools(dispatcher);
}
const call = (name, params = {}) => tools.get(name).fn(params);

// banner 两轮的应答器：按 policy 依次 resolve pendingConfirms（耗尽后一律 false）
function resolveConfirms(policy) {
  const t = setInterval(() => {
    for (const [k, p] of pendingConfirms) {
      p.resolve(policy.length ? policy.shift() : false);
      pendingConfirms.delete(k);
    }
  }, 5);
  return () => clearInterval(t);
}

const DEFAULT_DENY_MSG =
  'method not whitelisted; read: Page.getFrameTree, Page.getLayoutMetrics, Page.getNavigationHistory, Page.captureScreenshot…; write: Input.dispatchMouseEvent, Input.dispatchKeyEvent, Input.insertText, Page.navigate…';

// ============================================================ 1. 黑名单矩阵
console.log('\n[1] cdp-gate 黑名单矩阵');
{
  const blocked = [
    'Browser.close', 'Browser.crash', 'Page.crash', 'Target.closeTarget', 'Target.createTarget',
    'Target.attachToTarget', 'Target.setAutoAttach', 'Debugger.enable', 'Debugger.setBreakpoint',
    'Fetch.enable', 'Fetch.fulfillRequest', 'Network.setRequestInterception',
    'Page.setBypassCSP', 'Page.setDownloadBehavior', 'Browser.setDownloadBehavior',
    'Storage.clearDataForOrigin', 'Storage.clearCookies',
  ];
  for (const m of blocked) {
    ok(`${m} 拒绝且带中文 reason`, gate.isBlocked(m) && typeof gate.blockReason(m) === 'string' && gate.blockReason(m).length > 4, gate.blockReason(m));
  }
  const allowed = [
    'Browser.getVersion', 'SystemInfo.getInfo', 'Network.setCacheDisabled', 'Network.setExtraHTTPHeaders',
    'Runtime.evaluate', 'Page.addScriptToEvaluateOnNewDocument', 'Emulation.setDeviceMetricsOverride',
    'Page.navigate', 'Input.dispatchMouseEvent',
  ];
  for (const m of allowed) {
    ok(`${m} 不在黑名单`, !gate.isBlocked(m), gate.blockReason(m));
  }
}

// ============================================================ 2. full 档决策矩阵
console.log('\n[2] fullAllowed 决策矩阵');
{
  ok('危险方法 + 已批准 → 仍拒绝', gate.fullAllowed('Browser.close', ORIGIN, { granted: true }).ok === false);
  ok('普通方法 + 已批准 → 放行', gate.fullAllowed('Runtime.evaluate', ORIGIN, { granted: true }).ok === true);
  ok('普通方法 + 未批准 → 拒绝', gate.fullAllowed('Runtime.evaluate', ORIGIN, { granted: false }).ok === false);
  ok('非 http/https origin → 拒绝', gate.fullAllowed('Runtime.evaluate', 'chrome://settings', { granted: true }).ok === false);
  ok('空 origin → 拒绝', gate.fullAllowed('Runtime.evaluate', '', { granted: true }).ok === false);
  ok('http（非 https）→ 放行', gate.fullAllowed('Runtime.evaluate', 'http://localhost:3000', { granted: true }).ok === true);
}

// ============================================================ 3. gateFullCdpCall 与 cdp.call 三层闸
console.log('\n[3] cdp.call 三层闸');
{
  // 默认档：拒绝错误必须逐字等于现状文案
  const marker = { code: 'CDP_METHOD_BLOCKED', message: DEFAULT_DENY_MSG };
  const { e } = await grab(() => gate.gateFullCdpCall(1, 'Runtime.evaluate', () => marker));
  ok('开关关 → 抛出调用方给的默认拒绝（现状零变化）', e === marker, describe(e));

  localStore.set('ubb_cdp_raw', true);
  localStore.delete('ubb_cdp_origins');
  gate.clearDenials();
  const { e: e2 } = await grab(() => gate.gateFullCdpCall(99, 'Debugger.enable', () => marker));
  ok('开关开 + 黑名单方法 → CDP_METHOD_BLOCKED（黑名单先于 origin/banner）', e2?.code === 'CDP_METHOD_BLOCKED' && /调试器/.test(e2?.data?.reason || ''), describe(e2));

  const { e: e3 } = await grab(() => gate.gateFullCdpCall(77, 'Runtime.evaluate', () => marker));
  ok('非 http/https 页面 → CDP_ORIGIN_DENIED 且不弹 banner', e3?.code === 'CDP_ORIGIN_DENIED' && sendMessageCalls === 0, describe(e3));
  localStore.delete('ubb_cdp_raw');
  gate.clearDenials();
}

{
  const raw = () => localStore.set('ubb_cdp_raw', true);
  const rawOff = () => localStore.delete('ubb_cdp_raw');
  const clearOrigins = () => localStore.delete('ubb_cdp_origins');

  // 3a. 默认关 → 逐字一致
  rawOff(); clearOrigins(); gate.clearDenials();
  const a = await grab(() => call('cdp.call', { tab_id: 1, method: 'Browser.getVersion' }));
  ok('cdp.call 默认档拒绝文案逐字一致', a.e?.code === 'CDP_METHOD_BLOCKED' && a.e?.message === DEFAULT_DENY_MSG, describe(a.e));

  // 3b. 开 + 无批准 → banner 两轮：允许 + 不记住 → once 批准用后即删
  raw(); clearOrigins(); gate.clearDenials();
  midFlightOrigins = null;
  const stop1 = resolveConfirms([true, false]);
  const b = await grab(() => call('cdp.call', { tab_id: 1, method: 'Browser.getVersion' }));
  stop1();
  ok('banner 允许后调用成功', b.e === null && b.r?.method === 'Browser.getVersion' && b.r?.result?.product === 'stub-browser', describe(b.e));
  ok('命令出线时 once 批准还挂着（中间态）', midFlightOrigins && midFlightOrigins[ORIGIN]?.lifetime === 'once', JSON.stringify(midFlightOrigins));
  ok('once 用后即删', !(localStore.get('ubb_cdp_origins') || {})[ORIGIN], JSON.stringify(localStore.get('ubb_cdp_origins')));
  ok('full 档调用审计带 gate:"full-cdp"', (await import('../lib/audit.js')).audit.entries.some((x) => x.action === 'cdp.call' && x.gate === 'full-cdp' && x.origin === ORIGIN));

  // 3c. 拒绝 → CDP_ORIGIN_DENIED；10 分钟内不再弹 banner
  clearOrigins(); gate.clearDenials();
  const callsBefore = sendMessageCalls;
  const stop2 = resolveConfirms([false]);
  const c = await grab(() => call('cdp.call', { tab_id: 1, method: 'Runtime.evaluate' }));
  stop2();
  ok('拒绝 → CDP_ORIGIN_DENIED + approve_hint', c.e?.code === 'CDP_ORIGIN_DENIED' && /弹窗-原始CDP/.test(c.e?.data?.approve_hint || ''), describe(c.e));
  ok('拒绝只弹了一轮 banner', sendMessageCalls - callsBefore === 1, String(sendMessageCalls - callsBefore));
  const callsBefore2 = sendMessageCalls;
  const c2 = await grab(() => call('cdp.call', { tab_id: 1, method: 'Runtime.evaluate' }));
  ok('近期拒绝过 → 直接拒绝，不再弹 banner', c2.e?.code === 'CDP_ORIGIN_DENIED' && sendMessageCalls === callsBefore2, describe(c2.e));
  gate.clearDenials();

  // 3d. forever 批准 → 无 banner 放行
  localStore.set('ubb_cdp_origins', { [ORIGIN]: { granted_at: 1, lifetime: 'forever' } });
  const callsBefore3 = sendMessageCalls;
  const d = await grab(() => call('cdp.call', { tab_id: 1, method: 'Runtime.evaluate', params: { expression: '1' } }));
  ok('forever 批准 → 放行且零 banner', d.e === null && d.r?.result?.result?.value === 'main-tab-eval' && sendMessageCalls === callsBefore3, JSON.stringify(d.r || d.e));

  // 3e. 黑名单方法即使已批准也拒绝
  const e5 = await grab(() => call('cdp.call', { tab_id: 1, method: 'Debugger.enable' }));
  ok('已批准 + 黑名单 → CDP_METHOD_BLOCKED 带 reason', e5.e?.code === 'CDP_METHOD_BLOCKED' && typeof e5.e?.data?.reason === 'string', describe(e5.e));

  // 3f. CDP_WRITE 类叠加闸不豁免
  const f1 = await grab(() => call('cdp.call', { tab_id: 1, method: 'Input.dispatchMouseEvent' }));
  ok('写类无 confirm → NEEDS_CONFIRM（raw 开着也一样）', f1.e?.code === 'NEEDS_CONFIRM', describe(f1.e));
  const f2 = await grab(() => call('cdp.call', { tab_id: 1, method: 'Input.dispatchMouseEvent', confirm: true, params: { type: 'mouseMoved', x: 1, y: 1 } }));
  ok('写类 confirm 后照常放行', f2.e === null && f2.r?.gate_bypassed === true, describe(f2.e));

  // 3g. 默认档回复 → 逐字一致（回归）
  rawOff(); gate.clearDenials();
  const g = await grab(() => call('cdp.call', { tab_id: 1, method: 'Runtime.evaluate' }));
  ok('默认档回归：文案仍是现状逐字', g.e?.code === 'CDP_METHOD_BLOCKED' && g.e?.message === DEFAULT_DENY_MSG, describe(g.e));
  clearOrigins();
}

// ============================================================ 4. cdp.events 游标分页
console.log('\n[4] cdp.events 游标分页');
{
  await cdp.ensure(1);
  for (let i = 1; i <= 1250; i++) {
    cdp.handleEvent({ tabId: 1 }, 'Runtime.consoleAPICalled', { type: 'log', args: [{ value: 'e' + i }] });
  }
  const r1 = await call('cdp.events', { tab_id: 1, limit: 900 });
  ok('limit=900 → 900 条（旧参数兼容）', r1.events.length === 900, String(r1.events.length));
  ok('truncated=true（环满 1000，更早事件已丢）', r1.truncated === true);
  ok('has_more=true（还有 100 条没给）', r1.has_more === true);
  ok('cursor = 本页最后一条 seq', r1.cursor === r1.events[r1.events.length - 1].seq, String(r1.cursor));
  ok('next_seq 仍在（向后兼容字段）', typeof r1.next_seq === 'number');

  // 游标续读：r1 停在 1250，之后新到的 3 条由 since_seq=r1.cursor 一页收齐，
  // 与上一页零重叠（tail 语义：has_more 只报环内还有没有未返回的匹配）
  cdp.handleEvent({ tabId: 1 }, 'Runtime.consoleAPICalled', { type: 'log', args: [{ value: 'new1' }] });
  cdp.handleEvent({ tabId: 1 }, 'Runtime.consoleAPICalled', { type: 'log', args: [{ value: 'new2' }] });
  cdp.handleEvent({ tabId: 1 }, 'Runtime.consoleAPICalled', { type: 'log', args: [{ value: 'new3' }] });
  const r2 = await call('cdp.events', { tab_id: 1, since_seq: r1.cursor, limit: 10 });
  ok('cursor 续读 → 恰好拿到 3 条新事件', r2.events.length === 3 && r2.events[0].seq === r1.cursor + 1 && r2.has_more === false, JSON.stringify({ n: r2.events.length, first: r2.events[0]?.seq, cursor: r1.cursor }));
  ok('两页无重叠（本页从上一页 cursor 之后开始）', r2.events[0].seq === r1.events[r1.events.length - 1].seq + 1, JSON.stringify([r1.cursor, r2.events[0]?.seq]));

  // 空页：cursor 保持 since，has_more=false
  const r3 = await call('cdp.events', { tab_id: 1, since_seq: 999999 });
  ok('空页 cursor=since 且 has_more=false', r3.events.length === 0 && r3.cursor === 999999 && r3.has_more === false, JSON.stringify({ cursor: r3.cursor, more: r3.has_more }));

  // method_pattern 过滤 + has_more（tail 语义：has_more 报告环内还有未返回的匹配）
  cdp.handleEvent({ tabId: 1 }, 'Network.requestWillBeSent', { requestId: 'r1', request: { method: 'GET', url: 'https://x.example/' } });
  cdp.handleEvent({ tabId: 1 }, 'Network.requestWillBeSent', { requestId: 'r2', request: { method: 'GET', url: 'https://y.example/' } });
  const r4 = await call('cdp.events', { tab_id: 1, method_pattern: 'Network\\.requestWillBeSent', limit: 1 });
  ok('method_pattern 过滤生效', r4.events.length === 1 && r4.events[0].method === 'Network.requestWillBeSent', JSON.stringify(r4.events.map((e) => e.method)));
  ok('过滤后 has_more=true（环内还有未返回的匹配）', r4.has_more === true);
  const r5 = await call('cdp.events', { tab_id: 1, method_pattern: 'Network\\.requestWillBeSent', limit: 10 });
  ok('放大 limit 收完全部匹配 → has_more=false', r5.events.length === 2 && r5.has_more === false, JSON.stringify(r5.events.map((e) => e.method)));

  // limit 边界：缺省 200；>1000 截到 1000；0 → 1
  const r6 = await call('cdp.events', { tab_id: 1 });
  ok('缺省 limit=200', r6.events.length === 200, String(r6.events.length));
  const r7 = await call('cdp.events', { tab_id: 1, limit: 5000 });
  ok('limit 上限 1000', r7.events.length === 1000, String(r7.events.length));
  const r8 = await call('cdp.events', { tab_id: 1, limit: 0 });
  ok('limit 0 → 1（clamp 下限）', r8.events.length === 1, String(r8.events.length));
}

// ============================================================ 5. OOPIF
console.log('\n[5] OOPIF target 匹配与生命周期');
{
  // 5a. 纯判定
  const s1 = oop.oopifSetFromTree(extFrames, FRAME_TREE);
  ok('tree 判定：frame 2 是 OOPIF', s1.has(2), JSON.stringify([...s1]));
  ok('tree 判定：frame 3（同进程）不是', !s1.has(3));
  ok('tree 判定：4..11 都是 OOPIF', s1.has(4) && s1.has(11));
  ok('树不可用 → 空集（不可判定 ≠ 拒绝）', oop.oopifSetFromTree(extFrames, null).size === 0);
  const mt = oop.matchOopifTargets('https://a/x', [
    { targetId: '1', type: 'iframe', url: 'https://a/x' },
    { targetId: '2', type: 'page', url: 'https://a/x' },
    { targetId: '3', type: 'iframe', url: 'https://b/x' },
  ]);
  ok('target 匹配只认 iframe + URL 等值', mt.length === 1 && mt[0].targetId === '1', JSON.stringify(mt.map((t) => t.targetId)));
  const mt2 = oop.matchOopifTargets('https://a/x', [
    { targetId: '9', type: 'other', url: 'https://a/x' }, // 真机实测 Edge 157：OOPIF 以 type:'other' 出现
  ]);
  ok('Edge 157 形态：type:other + URL 等值也命中', mt2.length === 1 && mt2[0].targetId === '9', JSON.stringify(mt2.map((t) => t.targetId)));

  // 5b. ensure 拒绝路径（frame 2，缓存尚未建立）
  oopTargets = [iframeTargetFor('A', 'https://child.example/inner'), iframeTargetFor('B', 'https://child.example/inner')];
  const amb = await grab(() => oop.oopif.ensure(1, 2));
  ok('同 URL 多命中 → OOP_TARGET_AMBIGUOUS', amb.e?.code === 'OOP_TARGET_AMBIGUOUS' && amb.e?.data?.candidates?.length === 2, describe(amb.e));

  oopTargets = [{ targetId: 'T-CONFLICT', type: 'iframe', url: 'https://child.example/inner' }];
  const con = await grab(() => oop.oopif.ensure(1, 2));
  ok('attach 冲突 → DEBUGGER_CONFLICT', con.e?.code === 'DEBUGGER_CONFLICT', describe(con.e));

  oopTargets = [];
  const nf = await grab(() => oop.oopif.ensure(1, 2));
  ok('无命中 → OOP_TARGET_NOT_FOUND', nf.e?.code === 'OOP_TARGET_NOT_FOUND', describe(nf.e));

  const unknown = await grab(() => oop.oopif.ensure(1, 999));
  ok('未知 frame → FRAME_UNSUPPORTED', unknown.e?.code === 'FRAME_UNSUPPORTED', describe(unknown.e));
  const main = await grab(() => oop.oopif.ensure(1, 0));
  ok('主 frame → FRAME_UNSUPPORTED', main.e?.code === 'FRAME_UNSUPPORTED', describe(main.e));
  const sameProc = await grab(() => oop.oopif.ensure(1, 3));
  ok('同进程 frame → FRAME_UNSUPPORTED（不走 OOPIF 通道）', sameProc.e?.code === 'FRAME_UNSUPPORTED', describe(sameProc.e));

  // 5c. 唯一命中 → attach → send 路由到 targetId
  oopTargets = [iframeTargetFor(2, 'https://child.example/inner'), ...[4, 5, 6, 7, 8, 9, 10, 11].map((n) => iframeTargetFor(n, `https://f${n}.example/x`))];
  const entry = await oop.oopif.ensure(1, 2);
  ok('唯一命中 → attach 成功并缓存', entry?.targetId === 'T-2' && attachLog.includes('T-2'), JSON.stringify(entry));
  const v = await oop.oopif.send(1, 2, 'Runtime.evaluate', { expression: '1+1' });
  ok('send 经 target 会话返回', v?.result?.value === 42, JSON.stringify(v));
  ok('命令带 targetId 出线', targetCmds[targetCmds.length - 1]?.targetId === 'T-2');

  // 5d. evaluate {frame}
  const ev1 = await grab(() => oop.evaluateInFrame(1, { frame: 2, expression: '1+1' }));
  ok('evaluate OOPIF MAIN world → {value,frame_id,oopif}', ev1.r?.value === 42 && ev1.r?.frame_id === 2 && ev1.r?.oopif === true, JSON.stringify(ev1.r || ev1.e));
  const ev2 = await grab(() => oop.evaluateInFrame(1, { frame: 2, expression: 'x', world: 'isolated' }));
  const lastTwo = targetCmds.slice(-2);
  ok('evaluate OOPIF isolated → createIsolatedWorld + contextId', ev2.r?.value === 42 && lastTwo[0]?.method === 'Page.createIsolatedWorld' && lastTwo[1]?.params?.contextId === 99, JSON.stringify(lastTwo));
  const ev3 = await grab(() => oop.evaluateInFrame(1, { frame: 3, expression: 'x' }));
  ok('同进程子 frame MAIN world → 如实拒绝', ev3.e?.code === 'FRAME_UNSUPPORTED' && /诚实通道/.test(ev3.e?.message || ''), describe(ev3.e));
  const ev4 = await grab(() => oop.evaluateInFrame(1, { frame: 3, expression: 'x', world: 'isolated' }));
  ok('同进程子 frame isolated → 页面会话 createIsolatedWorld({frameId})', ev4.r?.value === 'main-tab-eval' && tabCmds[tabCmds.length - 2]?.params?.frameId === 3 && tabCmds[tabCmds.length - 1]?.params?.contextId === 55, JSON.stringify(tabCmds.slice(-2)));
  const ev5 = await grab(() => oop.evaluateInFrame(1, { frame: 0, expression: 'x' }));
  ok('frame 0 → null（原路径）', ev5.r === null && ev5.e === null, describe(ev5.e));
  const ev6 = await grab(() => oop.evaluateInFrame(1, { frame: 'child.example/inner', expression: 'x' }));
  ok('url 匹配串 → 解析到 frame 2', ev6.r?.frame_id === 2, JSON.stringify(ev6.r || ev6.e));

  // 5e. upload_file {frame}
  fileInputNodeIds = [101];
  const up1 = await grab(() => oop.uploadFileInFrame(1, { frame: 2, files: ['a.pdf', 'b.pdf'] }));
  ok('upload OOPIF → setFileInputFiles 命中唯一 input', up1.r?.uploaded === 2 && up1.r?.frame_id === 2, JSON.stringify(up1.r || up1.e));
  ok('setFileInputFiles 带 nodeId+files 出线', setFileCalls[setFileCalls.length - 1]?.nodeId === 101 && setFileCalls[setFileCalls.length - 1]?.files?.length === 2, JSON.stringify(setFileCalls[setFileCalls.length - 1]));
  fileInputNodeIds = [101, 102];
  const up2 = await grab(() => oop.uploadFileInFrame(1, { frame: 2, files: ['a.pdf'] }));
  ok('多个 file input 且无 ref → 拒绝', up2.e?.code === 'BAD_PARAMS' && /唯一定位/.test(up2.e?.message || ''), describe(up2.e));
  fileInputNodeIds = [];
  const up3 = await grab(() => oop.uploadFileInFrame(1, { frame: 2, files: ['a.pdf'] }));
  ok('没有 file input → 拒绝', up3.e?.code === 'BAD_PARAMS' && /没有找到/.test(up3.e?.message || ''), describe(up3.e));
  fileInputNodeIds = [101];
  const up4 = await grab(() => oop.uploadFileInFrame(1, { frame: 0, files: ['a.pdf'] }));
  ok('frame 0 → null（原路径）', up4.r === null, describe(up4.e));
  const up5 = await grab(() => oop.uploadFileInFrame(1, { frame: 3, files: ['a.pdf'] }));
  ok('同进程 frame → null（原路径维持现状）', up5.r === null, describe(up5.e));
  const ref3 = refs.put(1, null, { role: 'textbox', name: 'x', frameId: 3, domRef: '@d3.1', epoch: 1 });
  const up6 = await grab(() => oop.uploadFileInFrame(1, { frame: 2, ref: ref3, files: ['a.pdf'] }));
  ok('ref 与 frame 参数不一致 → BAD_PARAMS', up6.e?.code === 'BAD_PARAMS' && /不一致/.test(up6.e?.message || ''), describe(up6.e));
  const up7 = await grab(() => oop.uploadFileInFrame(1, { ref: ref3, files: ['a.pdf'] }));
  ok('ref 自带同进程 frameId → null（原路径）', up7.r === null, describe(up7.e));

  // 5f. LRU ≤ 8
  for (const fid of [4, 5, 6, 7, 8, 9, 10, 11]) await oop.oopif.ensure(1, fid);
  ok('LRU：9 条进来只留 8 条', oop.oopif.entries.size === 8, String(oop.oopif.entries.size));
  await new Promise((r) => setTimeout(r, 25)); // _drop 的隧道拆解走异步 cdp.send（fire-and-forget）
  ok('LRU：最久未用的 frame 2 被淘汰并 detach', detachLog.includes('T-2'), JSON.stringify(detachLog));

  // 5g. 30s 空闲清扫
  const now = Date.now();
  for (const [, en] of oop.oopif.entries) en.lastUsed = now - 31_000;
  const dropped = oop.oopif._sweep(now);
  ok('sweep 清掉全部过期条目并 detach', dropped === 8 && oop.oopif.entries.size === 0, `${dropped}/${oop.oopif.entries.size}`);
  await oop.oopif.ensure(1, 2);
  ok('新条目不会被 sweep 误清', oop.oopif._sweep(Date.now()) === 0 && oop.oopif.entries.size === 1);

  // 5h. tab 关闭 → cdp.forget 钩子清理（模块自注册，无需 background 接线）
  cdp.forget(1);
  await new Promise((r) => setTimeout(r, 25)); // 同上：detachFromTarget 异步落账
  ok('cdp.forget → oopif.detachTab 清掉该 tab 的条目', oop.oopif.entries.size === 0 && detachLog.filter((t) => t === 'T-2').length >= 2, JSON.stringify(detachLog));
}

// ============================================================ 6. secreq
console.log('\n[6] secreq（secret.request 待处理仓库）');
{
  const A = secReq.create({
    tab_id: 5, origin: 'https://shop.example', title: '登录', timeout_ms: 5000,
    fields: [{ id: 'u', label: '用户名', type: 'username' }, { id: 'p', label: '密码', type: 'password' }],
  });
  ok('create 返回 {id, promise}', typeof A.id === 'string' && typeof A.then !== 'function' && typeof A.promise?.then === 'function');
  const meta = secReq.listMeta();
  ok('listMeta 元数据齐全', meta.length === 1 && meta[0].id === A.id && meta[0].origin === 'https://shop.example' && meta[0].fields.length === 2, JSON.stringify(meta));
  ok('元数据无值（fields 只有 id/label/type）', !('values' in meta[0]) && !('value' in meta[0].fields[0]), JSON.stringify(meta[0].fields[0]));
  const stored = (sessionStore.get('ubb_sec_requests') || [])[0];
  ok('storage.session 只同步元数据（ubb_sec_requests）', stored && stored.id === A.id && !JSON.stringify(stored).includes('values'));
  ok('submit 未知 id → false', secReq.submit('sr_nope', { u: 'x' }) === false);

  let filled = null;
  A.promise.then((r) => { filled = r; });
  ok('submit 多余字段丢弃', secReq.submit(A.id, { u: 'bob', p: 'pw', extra: 'nope' }) === true);
  await new Promise((r) => setTimeout(r, 10));
  ok('promise resolve {state:"filled", values}（只含声明字段）', filled && filled.state === 'filled' && filled.values.u === 'bob' && filled.values.p === 'pw' && !('extra' in filled.values), JSON.stringify(filled));
  ok('提交后 pending 清空且 storage 同步', secReq.listMeta().length === 0 && (sessionStore.get('ubb_sec_requests') || []).length === 0);

  const B = secReq.create({ tab_id: 5, fields: [{ id: 'u', label: 'u', type: 'text' }], timeout_ms: 5000 });
  let cancelled = null;
  B.promise.then((r) => { cancelled = r; });
  const busy = await grab(() => secReq.create({ tab_id: 5, fields: [{ id: 'u', label: 'u', type: 'text' }] }));
  ok('同 tab 二次 create → SECRET_REQ_BUSY', busy.e?.code === 'SECRET_REQ_BUSY', describe(busy.e));
  ok('cancel → true 且 promise {state:"cancelled"}', secReq.cancel(B.id) === true);
  await new Promise((r) => setTimeout(r, 10));
  ok('cancel 语义', cancelled && cancelled.state === 'cancelled', JSON.stringify(cancelled));
  ok('重复 cancel → false', secReq.cancel(B.id) === false);

  const T = secReq.create({ tab_id: 6, fields: [{ id: 'u', label: 'u', type: 'text' }], timeout_ms: 30 });
  const to = await grab(() => T.promise);
  ok('超时 → reject SECRET_REQ_TIMEOUT', to.e?.code === 'SECRET_REQ_TIMEOUT', describe(to.e));
  ok('超时后 pending 清空', secReq.listMeta().length === 0 && (sessionStore.get('ubb_sec_requests') || []).length === 0);

  const v1 = await grab(() => secReq.create({ tab_id: 7, fields: Array.from({ length: 9 }, (_, i) => ({ id: 'f' + i, label: 'x', type: 'text' })) }));
  ok('fields > 8 → BAD_PARAMS', v1.e?.code === 'BAD_PARAMS', describe(v1.e));
  const v2 = await grab(() => secReq.create({ tab_id: 7, fields: [{ id: 'c', label: '卡号', type: 'card' }] }));
  ok('type 枚举封闭（card 拒绝）→ BAD_PARAMS', v2.e?.code === 'BAD_PARAMS', describe(v2.e));
  const v3 = await grab(() => secReq.create({ tab_id: 7, fields: [{ id: 'x', label: 'l'.repeat(41), type: 'text' }] }));
  ok('label > 40 字 → BAD_PARAMS', v3.e?.code === 'BAD_PARAMS', describe(v3.e));
  const v4 = await grab(() => secReq.create({ tab_id: 7, fields: [{ id: 'd', label: 'a', type: 'text' }, { id: 'd', label: 'b', type: 'text' }] }));
  ok('field.id 重复 → BAD_PARAMS', v4.e?.code === 'BAD_PARAMS', describe(v4.e));
}

// ============================================================ 7. 附加回归：会话档清理 helper
console.log('\n[7] origin 批准表生命周期 helper');
{
  localStore.set('ubb_cdp_origins', {
    'https://a.example': { granted_at: 1, lifetime: 'session' },
    'https://b.example': { granted_at: 2, lifetime: 'forever' },
    'https://c.example': { granted_at: 3, lifetime: 'once' },
  });
  const n = await gate.clearSessionGrants();
  const left = localStore.get('ubb_cdp_origins') || {};
  ok('clearSessionGrants 只清 session 档', n === 1 && left['https://a.example'] === undefined && left['https://b.example']?.lifetime === 'forever' && left['https://c.example']?.lifetime === 'once', JSON.stringify(left));
  ok('revokeOrigin 删键', (await gate.revokeOrigin('https://c.example')) === true && !left['https://c.example']);
  ok('consumeOnce 只动 once 档', (await gate.consumeOnce('https://b.example')) === false);
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
