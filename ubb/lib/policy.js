// policy 严格档（ABSORB-CODEX-PLAN §11，CORE-2）。
// strict 关（默认）= 现状零变化；strict 开：风险类（RISK_CLASSES）工具每次
// 调用都必须在【本次参数】里带 confirm:true —— protocol.js 在 dispatch 前读
// 这里的判定并抛 NEEDS_CONFIRM（data.risk=类别）。
// 双通道写入口：popup 的 UBB_POPUP_CONFIG（background.js 直接写 storage）与
// policy.set 工具（lib/tools-lifecycle.js）；本模块只负责读 + 缓存。

const KEY = 'ubb_policy_strict';
const CACHE_TTL_MS = 3000; // 3s 内存缓存：SW 里每次工具调用都读 storage 太吵，
// 而 strict 档只影响"下一次调用要不要 confirm"，3s 的滞后无观察面。

// 风险类别（契约 §11 冻结）：captcha=自动过验证、delete=删数据、
// transmit=数据出域、raw_cdp=原始调试通道。
export const RISK_CLASSES = ['captcha', 'delete', 'transmit', 'raw_cdp'];

let cache = { value: null, at: 0 };

/** strict 档当前是否开启。storage 不可用（离线测试/极端环境）按关处理。 */
export async function isStrict() {
  const now = Date.now();
  if (cache.value !== null && now - cache.at < CACHE_TTL_MS) return cache.value;
  let v = false;
  try {
    if (typeof chrome !== 'undefined' && chrome.storage?.local) {
      const st = await chrome.storage.local.get(KEY);
      v = st[KEY] === true;
    }
  } catch { /* 读不到 = 关 */ }
  cache = { value: v, at: now };
  return v;
}

/** 写入口（policy.set 用）；同时刷新缓存，避免 3s 内读到旧值。 */
export async function setStrict(v) {
  try {
    if (typeof chrome !== 'undefined' && chrome.storage?.local) {
      await chrome.storage.local.set({ [KEY]: v === true });
    }
  } catch { /* 写失败按未写入处理：cache 仍刷新为请求值，下一次读取会纠正 */ }
  cache = { value: v === true, at: Date.now() };
  return v === true;
}

/** 测试/单测复位缓存（不改 storage）。 */
export function resetStrictCache() {
  cache = { value: null, at: 0 };
}
