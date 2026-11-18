// 生命周期与策略工具（ABSORB-CODEX-PLAN §2.1/2.2/2.4/2.5，CORE-2）。
// tab.mark / bot.report / policy.set / policy.get / browser.diagnostics。
// 经 lib/tools-extra.js 一行接入 —— 本文件覆盖 tools.js 里同名的旧 tab.mark
// 注册（dispatcher 是 Map，后注册者生效；旧 {status} 注记语义被本契约取代）。

import { targetTab, sessions, audit, redact, truncate, err } from './actions.js';
import { requestDaemon } from './ws-client.js';
import { isStrict, setStrict, RISK_CLASSES } from './policy.js';

// 契约 §2.2 冻结的 reason 白名单
const BOT_REASONS = [
  'captcha_failed',
  'access_denied',
  'challenge_loop',
  'sign_in_blocked',
  'add_to_cart_blocked',
  'purchase_blocked',
  'unexpected_bot_error',
];

const MARKS = ['deliverable', 'handoff', 'none'];

function tabHostname(url) {
  try {
    return new URL(String(url || '')).hostname;
  } catch {
    return '';
  }
}

export function register(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('tab.mark', {
    category: 'tabs',
    sensitive: false,
    description:
      '给会话标签打交付标记（Codex 吸收）：mark:"deliverable"（✅ 绿徽标 — 交付物，session.stop 默认保留）|"handoff"（⏸ 橙徽标 — 交接物，同样默认保留）|"none"（清除）。' +
      '省略 mark = 返回当前标记。徽标画在页面 favicon 上（content/tab-badge.js），保留 tab 在 session.stop 后留在 📌 CUA 组内、registry 记 orphaned（tab.gc 可清）；close_marked:true 才连它们一起关。' +
      '默认目标 = 会话最近 tab；非会话 tab 拒绝（TAB_NOT_OWNED）。',
  }, async (p = {}) => {
    const tabId = targetTab(p); // 无会话 → NO_SESSION；非会话 tab → TAB_NOT_OWNED
    if (p.mark === undefined) {
      return { tab_id: tabId, mark: sessions.getMark(tabId) || null, badge_set: null };
    }
    if (!MARKS.includes(p.mark)) {
      throw err('BAD_MARK', `mark 只允许 ${MARKS.join('|')}（got ${JSON.stringify(String(p.mark).slice(0, 24))}）`);
    }
    const entry = await sessions.setMark(tabId, p.mark);
    // 徽标经 content/tab-badge.js 落地；消息失败静默（badge 是提示，不是状态源）
    let badge_set = false;
    try {
      await chrome.tabs.sendMessage(tabId, { type: 'CUA_BADGE', mark: p.mark === 'none' ? null : p.mark });
      badge_set = true;
    } catch { /* 无 content script（about:blank / 未注入页）— 标记已落账 */ }
    audit.log({ action: 'tab.mark', tabId, mark: entry ? entry.mark : 'none' });
    return { tab_id: tabId, mark: entry ? entry.mark : null, badge_set };
  });

  R('bot.report', {
    category: 'session',
    sensitive: false,
    description:
      '上报"被网站的反机器人措施挡住了"：challenge.solve 失败会自动上报 captcha 类原因；本工具用于 agent 主动上报业务路径上的 bot 拦截（登录被挡/加购被挡/下单被挡等）。' +
      `reason 只允许 ${BOT_REASONS.join('|')}。只记 tab 的 hostname（不记全 URL），detail 截断 200 字进审计。上报本身不改变任何页面状态。`,
  }, async (p = {}) => {
    const reason = p.reason;
    if (!BOT_REASONS.includes(reason)) {
      throw err('BAD_REASON', `reason 只允许 ${BOT_REASONS.join('|')}（got ${JSON.stringify(String(reason).slice(0, 40))}）`);
    }
    // 默认目标 = 会话最近 tab；没有会话/查不到 tab 时 hostname 记空串 —— 上报
    // 是遥测不是页面动作，永远可执行
    const tabId = p.tab_id != null ? p.tab_id : sessions.active()?.lastTabId ?? null;
    const tab = tabId != null ? await chrome.tabs.get(tabId).catch(() => null) : null;
    const hostname = tabHostname(tab?.url);
    audit.log({
      action: 'bot.report',
      reason,
      hostname,
      // 审计元数据硬截 200（契约：detail截断200 —— 不加 truncate 的省略尾巴）
      ...(p.detail != null ? { detail: String(p.detail).slice(0, 200) } : {}),
    });
    return { status: 'reported', hostname };
  });

  R('policy.set', {
    category: 'session',
    sensitive: false,
    description:
      '设置 CUA 策略档。strict:true 开启严格档 —— captcha/delete/transmit/raw_cdp 四类风险工具每次调用都必须带 confirm:true（policy strict 的 NEEDS_CONFIRM data.risk 会标出类别）。' +
      '开启需要 confirm:true（这会让之后每一步都多一次确认）。strict:false 随时关闭，不需要 confirm。',
  }, async (p = {}) => {
    if (typeof p.strict !== 'boolean') {
      throw err('BAD_PARAMS', `strict 必须是 boolean（got ${JSON.stringify(String(p.strict).slice(0, 24))}）`);
    }
    if (p.strict === true && p.confirm !== true) {
      throw err(
        'NEEDS_CONFIRM',
        '开启 policy strict 档后，challenge.solve / download.clean / secret.set / cdp.call / clipboard.read / tab.close_cua / history.list / evaluate 每次调用都会要求 confirm:true —— 与用户确认后带 confirm:true 重试',
        { strict: true },
      );
    }
    await setStrict(p.strict);
    audit.log({ action: 'policy.set', strict: p.strict });
    return { strict: p.strict };
  });

  R('policy.get', {
    category: 'session',
    sensitive: false,
    description: '当前策略档：{strict, risk_classes} — strict 开启时 risk_classes 里的类别在每次调用时要求 confirm:true。',
  }, async () => {
    return { strict: await isStrict(), risk_classes: [...RISK_CLASSES] };
  });

  R('browser.diagnostics', {
    category: 'diagnostics',
    sensitive: false,
    timeoutMs: 45000,
    description:
      '本机浏览器矩阵诊断（daemon 侧扫描，只读）：各家族（edge/chrome/brave/opera/vivaldi）的安装/运行状态、CUA 扩展在各 profile 的安装与启用、native messaging manifest 校验、daemon 槽位。' +
      '用于安装排障与「用户到底在用哪个浏览器」的判断；daemon 不可达时报 DIAG_UNKNOWN。',
  }, async (p = {}) => {
    let r;
    try {
      r = await requestDaemon('diag.browsers', {}, 30000);
    } catch (e) {
      throw err(
        'DIAG_UNKNOWN',
        `browser.diagnostics 未能从 daemon 取得扫描结果：${String(e?.message || e).slice(0, 160)} — 确认 daemon 在跑（cua browser status / CUA 弹窗状态灯）`,
      );
    }
    return redact(r ?? {});
  });
}
