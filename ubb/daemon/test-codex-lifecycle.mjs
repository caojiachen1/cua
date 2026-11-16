// Offline self-check for the CORE-2 lifecycle wave (ABSORB-CODEX-PLAN §2.1/
// §2.2/§2.3/§6/§7/§10/§11) — no browser, no daemon:
//   node daemon/test-codex-lifecycle.mjs
// Covers: marks 语义矩阵、session.stop 关闭矩阵、snapshot diff 三态与 gen 回退、
// tab.claim 三元组校验、secret.request 元数据无值、policy strict 风险门控、
// bot.report 白名单与 hostname 截断。

import { installChromeStub } from '../lib/testharness.mjs';

let pass = 0;
let fail = 0;
const ok = (name, cond, detail = '') => {
  if (cond) { pass++; console.log(`  ✓ ${name}`); }
  else { fail++; console.log(`  ✗ ${name}${detail ? ` — ${detail}` : ''}`); }
};

async function grab(f) {
  try { return { r: await f(), e: null }; } catch (e) { return { r: null, e }; }
}
const describe = (x) => (x == null ? 'it returned instead of throwing' : `got ${x.code || 'plain throw'}: ${String(x.message || x).slice(0, 140)}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── stateful chrome stub（在 lib 模块加载之前装好）────────────────────────────
installChromeStub();

function makeStore() {
  const m = new Map();
  return {
    _m: m,
    async get(keys) {
      const out = {};
      if (keys == null) { for (const [k, v] of m) out[k] = v; return out; }
      const ks = Array.isArray(keys) ? keys : typeof keys === 'string' ? [keys] : Object.keys(keys);
      for (const k of ks) if (m.has(k)) out[k] = m.get(k);
      return out;
    },
    async set(obj) { for (const [k, v] of Object.entries(obj || {})) m.set(k, v); },
    async remove(keys) { for (const k of Array.isArray(keys) ? keys : [keys]) m.delete(k); },
  };
}

function makeTabs() {
  const tabs = new Map();
  let nextId = 100;
  const removed = [];
  const sent = [];
  const ungrouped = [];
  return {
    _tabs: tabs, _removed: removed, _sent: sent, _ungrouped: ungrouped,
    async query(q = {}) {
      return [...tabs.values()].filter((t) => (q.groupId == null || t.groupId === q.groupId));
    },
    async get(id) {
      const t = tabs.get(id);
      if (!t) throw new Error('No tab with id: ' + id);
      return { ...t };
    },
    async create(p = {}) {
      const id = ++nextId;
      const t = { id, windowId: 1, url: p.url || 'about:blank', title: p.title || `tab ${id}`, active: !!p.active, groupId: null };
      tabs.set(id, t);
      return { ...t };
    },
    async update(id, p = {}) {
      const t = tabs.get(id);
      if (!t) throw new Error('No tab ' + id);
      Object.assign(t, p);
      return { ...t };
    },
    async remove(id) {
      if (!tabs.delete(id)) throw new Error('No tab ' + id);
      removed.push(id);
    },
    async group(p = {}) {
      const gid = p.groupId ?? 7;
      for (const id of [].concat(p.tabIds || [])) {
        const t = tabs.get(id);
        if (t) t.groupId = gid;
      }
      return gid;
    },
    async ungroup(tabIds) {
      for (const id of [].concat(tabIds || [])) {
        const t = tabs.get(id);
        if (t) { t.groupId = null; ungrouped.push(id); }
      }
    },
    async sendMessage(tabId, msg, opts) { sent.push({ tabId, msg, opts }); return {}; },
  };
}

const localStore = makeStore();
const sessionStore = makeStore();
const chromeTabs = makeTabs();
const groupTitles = [];

globalThis.chrome.storage = { local: localStore, session: sessionStore };
globalThis.chrome.tabs = chromeTabs;
globalThis.chrome.tabGroups = {
  query: async () => [{ id: 7, title: groupTitles[groupTitles.length - 1]?.[1] || '⏳ UBB' }],
  get: async (id) => ({ id }),
  update: async (id, p) => { groupTitles.push([id, p.title]); return {}; },
};
globalThis.chrome.webNavigation = { ...globalThis.chrome.webNavigation, getAllFrames: async () => [{ frameId: 0, parentFrameId: -1, url: 'https://stub.example/' }] };
globalThis.chrome.history = { search: async () => [{ title: 'h1', url: 'https://h.example/x', lastVisitTime: Date.now() }] };
globalThis.chrome.scripting = undefined; // fix19 可见性读在离线桩里就是 unknown

const lastTitle = (gid = 7) => [...groupTitles].reverse().find(([id]) => id === gid)?.[1] || null;

// ── load modules AFTER the stub ───────────────────────────────────────────────
const { sessions } = await import('../lib/tabs.js');
const { tabRegistry } = await import('../lib/tab-registry.js');
const { refs } = await import('../lib/refs.js');
const { takeSnapshot } = await import('../lib/snapshot.js');
const { cdp } = await import('../lib/cdp.js');
const { secReq } = await import('../lib/secreq.js');
const { shield } = await import('../lib/shield-ctl.js');
const { audit } = await import('../lib/audit.js');
const { err } = await import('../lib/config.js');
const { setStrict, resetStrictCache, isStrict } = await import('../lib/policy.js');
const { emitCursor } = await import('../lib/actions.js');
const { Dispatcher } = await import('../lib/protocol.js');
const { registerTools } = await import('../lib/tools.js');
const { registerExtras } = await import('../lib/tools-extra.js');
const { registerMoreTools } = await import('../lib/tools-more.js');

const tools = new Map();
const dispatcher = { register: (name, meta, fn) => tools.set(name, { meta, fn }) };
registerTools(dispatcher);
registerExtras(dispatcher);
registerMoreTools(dispatcher);
const call = (name, params = {}) => tools.get(name).fn(params);

// 一个真 Dispatcher（走 protocol.js 的全部闸，含 policy strict 风险门控）
const d2 = new Dispatcher();
registerTools(d2);
registerExtras(d2);
registerMoreTools(d2);
const handle = (method, params = {}) => d2.handle({ id: 't-' + Math.random().toString(36).slice(2), method: 'tool.' + method, params });
const handled = async (method, params = {}) => {
  const r = await handle(method, params);
  if (r?.error) throw r.error;
  return r?.result;
};

const auditEntries = (action) => audit.entries.filter((e) => e.action === action);

// ============================================================ 1. registry shape
console.log('\n[1] registry shape');
for (const t of ['tab.mark', 'bot.report', 'policy.set', 'policy.get', 'browser.diagnostics', 'secret.request']) {
  ok(`${t} is registered`, tools.has(t));
  ok(`${t} carries a description`, !!tools.get(t)?.meta?.description);
}
ok('tab.mark is NOT sensitive', tools.get('tab.mark')?.meta?.sensitive === false, JSON.stringify(tools.get('tab.mark')?.meta));
ok('tab.mark override replaced the old {status} annotator', !!tools.get('tab.mark')?.meta?.description?.includes('deliverable'), tools.get('tab.mark')?.meta?.description?.slice(0, 60));
for (const [t, risk] of [
  ['challenge.solve', 'captcha'],
  ['download.clean', 'delete'],
  ['secret.set', 'transmit'],
  ['cdp.call', 'raw_cdp'],
  ['clipboard.read', 'transmit'],
  ['tab.close_ubb', 'delete'],
  ['history.list', 'transmit'],
  ['evaluate', 'raw_cdp'],
]) {
  ok(`${t} meta.risk = ${risk}`, tools.get(t)?.meta?.risk === risk, String(tools.get(t)?.meta?.risk));
}

// ============================================================ 2. marks 语义矩阵
console.log('\n[2] marks 语义矩阵');
{
  const s = await sessions.start();
  ok('session started', !!s?.session_id);
  const a = await sessions.createTab('about:blank');
  const b = await sessions.createTab('about:blank');
  await chrome.tabs.update(a.tab_id, { url: 'https://app.example/report', title: '季度报表' });
  await chrome.tabs.update(b.tab_id, { url: 'https://app.example/handoff', title: '交接面板' });

  const set1 = await call('tab.mark', { tab_id: a.tab_id, mark: 'deliverable' });
  ok('set deliverable → mark 回读', set1.mark === 'deliverable' && set1.tab_id === a.tab_id, JSON.stringify(set1));
  ok('UBB_BADGE 消息发出（deliverable）', chromeTabs._sent.some((m) => m.tabId === a.tab_id && m.msg?.type === 'UBB_BADGE' && m.msg.mark === 'deliverable'), JSON.stringify(chromeTabs._sent));
  const set2 = await call('tab.mark', { tab_id: b.tab_id, mark: 'handoff' });
  ok('set handoff → mark 回读', set2.mark === 'handoff');
  const cur = await call('tab.mark', { tab_id: a.tab_id }); // 省略 mark = 返回当前
  ok('省略 mark 返回当前标记', cur.mark === 'deliverable', JSON.stringify(cur));
  const cleared = await call('tab.mark', { tab_id: b.tab_id, mark: 'none' });
  ok('mark none 清除（返回 null）', cleared.mark === null, JSON.stringify(cleared));

  const { e: badMark } = await grab(() => call('tab.mark', { tab_id: a.tab_id, mark: 'keep-forever' }));
  ok('非法 mark → BAD_MARK', badMark?.code === 'BAD_MARK', describe(badMark));
  const outsider = await chromeTabs.create({ url: 'https://user.example/private' });
  const { e: notOwned } = await grab(() => call('tab.mark', { tab_id: outsider.id, mark: 'deliverable' }));
  ok('非会话 tab → TAB_NOT_OWNED', notOwned?.code === 'TAB_NOT_OWNED', describe(notOwned));

  const lst = await call('tab.list', { scope: 'session' });
  const rowA = lst.tabs.find((t) => t.tab_id === a.tab_id);
  const rowB = lst.tabs.find((t) => t.tab_id === b.tab_id);
  ok('tab.list 附 mark 字段（deliverable / 未标记为空）', rowA?.mark === 'deliverable' && rowB?.mark === undefined, JSON.stringify({ a: rowA?.mark, b: rowB?.mark }));

  // ── stop 关闭矩阵（默认：unmarked 关、marked 留、标题 📌）──
  await call('tab.mark', { tab_id: b.tab_id, mark: 'handoff' });
  const c0 = await sessions.createTab('about:blank'); // 第三个 tab 保持未标记
  const stopped = await call('session.stop', {});
  ok('stop 默认关闭 unmarked created tab', chromeTabs._removed.includes(c0.tab_id), JSON.stringify(chromeTabs._removed));
  ok('stop 默认保留 marked tab', !chromeTabs._removed.includes(a.tab_id) && !chromeTabs._removed.includes(b.tab_id) && (stopped.kept_tabs || []).includes(a.tab_id) && (stopped.kept_tabs || []).includes(b.tab_id), JSON.stringify(stopped));
  ok('保留 tab 的 registry 记 orphaned', (await tabRegistry.list()).find((e) => e.tab_id === b.tab_id)?.kind === 'orphaned');
  ok('组标题变为 📌 UBB（有保留 tab）', lastTitle() === '📌 UBB', String(lastTitle()));
  ok('stop 返回 session_id 且会话结束', stopped.stopped === true && sessions.active() === null);

  // ── close_marked:true 全关（现状）──
  await sessions.start();
  const c = await sessions.createTab('about:blank');
  const d = await sessions.createTab('about:blank');
  await call('tab.mark', { tab_id: c.tab_id, mark: 'deliverable' });
  const removedBefore = chromeTabs._removed.length;
  const stopped2 = await call('session.stop', { close_marked: true });
  ok('close_marked:true 连 marked 一起关', chromeTabs._removed.includes(c.tab_id) && chromeTabs._removed.includes(d.tab_id) && chromeTabs._removed.length === removedBefore + 2, JSON.stringify(stopped2));
  ok('全关后组标题回 ✅ UBB', lastTitle() === '✅ UBB', String(lastTitle()));
  ok('close_marked 时 kept_tabs 为空', (stopped2.kept_tabs || []).length === 0, JSON.stringify(stopped2.kept_tabs));

  // ── close_created:false（旧行为）：未标记拆组、marked 留组 ──
  await sessions.start();
  const e1 = await sessions.createTab('about:blank');
  const e2 = await sessions.createTab('about:blank');
  await call('tab.mark', { tab_id: e2.tab_id, mark: 'deliverable' });
  const ungroupedBefore = chromeTabs._ungrouped.length;
  await call('session.stop', { close_created: false, close_spawned: false });
  ok('close_created:false 时全部保留（不关）', !chromeTabs._removed.includes(e1.tab_id) && !chromeTabs._removed.includes(e2.tab_id));
  ok('未标记的拆组（旧行为），marked 留在 📌 组内', chromeTabs._ungrouped.includes(e1.tab_id) && !chromeTabs._ungrouped.includes(e2.tab_id) && chromeTabs._ungrouped.length === ungroupedBefore + 1, JSON.stringify(chromeTabs._ungrouped));
  ok('标题 📌 UBB（marked 仍在组内）', lastTitle() === '📌 UBB', String(lastTitle()));
  // 清场：第二次 stop 全关
  await sessions.start().then(async () => { await sessions.stop({ close_marked: true }).catch(() => {}); });
  ok('清场后会话为空', sessions.active() === null);
}

// ============================================================ 3. diff 三态
console.log('\n[3] snapshot diff 三态');
{
  // 可调 AX 树：fabricate 确定性的两代快照，让 diff 有真实的增删行。
  // 拦 _send（ensure 的内部路径与公共 send 都汇聚在这里）
  const realSend = cdp._send.bind(cdp);
  let axNames = null; // null = 桩默认（空树）
  cdp._send = async (tabId, method, params) => {
    if (method === 'Accessibility.getFullAXTree' && axNames) {
      const nodes = [
        { nodeId: '1', role: { value: 'rootWebArea' }, ignored: false, childIds: axNames.map((_, i) => String(i + 2)) },
        ...axNames.map((n, i) => ({ nodeId: String(i + 2), role: { value: 'button' }, name: { value: n }, backendDOMNodeId: 5000 + i, ignored: false })),
      ];
      return { nodes };
    }
    // cdp.ensure 的 frame-tree URL 闸：库存桩返回 {} 会被判 URL 隐藏 → 给出
    // 该 tab 的真实 URL
    if (method === 'Page.getFrameTree') {
      const t = chromeTabs._tabs.get(tabId);
      return { frameTree: { frame: { id: 0, url: t?.url || 'https://stub.example/' } } };
    }
    return realSend(tabId, method, params);
  };

  const tab = await chromeTabs.create({ url: 'https://diff.example/p1' });
  const tid = tab.id;
  refs.invalidate(tid);

  axNames = ['登录', '提交订单', '搜索'];
  const first = await takeSnapshot(tid, {}); // 默认（缺省 = auto）
  ok('首个 snapshot 无基线 → 全量回退（diff:null + diff_note）', first.diff === null && !!first.diff_note, JSON.stringify(first.diff));
  ok('全量视图带 @eN 行', /@e\d+/.test(first.text), first.text.slice(0, 60));

  const same = await takeSnapshot(tid, {}); // gen 未变 → auto diff 视图
  ok('同 gen 第二拍 → auto diff 视图（0 增 0 删）', !!same.diff && same.diff.added === 0 && same.diff.removed === 0 && same.diff.changed_count === 0, JSON.stringify(same.diff));
  ok('diff 载荷带 from_nonce（基线 nonce）', typeof same.diff?.from_nonce === 'string' && same.diff.from_nonce.length > 4, JSON.stringify(same.diff?.from_nonce));

  axNames = ['登录', '立即付款', '搜索'];
  const changed = await takeSnapshot(tid, {}); // auto：只回变更行
  ok('auto 捕捉到变更行（1 增 1 删，changed_count=2）', changed.diff?.added === 1 && changed.diff?.removed === 1 && changed.diff?.changed_count === 2, JSON.stringify(changed.diff));
  ok('diff 文本里有 + / - 行', changed.text.includes('+') && changed.text.includes('-'), changed.text.slice(0, 120));
  ok('refs 仍全量注册（ref_count 是整树）', changed.ref_count === 3, String(changed.ref_count));

  const full = await takeSnapshot(tid, { diff: 'full' });
  ok('diff:"full" 强制全量（无 diff 载荷）', full.diff === undefined && full.text.includes('立即付款'), JSON.stringify(full.diff));
  const off = await takeSnapshot(tid, { diff: 'off' });
  ok('diff:"off" 全量', off.diff === undefined && off.text.includes('立即付款'));
  const boolT = await takeSnapshot(tid, { diff: true });
  ok('diff:true 兼容 = auto（diff 视图）', !!boolT.diff && boolT.diff.changed_count === 0, JSON.stringify(boolT.diff));
  const boolF = await takeSnapshot(tid, { diff: false });
  ok('diff:false 兼容 = off（全量）', boolF.diff === undefined);
  const { e: badDiff } = await grab(() => takeSnapshot(tid, { diff: 'yes' }));
  ok('非法 diff 值 → BAD_PARAMS', badDiff?.code === 'BAD_PARAMS', describe(badDiff));

  // gen 变化 → 回退全量
  refs.bump(tid);
  const afterNav = await takeSnapshot(tid, {});
  ok('gen 变化后 auto 回退全量（diff:null）', afterNav.diff === null && !!afterNav.diff_note, JSON.stringify(afterNav.diff));

  // diff 视图的字节预算 + next_cursor 续读（读的是同一次 diff，不是全量树）
  refs.invalidate(tid);
  axNames = Array.from({ length: 900 }, (_, i) => `button-${i}`);
  await takeSnapshot(tid, { diff: 'off' }); // 基线：900 行
  axNames = axNames.map((n) => `${n}-renamed-with-a-long-accessible-label-for-the-byte-budget-test`);
  const big = await takeSnapshot(tid, {}); // 900 行全变 → diff 文本超 16KiB
  ok('超大 diff 视图受字节预算截断（truncated + next_cursor）', big.truncated === true && !!big.next_cursor, JSON.stringify({ truncated: big.truncated, next_cursor: big.next_cursor }));
  const page2 = await takeSnapshot(tid, { cursor: big.next_cursor });
  ok('diff 游标续读到第二页（非全量树）', page2.text.length > 0 && page2.page.offset > 0 && /renamed/.test(page2.text), JSON.stringify(page2.page));

  cdp._send = realSend; // 还原
  await chromeTabs.remove(tid).catch(() => {});
}

// ============================================================ 4. tab.claim 三元组
console.log('\n[4] tab.claim 三元组校验');
{
  const s = await sessions.start();
  const realConfirm = shield.confirm.bind(shield);
  shield.confirm = async () => true; // 用户一律允许 —— 校验逻辑与确认解耦
  try {
    const t1 = await chromeTabs.create({ url: 'https://shop.example/listing?page=2', title: '商品列表' });
    const claimed = await call('tab.claim', { tab_id: t1.id, expect_url: 'https://shop.example/listing?tracking=zzz', expect_title: '商品列表' });
    ok('expect_url 只比 origin+pathname（query 变化容忍）→ 接管成功', claimed.in_group === true && sessions.owns(t1.id), JSON.stringify(claimed));
    await sessions.release(t1.id).catch(() => {});

    const t2 = await chromeTabs.create({ url: 'https://shop.example/listing', title: '页面已改版' });
    const { e: driftT } = await grab(() => call('tab.claim', { tab_id: t2.id, expect_title: '商品列表' }));
    ok('expect_title 不符 → CLAIM_DRIFT 且不接管', driftT?.code === 'CLAIM_DRIFT' && !sessions.owns(t2.id), describe(driftT));
    ok('CLAIM_DRIFT data.actual 带真实 title/url', driftT?.data?.actual?.title === '页面已改版' && /shop\.example/.test(driftT?.data?.actual?.url || ''), JSON.stringify(driftT?.data));

    const t3 = await chromeTabs.create({ url: 'https://shop.example/other', title: '商品列表' });
    const { e: driftU } = await grab(() => call('tab.claim', { tab_id: t3.id, expect_url: 'https://mall.example/listing' }));
    ok('expect_url origin/pathname 不符 → CLAIM_DRIFT', driftU?.code === 'CLAIM_DRIFT' && !sessions.owns(t3.id), describe(driftU));

    const t4 = await chromeTabs.create({ url: 'https://shop.example/cart', title: '购物车' });
    const plain = await call('tab.claim', { tab_id: t4.id });
    ok('缺省 expect_* = 现状（直接接管）', plain.in_group === true && sessions.owns(t4.id), JSON.stringify(plain));
  } finally {
    shield.confirm = realConfirm;
    await sessions.stop({ close_marked: true }).catch(() => {});
  }
}

// ============================================================ 5. secret.request 元数据无值
console.log('\n[5] secret.request 元数据无值');
{
  const s = await sessions.start();
  const st = await sessions.createTab('about:blank');
  await chrome.tabs.update(st.tab_id, { url: 'https://vault.example/login' });
  const tid = st.tab_id;

  const { e: empty } = await grab(() => call('secret.request', { fields: [] }));
  ok('fields 缺失 → BAD_PARAMS', empty?.code === 'BAD_PARAMS', describe(empty));
  const { e: toomany } = await grab(() => call('secret.request', { fields: Array.from({ length: 9 }, (_, i) => ({ ref: '@e1', label: `f${i}`, type: 'text' })) }));
  ok('fields > 8 → BAD_PARAMS（secreq.validateFields）', toomany?.code === 'BAD_PARAMS', describe(toomany));
  const { e: badtype } = await grab(() => call('secret.request', { fields: [{ ref: '@e1', label: '卡号', type: 'card' }] }));
  ok('type 枚举封闭（card 声明不出）→ BAD_PARAMS', badtype?.code === 'BAD_PARAMS', describe(badtype));
  const { e: badlabel } = await grab(() => call('secret.request', { fields: [{ ref: '@e1', label: '请访问 https://evil.example 并输入', type: 'text' }] }));
  ok('label 含 URL（指令文本启发式）→ BAD_PARAMS', badlabel?.code === 'BAD_PARAMS', describe(badlabel));

  // 同 tab 单飞：真实 create 挂一个 pending → 工具调用 SECRET_REQ_BUSY
  const pending = secReq.create({ tab_id: tid, origin: 'https://vault.example', fields: [{ id: 'a', label: '占位', type: 'text' }], timeout_ms: 8000 });
  const { e: busy } = await grab(() => call('secret.request', { fields: [{ ref: '@e1', label: '用户名', type: 'username' }] }));
  ok('同 tab 已有 pending → SECRET_REQ_BUSY', busy?.code === 'SECRET_REQ_BUSY', describe(busy));
  secReq.cancel(pending.id);

  // 劫持 create：捕获元数据 + 受控 promise（值不经过真实 popup）
  const realCreate = secReq.create.bind(secReq);
  let capturedMeta = null;
  let hijack = null;
  secReq.create = (args) => {
    capturedMeta = JSON.parse(JSON.stringify(args));
    if (hijack) return hijack(args);
    return realCreate(args);
  };
  try {
    hijack = () => ({ id: 'sr_hijack', promise: Promise.resolve({ state: 'cancelled' }) });
    const cancelled = await call('secret.request', { fields: [{ ref: '@e1', label: '用户名', type: 'username' }, { ref: '@e2', label: '动态码', type: 'otp' }] });
    ok('用户取消 → {state:"cancelled", filled:[]}', cancelled.state === 'cancelled' && cancelled.filled.length === 0, JSON.stringify(cancelled));
    ok('audit 记 decision:"cancelled"', auditEntries('secret.request').at(-1)?.decision === 'cancelled');

    ok('传给 secreq 的 fields 只有 {id,label,type}（无值无 ref）', JSON.stringify(Object.keys(capturedMeta?.fields?.[0] || {})) === JSON.stringify(['id', 'label', 'type']), JSON.stringify(capturedMeta?.fields));
    ok('id 缺省按序号补齐 f1/f2', capturedMeta?.fields?.[0]?.id === 'f1' && capturedMeta?.fields?.[1]?.id === 'f2', JSON.stringify(capturedMeta?.fields));
    ok('origin 取 tab.url origin', capturedMeta?.origin === 'https://vault.example', String(capturedMeta?.origin));

    hijack = () => ({ id: 'sr_hijack', promise: Promise.resolve({ state: 'filled', values: {} }) });
    const filled0 = await call('secret.request', { fields: [{ ref: '@e1', label: '用户名', type: 'username' }] });
    ok('filled 但用户没填任何字段 → filled:[]（不造空值）', filled0.state === 'filled' && filled0.filled.length === 0, JSON.stringify(filled0));
    ok('audit 记 decision:"filled" 且 fields 是元数据', auditEntries('secret.request').at(-1)?.decision === 'filled' && auditEntries('secret.request').at(-1)?.fields?.[0]?.type === 'username');

    const SECRET = 'TOPSECRET_OTP_913757';
    hijack = () => ({ id: 'sr_hijack', promise: Promise.resolve({ state: 'filled', values: { f1: SECRET } }) });
    const { e: unresolvable } = await grab(() => call('secret.request', { fields: [{ ref: '@e1', label: '用户名', type: 'username' }] }));
    ok('值存在但 ref 不可解析 → 填充失败如实报错', unresolvable?.code === 'REF_UNKNOWN', describe(unresolvable));
    ok('audit 记 decision:"failed"', auditEntries('secret.request').at(-1)?.decision === 'failed');
    const leakedAudit = JSON.stringify(audit.entries).includes(SECRET);
    const leakedErr = JSON.stringify(unresolvable || {}).includes(SECRET);
    ok('值不进 audit / 不进错误（只在内存）', !leakedAudit && !leakedErr);

    hijack = (_args) => ({ id: 'sr_hijack', promise: Promise.reject(err('SECRET_REQ_TIMEOUT', '用户在 120s 内没有提交（popup 关闭或超时）')) });
    const { e: timeout } = await grab(() => call('secret.request', { fields: [{ ref: '@e1', label: '用户名', type: 'username' }] }));
    ok('超时 → SECRET_REQ_TIMEOUT', timeout?.code === 'SECRET_REQ_TIMEOUT', describe(timeout));
    ok('audit 记 decision:"timeout"', auditEntries('secret.request').at(-1)?.decision === 'timeout');

    const outsider = await chromeTabs.create({ url: 'https://elsewhere.example/' });
    const { e: notOwned } = await grab(() => call('secret.request', { tab_id: outsider.id, fields: [{ ref: '@e1', label: 'x', type: 'text' }] }));
    ok('非会话 tab → TAB_NOT_OWNED', notOwned?.code === 'TAB_NOT_OWNED', describe(notOwned));
  } finally {
    secReq.create = realCreate;
    await sessions.stop({ close_marked: true }).catch(() => {});
  }
}

// ============================================================ 6. policy strict 风险门控
console.log('\n[6] policy strict 风险门控');
{
  await setStrict(false);
  resetStrictCache();
  ok('默认 strict 关', (await isStrict()) === false);
  // evaluate/cdp.call 走到工具本体需要会话 tab（strict off 时门控放行到工具）
  const s6 = await sessions.start();
  const t6 = await sessions.createTab('about:blank');
  const offRes = await handled('history.list', { query: 'h' });
  ok('strict off：history.list 照常（现状零变化）', Array.isArray(offRes?.items), JSON.stringify(offRes)?.slice(0, 60));
  const evOff = await handle('evaluate', { expression: '1', world: 'main', tab_id: t6.tab_id });
  ok('strict off：evaluate 的 NEEDS_CONFIRM 是工具自己的（无 data.risk）', evOff?.error?.code === 'NEEDS_CONFIRM' && evOff?.error?.data?.risk === undefined, JSON.stringify(evOff?.error));

  await setStrict(true);
  resetStrictCache();
  ok('policy strict 开', (await isStrict()) === true);
  const gated = await handle('history.list', { query: 'h' });
  ok('strict on：无 confirm → NEEDS_CONFIRM 且 data.risk=transmit', gated?.error?.code === 'NEEDS_CONFIRM' && gated?.error?.data?.risk === 'transmit', JSON.stringify(gated?.error));
  const passed = await handled('history.list', { query: 'h', confirm: true });
  ok('strict on：confirm:true 放行', Array.isArray(passed?.items), JSON.stringify(passed)?.slice(0, 60));
  const evGated = await handle('evaluate', { expression: '1', world: 'main', tab_id: t6.tab_id });
  ok('strict on：evaluate 门控带 data.risk=raw_cdp', evGated?.error?.code === 'NEEDS_CONFIRM' && evGated?.error?.data?.risk === 'raw_cdp', JSON.stringify(evGated?.error));
  const cdpGated = await handle('cdp.call', { method: 'Runtime.evaluate', tab_id: t6.tab_id });
  ok('strict on：cdp.call 门控带 data.risk=raw_cdp（在白名单判定之前）', cdpGated?.error?.code === 'NEEDS_CONFIRM' && cdpGated?.error?.data?.risk === 'raw_cdp', JSON.stringify(cdpGated?.error));

  // policy.set / policy.get 工具
  const { e: pc } = await grab(() => call('policy.set', { strict: true }));
  ok('policy.set 开 strict 无 confirm → NEEDS_CONFIRM', pc?.code === 'NEEDS_CONFIRM', describe(pc));
  const setRes = await call('policy.set', { strict: true, confirm: true });
  ok('policy.set confirm:true → {strict:true}', setRes.strict === true, JSON.stringify(setRes));
  const getRes = await call('policy.get', {});
  ok('policy.get 返回 {strict, risk_classes}', getRes.strict === true && JSON.stringify(getRes.risk_classes) === JSON.stringify(['captcha', 'delete', 'transmit', 'raw_cdp']), JSON.stringify(getRes));
  ok('ubb_policy_strict 落在 chrome.storage.local', (await localStore.get('ubb_policy_strict')).ubb_policy_strict === true);
  await call('policy.set', { strict: false });
  ok('policy.set strict:false 关闭（无需 confirm）', (await call('policy.get', {})).strict === false);

  const { e: diag } = await grab(() => call('browser.diagnostics', {}));
  ok('daemon 不可达 → DIAG_UNKNOWN', diag?.code === 'DIAG_UNKNOWN', describe(diag));

  await sessions.stop({ close_marked: true }).catch(() => {}); // 清场 s6

  await setStrict(false); // 清场：不污染同进程后续
}

// ============================================================ 7. bot.report
console.log('\n[7] bot.report');
{
  const s = await sessions.start();
  const t = await sessions.createTab('about:blank');
  await chrome.tabs.update(t.tab_id, { url: 'https://mall.example/search?q=gpu' });

  const r1 = await call('bot.report', { reason: 'sign_in_blocked', detail: 'x'.repeat(500) });
  ok('上报成功 → {status:"reported", hostname}', r1.status === 'reported' && r1.hostname === 'mall.example', JSON.stringify(r1));
  const entry = auditEntries('bot.report').at(-1);
  ok('audit 只记 hostname（不记全 URL）', entry?.hostname === 'mall.example' && entry?.url === undefined, JSON.stringify(entry));
  ok('detail 截断 200', typeof entry?.detail === 'string' && entry.detail.length === 200, String(entry?.detail?.length));

  const { e: bad } = await grab(() => call('bot.report', { reason: 'because_i_felt_like_it' }));
  ok('白名单外 reason → BAD_REASON', bad?.code === 'BAD_REASON', describe(bad));

  const r2 = await call('bot.report', { reason: 'unexpected_bot_error', tab_id: 999999 });
  ok('tab 查不到 → hostname 空串（上报仍可执行）', r2.status === 'reported' && r2.hostname === '', JSON.stringify(r2));

  // challenge.solve 失败自动上报：源码钉子（跑真 solve 需要整棵 CDP 世界）
  const { readFileSync } = await import('node:fs');
  const src = readFileSync(new URL('../lib/tools-challenge.js', import.meta.url), 'utf8');
  ok("challenge.solve 失败终态自动上报（audit action:'bot.report'）", src.includes("action: 'bot.report'"), '');
  ok('映射：wrong_or_unchanged → challenge_loop', src.includes("return 'challenge_loop'"));
  ok('映射：打满重试仍被挡 → access_denied', src.includes("return 'access_denied'"));
  ok('映射：timeout/failed → captcha_failed', src.includes("return 'captcha_failed'"));
  await sessions.stop({ close_marked: true }).catch(() => {});
}

// ============================================================ 8. emitCursor（增量需求）
console.log('\n[8] emitCursor');
{
  const t = await chromeTabs.create({ url: 'https://cursor.example/x' });
  const before = chromeTabs._sent.length;
  const sentOk = await emitCursor(t.id, { cx: 12.6, cy: 40.2 }, 'click', '提交订单按钮的名字特别长会被截断到这里为止');
  const msg = chromeTabs._sent.slice(before).find((m) => m.msg?.type === 'UBB_CURSOR');
  ok('emitCursor 发出 UBB_CURSOR（盒中心取整）', sentOk === true && msg?.msg?.x === 13 && msg?.msg?.y === 40, JSON.stringify(msg));
  ok('label = 动词 + 元素名截 16 字', msg?.msg?.label === '点击 提交订单按钮的名字特别长会被截断', JSON.stringify(msg?.msg?.label));
  ok('发送带 {frameId:0}（top frame 渲染）', msg?.opts?.frameId === 0, JSON.stringify(msg?.opts));
  ok('无坐标锚点 → 不发（光标不落 (0,0)）', (await emitCursor(t.id, null, 'type')) === false && !chromeTabs._sent.slice(before + 1).some((m) => m.msg?.type === 'UBB_CURSOR'));
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
