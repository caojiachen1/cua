// OOPIF 真实 CDP（Codex 报告 §5 吸收，ABSORB-CODEX-PLAN §5）。
// out-of-process iframe 拿不到页面会话的通道：Page.getFrameTree 里没有它、
// Input.*/DOM.* 也进不去（frameIsOutOfProcess 的结论）。
//
// 真机结论（Edge 157，2026-11-14 实测）决定了传输层形态：
//   1) chrome.debugger.getTargets() 里 OOPIF 以 type:'other' + URL 出现
//      （条目只有 attached/id/title/type/url，无 tabId —— 同 URL 多实例只能诚实
//      OOP_TARGET_AMBIGUOUS）；type:'iframe' 反而根本不出现。
//   2) chrome.debugger.attach({targetId}) 被拒（"Either tab id or extension id
//      must be specified"）—— targetId 直连在这条 Edge 上不可用。
// 因此本模块走【页面会话隧道】：tab 的 debugger 会话（cdp.js 已持有）内发
// Target.attachToTarget({targetId, flatten:false}) 拿 child sessionId，命令经
// Target.sendMessageToTarget({sessionId, message}) 打进 iframe 进程，响应以
// Target.receivedMessageFromTarget 事件流回（cdp.eventHooks 分拣）。全部纪律
// 镜像 lib/cdp.js：LRU ≤ 8 条、30s 空闲拆隧道、45s 命令超时（CDP_CMD_TIMEOUT_MS）、
// corpse 清理。evaluate/upload_file 的 frame 参数走本文件导出的 evaluateInFrame /
// uploadFileInFrame；同进程 frame 一律不进本模块（返回 null 让调用方维持原路径）。

import { err, CDP_CMD_TIMEOUT_MS } from './config.js';
import { cdp, flattenFrameTree } from './cdp.js';
import { listExtFrames } from './snapshot.js';
import { refs } from './refs.js';
import { redact, truncate } from './redact.js';
import { audit } from './audit.js';

const OOP_MAX_ENTRIES = 8;
const OOP_IDLE_MS = 30_000;
const ATTACH_DEADLINE_MS = 15_000;

/** target 匹配（纯函数，便于离线测试）：URL 等值且 type 为 iframe（Chromium
 *  文档约定）或 other（真机实测 Edge 157：OOPIF 以 type:'other' 出现）。 */
export function matchOopifTargets(url, targets) {
  return (targets || []).filter((t) => t && (t.type === 'iframe' || t.type === 'other') && t.url === url);
}

/**
 * 纯判定（镜像 snapshot.js 的 frameIsOutOfProcess 判断逻辑，不 import 它 ——
 * 该文件不归本次改动）：同 URL 的扩展 frame 数 > 页面会话 frame 树里同 URL 的
 * 子 frame 数 → 缺席的那批是 out-of-process。树不可用/根 URL 为空时返回空集
 * （不可判定 ≠ 拒绝，与 frameIsOutOfProcess 同款保守方向）。
 */
export function oopifSetFromTree(extFrames, tree) {
  const root = tree?.frameTree?.frame;
  if (!root || !String(root.url || '').trim()) return new Set();
  const rootUrl = String(root.url || '');
  const kids = flattenFrameTree(tree).filter((f) => f.frameId !== root.id);
  const out = new Set();
  for (const f of extFrames || []) {
    const url = String(f.url || '');
    if (f.frameId === 0 || f.frameId === root.id) continue;
    if (!/^https?:/i.test(url) || url === rootUrl) continue;
    const need = (extFrames || []).filter((x) => x.frameId !== 0 && String(x.url || '') === url).length;
    const have = kids.filter((x) => String(x.url || '') === url).length;
    if (have < need) out.add(f.frameId);
  }
  return out;
}

/** frameId（扩展命名空间）是否 out-of-process。 */
export async function isOopif(tabId, frameId) {
  if (!(Number.isInteger(frameId) && frameId > 0)) return false;
  const all = await listExtFrames(tabId).catch(() => null);
  const target = all?.find((f) => f.frameId === frameId);
  const url = String(target?.url || '');
  if (!target || !/^https?:/i.test(url)) return false; // gone / about:blank：不可判定
  let tree;
  try {
    tree = await cdp.send(tabId, 'Page.getFrameTree');
  } catch {
    return false;
  }
  return oopifSetFromTree(all, tree).has(frameId);
}

/** 把 frame 参数（整数 frameId，或匹配 frames.list 的 url/name 串）解析成
 *  扩展 frame 记录；主 frame（0）→ null，调用方维持原路径。 */
export async function resolveFrameParam(tabId, frame) {
  if (frame === 0) return null;
  const all = await listExtFrames(tabId);
  let pick = null;
  if (typeof frame === 'number' && Number.isInteger(frame)) {
    pick = all.find((f) => f.frameId === frame) || null;
    if (!pick) throw err('BAD_PARAMS', `frame ${frame} 不在当前 tab 的 frame 列表里（用 frames.list 查看可用 frame）`);
  } else if (typeof frame === 'string' && frame.trim()) {
    const s = frame.trim();
    if (/^\d+$/.test(s)) {
      pick = all.find((f) => f.frameId === Number(s)) || null;
      if (!pick) throw err('BAD_PARAMS', `frame ${s} 不在当前 tab 的 frame 列表里（用 frames.list 查看可用 frame）`);
    } else {
      // frames.list 语义：先 name 精确匹配，再 url 子串（大小写不敏感）
      const byName = all.filter((f) => f.name && f.name === s);
      const byUrl = byName.length ? byName : all.filter((f) => String(f.url || '').toLowerCase().includes(s.toLowerCase()));
      if (byUrl.length === 0) {
        throw err('BAD_PARAMS', `frame 匹配串「${s}」没有命中任何 frame 的 url/name（用 frames.list 查看可用 frame）`);
      }
      if (byUrl.length > 1) {
        throw err('BAD_PARAMS', `frame 匹配串「${s}」命中 ${byUrl.length} 个 frame — 换更长的子串或直接给 frame_id`, {
          frame_ids: byUrl.slice(0, 8).map((f) => f.frameId),
        });
      }
      pick = byUrl[0];
    }
  } else {
    throw err('BAD_PARAMS', 'frame 必须是 frameId（整数）或 url/name 匹配串');
  }
  return pick.frameId === 0 ? null : pick;
}

class OopifHub {
  constructor() {
    this.entries = new Map(); // `${tabId}:${frameId}` → { tabId, frameId, targetId, sessionId, url, lastUsed }
    this.queues = new Map(); // 命令队列在条目之外：首条命令（也是创建条目的命令）必须串行
    this.pendingById = new Map(); // 命令 id → { resolve, reject, timer, key }
    this.childSessions = new Map(); // targetId → { sessionId, tabId }（setAutoAttach 事件喂进来）
    this._cmdSeq = 0;
    this._inFlight = 0;
    this._sweepTimer = null;
  }

  key(tabId, frameId) {
    return `${tabId}:${frameId}`;
  }

  /** 取（或建立）该 frame 的 target 会话。非 OOPIF / 主 frame / 未知 frame
   *  都在此拒绝 —— send() 的自动建立路径依赖这里抛错。 */
  async ensure(tabId, frameId) {
    const key = this.key(tabId, frameId);
    const hit = this.entries.get(key);
    if (hit) {
      hit.lastUsed = Date.now();
      return hit;
    }
    if (frameId === 0) {
      throw err('FRAME_UNSUPPORTED', '主 frame 就是页面会话本身 — 用 cdp.send 直达，不经 OOPIF 通道');
    }
    if (!(Number.isInteger(tabId) && tabId > 0) || !(Number.isInteger(frameId) && frameId > 0)) {
      throw err('BAD_PARAMS', `oopif 需要整数 tabId/frameId（got ${JSON.stringify(tabId)}/${JSON.stringify(frameId)}）`);
    }
    await cdp.ensure(tabId); // 主会话先在：frame 判定、target 隧道全靠它撑着
    if (cdp.buffers(tabId)?.frameId === frameId) {
      throw err('FRAME_UNSUPPORTED', '主 frame 就是页面会话本身 — 用 cdp.send 直达，不经 OOPIF 通道');
    }
    const all = await listExtFrames(tabId);
    const info = all.find((f) => f.frameId === frameId) || null;
    const url = String(info?.url || '');
    if (!info || !/^https?:/i.test(url)) {
      throw err('FRAME_UNSUPPORTED', `frame ${frameId} 不在当前 tab 的 frame 列表里（或 URL 不可自动化）— 用 frames.list 查看可用 frame`);
    }
    if (!(await isOopif(tabId, frameId))) {
      throw err('FRAME_UNSUPPORTED', `frame ${frameId} 是同进程 frame — 不需要 OOPIF 通道（走既有路径）`);
    }
    const targets = await chrome.debugger.getTargets();
    const cands = matchOopifTargets(url, targets || []);
    if (cands.length > 1) {
      throw err('OOP_TARGET_AMBIGUOUS', `同 URL（${url}）的 OOPIF target 有 ${cands.length} 个，无法确定唯一会话（条目不带宿主 tab 信息）`, {
        candidates: cands.map((t) => ({ target_id: t.targetId || t.id, url: t.url, attached: !!t.attached })),
      });
    }
    if (cands.length === 0) {
      const seen = (targets || [])
        .filter((t) => t && t.type !== 'worker' && t.type !== 'background_page')
        .map((t) => ({ type: t.type, url: String(t.url || '').slice(0, 60) }));
      throw err('OOP_TARGET_NOT_FOUND', `没有找到 URL 相等的 OOPIF target（${url}）— 该 frame 可能尚未提交为独立进程`, {
        iframe_targets_seen: seen.slice(0, 10),
        types_seen: [...new Set((targets || []).map((t) => String(t?.type || '?')))],
      });
    }
    const entry = {
      tabId,
      frameId,
      targetId: cands[0].targetId || cands[0].id,
      url,
      sessionId: null,
      lastUsed: Date.now(),
    };
    await this._openTunnel(entry);
    this.entries.set(key, entry);
    this._evictLRU();
    return entry;
  }

  /** 页面会话里的 CDP 命令（隧道控制面走它；复用 cdp.js 的队列/超时纪律）。 */
  _pageSend(tabId, method, params) {
    return cdp.send(tabId, method, params);
  }

  /** 开隧道：Target.setAutoAttach（页面会话对自己相关的 OOPIF 的既定附加方式
   *  —— attachToTarget 从页面会话直接调会被浏览器以 "Not allowed" 拒绝）。
   *  autoAttach 后 Target.attachedToTarget 事件带回 {sessionId, targetInfo}，
   *  事件钩子（模块尾）喂进 childSessions，这里等它出现。 */
  async _openTunnel(entry) {
    await this._deadline(
      this._pageSend(entry.tabId, 'Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: false }),
      'Target.setAutoAttach',
    ).catch((e) => {
      const msg = String(e?.message || e);
      if (/another debugger|attached/i.test(msg)) {
        throw err('DEBUGGER_CONFLICT', '该 OOPIF 上已附着其他调试器/DevTools');
      }
      throw err('ATTACH_FAILED', `Target.setAutoAttach 失败：${msg}`);
    });
    const sid = await this._waitForChildSession(entry);
    if (!sid) {
      throw err('ATTACH_FAILED', `OOPIF（${entry.url}）的 auto-attach 会话 15s 未到达 — iframe 可能已卸载，重试`);
    }
    entry.sessionId = sid;
  }

  /** 等事件钩子把 targetId 对应的 child sessionId 喂进来（100ms 步进）。 */
  async _waitForChildSession(entry, deadlineMs = ATTACH_DEADLINE_MS) {
    const end = Date.now() + deadlineMs;
    while (Date.now() < end) {
      const hit = this.childSessions.get(entry.targetId);
      if (hit && hit.tabId === entry.tabId && hit.sessionId) return hit.sessionId;
      await new Promise((r) => setTimeout(r, 100));
    }
    return null;
  }
  _evictLRU() {
    while (this.entries.size > OOP_MAX_ENTRIES) {
      const oldest = [...this.entries.entries()].sort((a, b) => a[1].lastUsed - b[1].lastUsed)[0];
      if (!oldest) break;
      this._drop(oldest[0], { tunnel: true });
    }
  }

  _drop(key, { tunnel = false } = {}) {
    const e = this.entries.get(key);
    if (!e) return false;
    this.entries.delete(key);
    this.queues.delete(key);
    // 该条目上还在等的命令全部作废（隧道没了，等也是白等）
    for (const [id, p] of [...this.pendingById.entries()]) {
      if (p.key === key) {
        clearTimeout(p.timer);
        this.pendingById.delete(id);
        p.reject(err('CDP_DETACHED', 'OOPIF 隧道被拆除（tab 关闭/页面会话掉线/空闲回收）'));
      }
    }
    if (tunnel && e.sessionId) {
      // best-effort 拆隧道：页面会话可能本身已经没了
      this._pageSend(e.tabId, 'Target.detachFromTarget', { sessionId: e.sessionId }).catch(() => { /* 已经掉了 */ });
    }
    return true;
  }

  /** 独立 promise 队列（镜像 cdp.send 的序列化纪律，但按 frame 一条队列）。 */
  send(tabId, frameId, method, params) {
    const key = this.key(tabId, frameId);
    let q = this.queues.get(key);
    if (!q) {
      q = { tail: Promise.resolve() };
      this.queues.set(key, q);
    }
    const run = async () => {
      const entry = await this.ensure(tabId, frameId);
      return this._send(entry, method, params);
    };
    const next = q.tail.then(run, run);
    q.tail = next.catch(() => {});
    return next;
  }

  /** 经页面会话隧道向 OOPIF 发一条 CDP 命令：Target.sendMessageToTarget
   *  载荷 {id, method, params}，结果从 Target.receivedMessageFromTarget 事件
   *  分拣回来（模块尾的事件钩子按 pendingById 对号）。 */
  async _send(entry, method, params) {
    this._beginWork();
    const key = this.key(entry.tabId, entry.frameId);
    const id = ++this._cmdSeq;
    const p = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingById.delete(id);
        // corpse 清理（镜像 cdp.js fix21）：不拆隧道的话，残留子会话会污染
        // 下一次 attachToTarget
        this._drop(key, { tunnel: true });
        reject(err('CDP_TIMEOUT', `${method}: OOPIF target ${CDP_CMD_TIMEOUT_MS / 1000}s 无响应（隧道已拆）`));
      }, CDP_CMD_TIMEOUT_MS);
      this.pendingById.set(id, { resolve, reject, timer, key });
    });
    try {
      const message = JSON.stringify({ id, method, params: params || {} });
      await this._pageSend(entry.tabId, 'Target.sendMessageToTarget', { sessionId: entry.sessionId, message }).catch((e) => {
        const m = String(e?.message || e);
        if (/detached|not attached|No target with given id|No session/i.test(m)) {
          this._drop(key, { tunnel: false }); // 隧道/页面会话已死，丢条目让下次重走 ensure
          throw err('CDP_DETACHED', `${method}: OOPIF 隧道中途掉线（${m}）`);
        }
        throw e;
      });
    } finally {
      this._endWork();
    }
    return p;
  }

  _beginWork() {
    this._inFlight += 1;
    if (this._sweepTimer) {
      clearTimeout(this._sweepTimer);
      this._sweepTimer = null;
    }
  }

  _endWork() {
    this._inFlight = Math.max(0, this._inFlight - 1);
    this._armSweep();
  }

  _armSweep() {
    if (this._sweepTimer) clearTimeout(this._sweepTimer);
    this._sweepTimer = setTimeout(() => {
      this._sweepTimer = null;
      this._sweep();
    }, OOP_IDLE_MS);
  }

  /** 30s 空闲拆隧道（now 可注入，测试用）。返回清掉的条数。 */
  _sweep(now = Date.now()) {
    if (this._inFlight > 0 || this.pendingById.size > 0) return 0;
    let dropped = 0;
    for (const [k, e] of this.entries) {
      if (now - e.lastUsed >= OOP_IDLE_MS) {
        this._drop(k, { tunnel: true });
        dropped += 1;
      }
    }
    return dropped;
  }

  /** 单 tab 清理：tab 关闭 / 页面会话中途掉线时调用。返回清掉的条数。 */
  detachTab(tabId) {
    let n = 0;
    for (const k of [...this.entries.keys()]) {
      if (this.entries.get(k)?.tabId === tabId) {
        this._drop(k, { tunnel: true });
        n += 1;
      }
    }
    return n;
  }

  /** 全量清理（SW 退场 / 测试复位）。 */
  detachAll() {
    for (const k of [...this.entries.keys()]) {
      this._drop(k, { tunnel: true });
    }
  }

  /** 15s 死限（attachToTarget 这类控制面命令的兜底，镜像 cdp.js fix21）。 */
  _deadline(promise, method) {
    return new Promise((resolve, reject) => {
      const t = setTimeout(
        () => reject(err('ATTACH_FAILED', `OOPIF ${method} 在 ${ATTACH_DEADLINE_MS / 1000}s 内无响应`)),
        ATTACH_DEADLINE_MS,
      );
      promise.then(
        (v) => {
          clearTimeout(t);
          resolve(v);
        },
        (e) => {
          clearTimeout(t);
          reject(e);
        },
      );
    });
  }
}

export const oopif = new OopifHub();

// tab 关闭 → cdp.forget 钩子清理对应条目（cdp.js 的 forgetHooks 本来就是给
// 功能模块留的；不碰 cdp.onDetach 单槽回调）
cdp.forgetHooks.add((tabId) => {
  try {
    oopif.detachTab(tabId);
  } catch { /* 钩子永不破坏泵 */ }
});

// 页面会话里回来的 Target.receivedMessageFromTarget：按 pendingById 分拣命令
// 响应。child 事件（无 id）本波不接 —— evaluate/upload_file 是纯命令-响应。
if (typeof chrome !== 'undefined' && chrome.debugger?.onEvent?.addListener) {
  cdp.eventHooks.add((tabId, method, params) => {
    if (method === 'Target.attachedToTarget') {
      const ti = params?.targetInfo;
      if (ti?.targetId && params?.sessionId) {
        oopif.childSessions.set(String(ti.targetId), { sessionId: params.sessionId, tabId });
      }
      return;
    }
    if (method !== 'Target.receivedMessageFromTarget') return;
    const raw = params?.message;
    if (typeof raw !== 'string') return;
    let msg;
    try { msg = JSON.parse(raw); } catch { return; }
    if (typeof msg?.id !== 'number') return; // 子进程事件 → 忽略
    const p = oopif.pendingById.get(msg.id);
    if (!p) return;
    oopif.pendingById.delete(msg.id);
    clearTimeout(p.timer);
    if (msg.error) {
      p.reject(err('CDP_ERROR', `${msg.error.message || 'OOPIF target 返回错误'}`, msg.error.data));
    } else {
      p.resolve(msg.result);
    }
  });
}

// ── evaluate / upload_file 的 frame 支持 ─────────────────────────────────────
// 工具注册在 lib/tools.js（R('evaluate') / R('upload_file')），那里各加一行
// 调用（confirm 闸之后）。返回 null = 同进程 frame / 主 frame → 调用方维持原路径。

/** evaluate 的 frame 支持。p.frame = frameId 或 url/name 匹配串。
 *  OOPIF：走该 frame 自己的 target 会话（MAIN world 默认；world:"isolated" 在
 *  target 会话里建 isolated world —— target 会话的主 frame 就是 iframe 文档）。
 *  同进程子 frame：isolated world 走页面会话的 Page.createIsolatedWorld({frameId})；
 *  MAIN world 子帧求值没有诚实通道（Runtime.evaluate 只打主文档），如实拒绝。
 *  输出形状与 evaluate 主路径一致：{ value, frame_id, oopif }。 */
export async function evaluateInFrame(tabId, p) {
  const pick = await resolveFrameParam(tabId, p.frame);
  if (!pick) return null; // 主 frame → 原路径
  const world = p.world === 'isolated' ? 'isolated' : 'main';
  const expression = String(p.expression || '');
  let value;
  if (await isOopif(tabId, pick.frameId)) {
    await oopif.ensure(tabId, pick.frameId);
    let contextId;
    if (world === 'isolated') {
      // target 会话里的 Page.createIsolatedWorld 必填 frameId（e2e/absorb2 实测：
      // 漏传 → Invalid parameters BINDINGS mandatory field missing）。该 target
      // 自己的主 frame 就是 iframe 文档 —— Page.getFrameTree 取它的 id。
      const ft = await oopif.send(tabId, pick.frameId, 'Page.getFrameTree', {});
      const fid = ft?.frameTree?.frame?.id;
      if (!fid) throw err('NO_FRAME_CONTEXT', 'OOPIF target 未返回 frame 树 — iframe 可能仍在加载，稍后重试');
      const w = await oopif.send(tabId, pick.frameId, 'Page.createIsolatedWorld', { frameId: fid, worldName: 'cua-frame' });
      contextId = w?.executionContextId;
      if (contextId == null) {
        // 绝不静默回退 MAIN：沙箱读被升级成页面权限就是事故
        throw err('NO_FRAME_CONTEXT', 'OOPIF target 拒绝创建 isolated world — 本次未执行任何代码');
      }
    }
    const r = await oopif.send(tabId, pick.frameId, 'Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      ...(contextId ? { contextId } : {}),
    });
    if (r?.exceptionDetails) {
      throw err('EVAL_ERROR', r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    }
    value = r?.result?.value;
  } else {
    if (world === 'main') {
      throw err(
        'FRAME_UNSUPPORTED',
        `frame ${pick.frameId} 是同进程子 frame：MAIN world 求值没有诚实通道（Runtime.evaluate 只打主文档）— world:"isolated" 走 CDP isolated world，或用 @d${pick.frameId}.<n> ref 的桥路径`,
      );
    }
    const w = await cdp.send(tabId, 'Page.createIsolatedWorld', { frameId: pick.frameId, worldName: 'cua-frame' });
    if (w?.executionContextId == null) {
      throw err('NO_FRAME_CONTEXT', '浏览器拒绝创建 isolated world — 本次未执行任何代码');
    }
    const r = await cdp.send(tabId, 'Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      contextId: w.executionContextId,
    });
    if (r?.exceptionDetails) {
      throw err('EVAL_ERROR', r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    }
    value = r?.result?.value;
  }
  audit.log({ action: 'evaluate', tabId, world, frame_id: pick.frameId });
  const out = typeof value === 'string' ? truncate(value, 60000) : redact(value);
  return { value: out, frame_id: pick.frameId, oopif: true };
}

/** upload_file 的 frame 支持。目标 frame：显式 p.frame 优先，否则从 ref 自己的
 *  frameId 推。OOPIF：在该 target 自己的 DOM 文档里定位 file input
 *  （DOM.getDocument → DOM.querySelectorAll → DOM.getAttributes →
 *  DOM.setFileInputFiles）；ref 的桥端属性（id/name）可参与唯一化，ref 失效时
 *  退化为"该 frame 内恰好一个 file input"。 */
export async function uploadFileInFrame(tabId, p) {
  const files = (Array.isArray(p.files) ? p.files : [p.file]).filter(Boolean);
  if (!files.length) throw err('BAD_PARAMS', 'files required');
  // 目标 frame：显式参数，或 ref 自带的 frameId
  let pick = null;
  let refEntry = null;
  if (p.frame != null) {
    pick = await resolveFrameParam(tabId, p.frame);
    if (!pick) return null; // 主 frame → 原路径
  }
  if (p.ref) {
    try {
      refEntry = refs.get(tabId, p.ref); // 只取登记信息（不触发桥解析）
    } catch (e) {
      if (e && e.code === 'BAD_REF') throw e; // ref 形状错了就该在入口报错
      // REF_UNKNOWN / REF_STALE：frame 参数给定时仍可继续（见下）
      refEntry = null;
    }
  }
  if (!pick) {
    if (!refEntry || refEntry.frameId == null || refEntry.frameId === 0) return null; // 主 frame → 原路径
    const all = await listExtFrames(tabId);
    pick = all.find((f) => f.frameId === refEntry.frameId) || null;
    if (!pick) return null;
  }
  // ref 与 frame 参数必须指向同一 frame（ref 来自另一个 frame 就是定位错误）
  if (p.ref && refEntry && refEntry.frameId != null && refEntry.frameId !== 0 && refEntry.frameId !== pick.frameId) {
    throw err('BAD_PARAMS', `ref ${p.ref} 属于 frame ${refEntry.frameId}，与 frame 参数（${pick.frameId}）不一致`);
  }
  if (!(await isOopif(tabId, pick.frameId))) return null; // 同进程 → 原路径（含原 FRAME_UNSUPPORTED 行为）

  const entry = await oopif.ensure(tabId, pick.frameId);
  const doc = await oopif.send(tabId, pick.frameId, 'DOM.getDocument', {});
  const rootId = doc?.root?.nodeId;
  if (rootId == null) throw err('DOM_UNAVAILABLE', 'OOPIF target 未返回 DOM 文档 — iframe 可能仍在加载，稍后重试');
  const q = await oopif.send(tabId, pick.frameId, 'DOM.querySelectorAll', { nodeId: rootId, selector: 'input[type=file]' });
  const nodeIds = (q?.nodeIds || []).slice(0, 50);
  const cands = [];
  for (const nid of nodeIds) {
    const at = await oopif.send(tabId, pick.frameId, 'DOM.getAttributes', { nodeId: nid }).catch(() => null);
    const flat = at?.attributes || [];
    const attrs = {};
    for (let i = 0; i + 1 < flat.length; i += 2) attrs[String(flat[i]).toLowerCase()] = flat[i + 1];
    cands.push({ nodeId: nid, attrs });
  }
  if (!cands.length) throw err('BAD_PARAMS', `frame ${pick.frameId} 内没有找到 <input type=file>`);
  // 唯一化：优先用 ref 的桥端描述属性（id/name），退化为"恰好一个"
  let hit = null;
  if (refEntry?.desc?.attributes && typeof refEntry.desc.attributes === 'object') {
    const want = refEntry.desc.attributes;
    const byAttr = cands.filter(
      (c) => (want.id && c.attrs.id === want.id) || (want.name && c.attrs.name === want.name),
    );
    if (byAttr.length === 1) hit = byAttr[0];
    else if (byAttr.length > 1) {
      throw err('BAD_PARAMS', `frame ${pick.frameId} 内有 ${byAttr.length} 个属性匹配的 file input — 给 input 补个 id/name 或重新 snapshot`);
    }
  }
  if (!hit && cands.length === 1) hit = cands[0];
  if (!hit) {
    throw err('BAD_PARAMS', `frame ${pick.frameId} 内有 ${cands.length} 个 file input，无法唯一定位 — 先重新 snapshot 取该 frame 的 @d ref`);
  }
  await oopif.send(tabId, pick.frameId, 'DOM.setFileInputFiles', { nodeId: hit.nodeId, files });
  audit.log({
    action: 'upload_file',
    tabId,
    files: files.map((f) => String(f).split(/[\\/]/).pop()),
    frame_id: pick.frameId,
  });
  return { uploaded: files.length, frame_id: pick.frameId, oopif: true };
}

/* ── 集成者接线点（lib/tools.js） ────────────────────────────────────────────
 * 1) import 区加：
 *      import { evaluateInFrame as oopifEvaluate, uploadFileInFrame as oopifUpload } from './oopif.js';
 * 2) R('evaluate', …) 内、NEEDS_CONFIRM 检查之后、await cdp.ensure(tabId) 之前加：
 *      if (p.frame != null) return oopifEvaluate(tabId, p);
 *    （必须放在 confirm 检查之后：OOPIF MAIN world 求值同样要过 confirm 闸）
 * 3) R('upload_file', …) 内、resolveRef 之前加：
 *      const oo = await oopifUpload(tabId, p);
 *      if (oo !== null) return oo;
 */
