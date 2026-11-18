// secret.request 的后台待处理仓库（ABSORB-CODEX-PLAN §14，CORE-1 建，CORE-2 用）。
// 工具端 create() 挂一个 promise 等用户；popup 经 background.js 的
// CUA_POPUP_SECREQ_SUBMIT / CUA_POPUP_SECREQ_CANCEL 分支调 submit()/cancel()。
//
// API（CORE-2 直接 import 使用）：
//   secReq.create({ tab_id, origin, title, fields:[{id,label,type}], timeout_ms })
//     → { id, promise }；promise resolve {state:"filled", values} |
//     {state:"cancelled"}，超时 reject err('SECRET_REQ_TIMEOUT')。
//     同 tab 已有 pending → throw err('SECRET_REQ_BUSY')。
//   secReq.submit(id, values:{fieldId:string}) → boolean（popup 提交）
//   secReq.cancel(id) → boolean（popup 取消）
//   secReq.listMeta() → [{id, tab_id, origin, title, fields, created_at, expires_at}]
//
// 值只在内存：values 存在 Map 里，resolve 后即弃；元数据（无值）同步到
// chrome.storage.session 'cua_sec_requests' 供 popup 轮询 —— 绝不写 local、
// 不进 audit、不进 daemon。

import { err, randomHex } from './config.js';

const SESSION_KEY = 'cua_sec_requests';
const FIELD_TYPES = new Set(['username', 'password', 'otp', 'text']);
const MAX_FIELDS = 8;
const LABEL_MAX = 40;
const FIELD_ID_MAX = 64;
const DEFAULT_TIMEOUT_MS = 120_000;

class SecReqStore {
  constructor() {
    this.pending = new Map(); // id → { meta, resolve, reject, timer, tabId }
  }

  async _sync() {
    try {
      await chrome.storage.session.set({ [SESSION_KEY]: this.listMeta() });
    } catch { /* storage.session 不可用时 popup 轮询拿不到列表，工具链路不受影响 */ }
  }

  _now() {
    return Date.now();
  }

  /** fields 校验（工具端也校验；这里是消息入口的最后一道）：≤8 个、type 枚举
   *  封闭（agent 无法声明 card/cvv 类）、id 唯一且有界、label ≤40 字。 */
  validateFields(fields) {
    if (!Array.isArray(fields) || !fields.length) throw err('BAD_PARAMS', 'fields required');
    if (fields.length > MAX_FIELDS) throw err('BAD_PARAMS', `fields 最多 ${MAX_FIELDS} 个`);
    const ids = new Set();
    for (const f of fields) {
      if (!f || typeof f !== 'object') throw err('BAD_PARAMS', 'fields 每项必须是对象');
      if (typeof f.id !== 'string' || !f.id.length || f.id.length > FIELD_ID_MAX) {
        throw err('BAD_PARAMS', `field.id 必须是 1-${FIELD_ID_MAX} 字符的字符串`);
      }
      if (ids.has(f.id)) throw err('BAD_PARAMS', `field.id 重复：${f.id}`);
      ids.add(f.id);
      if (!FIELD_TYPES.has(f.type)) {
        throw err('BAD_PARAMS', `field.type 只允许 ${[...FIELD_TYPES].join('|')}`);
      }
      if (typeof f.label !== 'string' || !f.label.trim() || f.label.length > LABEL_MAX) {
        throw err('BAD_PARAMS', `field.label 必须是 1-${LABEL_MAX} 字`);
      }
    }
  }

  create({ tab_id, origin, title, fields, timeout_ms } = {}) {
    this.validateFields(fields);
    // 同 tab 单飞：一个 tab 同时只挂一个等待中的请求
    for (const e of this.pending.values()) {
      if (e.tabId === tab_id) {
        throw err('SECRET_REQ_BUSY', `tab ${tab_id} 已有一个等待中的 secret.request（id ${e.meta.id}）`);
      }
    }
    const id = 'sr_' + randomHex(8);
    const created_at = this._now();
    const ms = Number.isFinite(timeout_ms) && timeout_ms > 0 ? timeout_ms : DEFAULT_TIMEOUT_MS;
    const expires_at = created_at + ms;
    const meta = {
      id,
      tab_id: tab_id ?? null,
      origin: String(origin || ''),
      title: String(title || '').slice(0, 80),
      // 元数据只带 id/label/type —— 值永远不进这里
      fields: fields.map((f) => ({ id: f.id, label: f.label, type: f.type })),
      created_at,
      expires_at,
    };
    const store = this;
    const promise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!store.pending.has(id)) return;
        store.pending.delete(id);
        store._sync();
        reject(err('SECRET_REQ_TIMEOUT', `用户在 ${Math.round(ms / 1000)}s 内没有提交（popup 关闭或超时）`));
      }, Math.max(1, ms));
      this.pending.set(id, { meta, resolve, reject, timer, tabId: tab_id ?? null });
    });
    this._sync();
    return { id, promise };
  }

  _finish(id, outcome) {
    const e = this.pending.get(id);
    if (!e) return false;
    clearTimeout(e.timer);
    this.pending.delete(id);
    this._sync();
    if (outcome.state === 'filled') e.resolve({ state: 'filled', values: outcome.values });
    else e.resolve({ state: 'cancelled' });
    return true;
  }

  /** popup 提交。只收声明的字段（多余字段丢弃），返回是否真的有一个等待中的
   *  请求被这次提交终结。 */
  submit(id, values) {
    const e = this.pending.get(id);
    if (!e || !values || typeof values !== 'object') return false;
    const clean = {};
    for (const f of e.meta.fields) {
      if (typeof values[f.id] === 'string') clean[f.id] = values[f.id];
    }
    return this._finish(id, { state: 'filled', values: clean });
  }

  cancel(id) {
    return this._finish(id, { state: 'cancelled' });
  }

  /** 元数据视图（无值）；顺带惰性清掉已过期项（其 promise reject
   *  SECRET_REQ_TIMEOUT）。 */
  listMeta() {
    const now = this._now();
    let dirty = false;
    const out = [];
    for (const [id, e] of this.pending) {
      if (e.meta.expires_at <= now) {
        clearTimeout(e.timer);
        this.pending.delete(id);
        e.reject(err('SECRET_REQ_TIMEOUT', '请求已过期'));
        dirty = true;
      } else {
        out.push({ ...e.meta, fields: e.meta.fields.map((f) => ({ ...f })) });
      }
    }
    if (dirty) this._sync();
    return out;
  }

  /** 某 tab 是否已有 pending（工具端可在 create 前快速探测）。 */
  pendingFor(tabId) {
    for (const e of this.pending.values()) {
      if (e.tabId === tabId) return e.meta.id;
    }
    return null;
  }
}

export const secReq = new SecReqStore();
