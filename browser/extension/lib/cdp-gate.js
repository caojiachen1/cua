// CDP 三层闸（Codex 报告 §7 吸收，ABSORB-CODEX-PLAN §4）：
//   层1 主开关 cua_cdp_raw（默认关 —— tools.js 的 cdp.call 在白名单未命中时
//       调 gateFullCdpCall()，开关关闭时抛出与现状逐字一致的拒绝，行为零变化）；
//   层2 origin 批准（cua_cdp_origins + shield.confirm 两轮 banner）；
//   层3 方法黑名单（本文件 CDP_BLOCKLIST，永远拒绝并给中文 reason）。
// 本模块只做决策与状态；audit 与 storage 写入也在这些 helper 里完成。

import { err } from './config.js';
import { shield } from './shield-ctl.js';
import { audit } from './audit.js';

// 存储键（契约 §3 冻结；config.js 不归本次改动，键名就近定义）
export const CDP_RAW_KEY = 'cua_cdp_raw'; // bool，默认 false
export const CDP_ORIGINS_KEY = 'cua_cdp_origins'; // { <origin>: {granted_at, lifetime:"once"|"session"|"forever"} }

// ── 层3：方法黑名单 ──────────────────────────────────────────────────────────
// 定稿原则（契约 §4）：能关/崩浏览器或页面、能旁路调试器、能开全局拦截
// （CUA 的网络模拟走 net.mock，raw CDP 不许再开一路拦截）、以及破坏性清除。
// 无害 read（Browser.getVersion / SystemInfo.getInfo / Network.setCacheDisabled）
// 一律放行；'域.*' 为前缀通配。
export const CDP_BLOCKLIST = {
  // 能关/崩浏览器或页面
  'Browser.close': '会关闭整个浏览器',
  'Browser.crash': '会让浏览器进程崩溃',
  'Page.crash': '会直接让当前页面崩溃',
  'Target.closeTarget': '可以关闭任意标签页，绕过 tab 闸',
  'Target.createTarget': '能新开页面，绕过 tab.create 的分组与 URL 闸',
  // 能旁路调试器（CUA 自己管理调试会话，不许 raw CDP 另开一路）
  'Target.attachToTarget': '绕过 CUA 的调试器会话管理另开调试通道',
  'Target.setAutoAttach': '会自动旁路附加到新 target，绕过会话管理',
  'Debugger.*': '调试器域可冻结/单步/改写页面执行，绕过 CUA 的 evaluate 闸',
  // 能开全局拦截（与 net.mock 冲突，且会挂死页面请求）
  'Fetch.*': '全局请求拦截与 CUA net.mock 冲突，且会挂死页面请求',
  'Network.setRequestInterception': '全局请求拦截与 CUA net.mock 冲突（遗留接口）',
  // 旁路/破坏类
  'Page.setBypassCSP': '会绕过页面 CSP，等同关闭站点安全策略',
  'Page.setDownloadBehavior': '会绕过 CUA 下载台账改写下载行为',
  'Browser.setDownloadBehavior': '会在浏览器级绕过 CUA 下载台账改写下载行为',
  'Storage.clearDataForOrigin': '会清除站点存储数据（破坏性）',
  'Storage.clearCookies': '会清除浏览器 Cookie（破坏性）',
};

/** 命中黑名单 → 返回中文 reason；否则 null。精确键优先，'域.*' 走前缀。 */
export function blockReason(method) {
  const m = String(method || '');
  if (CDP_BLOCKLIST[m]) return CDP_BLOCKLIST[m];
  for (const k of Object.keys(CDP_BLOCKLIST)) {
    if (k.endsWith('.*') && m.startsWith(k.slice(0, -1))) return CDP_BLOCKLIST[k];
  }
  return null;
}

export function isBlocked(method) {
  return blockReason(method) != null;
}

/** 层3+层2 的纯决策（便于离线测试）。cfg.granted = 该 origin 是否已有批准记录。
 *  主开关（层1）不在这里判 —— gateFullCdpCall 负责它。 */
export function fullAllowed(method, origin, cfg = {}) {
  const reason = blockReason(method);
  if (reason) return { ok: false, reason: `方法被黑名单拒绝：${reason}` };
  if (!/^https?:/i.test(String(origin || ''))) {
    return { ok: false, reason: '原始 CDP 调试只对 http/https 页面开放' };
  }
  if (!cfg.granted) return { ok: false, reason: `网站 ${origin} 尚未批准原始 CDP 调试` };
  return { ok: true };
}

// ── 层1/层2：开关与 origin 批准的存取 ────────────────────────────────────────

export async function getRawEnabled() {
  const st = await chrome.storage.local.get(CDP_RAW_KEY);
  return st[CDP_RAW_KEY] === true;
}

export async function getOrigins() {
  const st = await chrome.storage.local.get(CDP_ORIGINS_KEY);
  const o = st[CDP_ORIGINS_KEY];
  return o && typeof o === 'object' ? o : {};
}

async function setOrigins(o) {
  await chrome.storage.local.set({ [CDP_ORIGINS_KEY]: o });
}

/** 写入批准记录；banner 只授予 once/session，forever 由 popup 面板管理。 */
export async function grantOrigin(origin, lifetime) {
  const o = await getOrigins();
  o[origin] = { granted_at: Date.now(), lifetime };
  await setOrigins(o);
  return o[origin];
}

/** "once" 档用后即删（tools.js 的 cdp.call 在每次 full 档调用后调用）。 */
export async function consumeOnce(origin) {
  const o = await getOrigins();
  if (o[origin] && o[origin].lifetime === 'once') {
    delete o[origin];
    await setOrigins(o);
    return true;
  }
  return false;
}

/** popup 的 cdp_revoke_origin。 */
export async function revokeOrigin(origin) {
  const o = await getOrigins();
  if (o[origin]) {
    delete o[origin];
    await setOrigins(o);
    return true;
  }
  return false;
}

/** session 档清理：session.stop（background.js 包装）与浏览器冷启动时调用。
 *  once 用后即删、forever 保留 —— 只清 lifetime==="session"。 */
export async function clearSessionGrants() {
  const o = await getOrigins();
  let n = 0;
  for (const k of Object.keys(o)) {
    if (o[k] && o[k].lifetime === 'session') {
      delete o[k];
      n += 1;
    }
  }
  if (n) await setOrigins(o);
  return n;
}

// 拒绝缓存：拒绝过就不再逐次弹 banner（SW 内存，浏览器重启自然清零）
const DENY_TTL_MS = 10 * 60_000;
const deniedAt = new Map(); // origin → Date.now()

export function recentlyDenied(origin) {
  const at = deniedAt.get(origin);
  return at != null && Date.now() - at < DENY_TTL_MS;
}

export function noteDenied(origin) {
  deniedAt.set(origin, Date.now());
}

export function clearDenials() {
  deniedAt.clear();
}

function originDeniedErr(origin) {
  return err('CDP_ORIGIN_DENIED', `网站 ${origin} 未批准原始 CDP 调试（或此前已拒绝）`, {
    approve_hint: '在 CUA 弹窗-原始CDP 面板中可永久允许该网站',
  });
}

// banner 两轮合计必须落在 cdp.call 的 40s 工具超时之内：
// 第一轮（允许/拒绝）20s + 第二轮（本会话记住与否）15s
const CONFIRM_ROUND1_MS = 20_000;
const CONFIRM_ROUND2_MS = 15_000;

/** 解析 tab 的 http/https origin；不可批准的页面直接拒绝（banner 都不弹）。 */
async function originOf(tabId) {
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  if (!tab) throw err('TAB_NOT_FOUND', `tab ${tabId} does not exist`);
  let origin = '';
  try {
    origin = new URL(tab.url || '').origin;
  } catch { /* unparseable → '' */ }
  if (!/^https?:/i.test(origin)) {
    throw err(
      'CDP_ORIGIN_DENIED',
      `此 tab（${String(tab.url || '').slice(0, 80)}）不是普通网页，原始 CDP 调试没有 origin 批准通道`,
      { approve_hint: '原始 CDP 仅对 http/https 页面开放' },
    );
  }
  return origin;
}

/**
 * full 档闸入口（tools.js 的 cdp.call 在方法不在 CDP_READ/CDP_WRITE 白名单时调用）。
 * defaultDenyErr = 默认档（cua_cdp_raw=false）的拒绝错误 —— 由 tools.js 构造，
 * 保证逐字与现状一致。返回 {origin, once} 供调用方在调用完成后 consumeOnce。
 * 抛出：defaultDenyErr() / CDP_METHOD_BLOCKED(带 reason) / CDP_ORIGIN_DENIED。
 */
export async function gateFullCdpCall(tabId, method, defaultDenyErr) {
  // 层1：主开关。关 → 与现状完全一致（含错误文案）。
  if (!(await getRawEnabled())) throw defaultDenyErr();
  // 层3：方法黑名单（先于 origin —— 危险方法无论哪个网站都拒绝）。
  const reason = blockReason(method);
  if (reason) throw err('CDP_METHOD_BLOCKED', `方法被黑名单拒绝：${reason}`, { reason });
  // 层2：origin 批准。
  const origin = await originOf(tabId);
  const origins = await getOrigins();
  if (origins[origin]) return { origin, once: origins[origin].lifetime === 'once' };
  if (recentlyDenied(origin)) throw originDeniedErr(origin);
  // —— banner：shield.confirm 只有允许/拒绝两个按钮，按契约做两轮 ——
  //   第一轮：允许本次 / 拒绝；第二轮（仅允许后）：是否本会话内记住。
  //   forever 档不由 banner 提供（popup 面板管理）。
  const allow = await shield.confirm(tabId, `CUA 请求在此网站使用原始 CDP 调试：${origin}（允许本次调用）`, CONFIRM_ROUND1_MS);
  if (!allow) {
    noteDenied(origin);
    audit.log({ action: 'cdp.origin_deny', tab_id: tabId, origin });
    throw originDeniedErr(origin);
  }
  const remember = await shield.confirm(tabId, `在本次会话内记住允许？（${origin} 的原始 CDP 调试将不再逐次询问）`, CONFIRM_ROUND2_MS);
  await grantOrigin(origin, remember ? 'session' : 'once');
  audit.log({ action: 'cdp.origin_grant', tab_id: tabId, origin, lifetime: remember ? 'session' : 'once' });
  return { origin, once: !remember };
}
