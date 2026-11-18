// challenge.* tools — human-verification detection, solving, verification.
//
// Deliberately NOT a "bypass" API: every result separates what was DONE
// (clicks dispatched) from what the PAGE admitted (token / widget gone /
// navigation). The second half is the only half that can say "solved".
//
// Vendor coverage is signature-driven (content/challenge.js): reCAPTCHA
// v2/v3/enterprise, hCaptcha, Cloudflare Turnstile + interstitial, GeeTest,
// Arkose/FunCaptcha (press-and-hold), AWS WAF, generic slider, generic
// image-grid, generic text captcha. Unknown widgets are reported as unknown —
// never silently clicked.

import { targetTab, audit, redact } from './actions.js';
import * as challenge from './challenge.js';
import { setChallengeHintProvider } from './snapshot.js';

// challenge.solve 失败终态自动上报 bot.report（契约 §2.2）——不发新工具调用，
// 只在失败出口补一条 audit {action:'bot.report', reason, hostname}。
// 映射：超时/被拒→captcha_failed；循环（错答案原样回）→challenge_loop；
// 硬拒（interstitial 打满重试次数页面仍挡着）→access_denied。
// needs_grid/needs_text/needs_offset 是等 agent 下一步的中间态，absent/
// unavailable 是"没有验证/没有解法"，都不算被挡。
function botReportReason(out) {
  if (!out || typeof out !== 'object') return null;
  if (out.state === 'solved' || out.verified === true) return null;
  if (out.state === 'wrong_or_unchanged') return 'challenge_loop';
  if (out.state === 'timeout' && Number(out.clicks) >= 3) return 'access_denied';
  if (out.state === 'timeout' || out.state === 'failed') return 'captcha_failed';
  return null;
}

async function reportBotBlock(tabId, out) {
  const reason = botReportReason(out);
  if (!reason) return;
  try {
    const t = await chrome.tabs.get(tabId).catch(() => null);
    let hostname = '';
    try {
      hostname = new URL(String(t?.url || '')).hostname;
    } catch { /* 非常规页面 hostname 空串 */ }
    audit.log({ action: 'bot.report', reason, hostname, auto: 'challenge.solve' });
  } catch { /* 上报永不破坏 solve 结果 */ }
}

export function register(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  // snapshot() calls this once per capture; cached 2.5s inside lib/challenge.js
  setChallengeHintProvider((tabId) => challenge.quickHint(tabId));

  R('challenge.detect', {
    category: 'sensing',
    description:
      'Detect human-verification widgets on the target tab across every frame the sensor can reach: reCAPTCHA (v2 checkbox/grid, v3 score, enterprise), hCaptcha, Cloudflare Turnstile + interstitial, GeeTest, Arkose press-and-hold, AWS WAF, generic slider/grid/text captchas. Returns vendor, kind, confidence, evidence, the clickable target composed into top-viewport coordinates, and page-side token state (length only — the token value is never read). Read-only.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.detect(tabId, { deep: p.deep !== false });
    audit.log({ action: 'challenge.detect', tabId, vendor: out.vendor, kind: out.kind, challenge: out.challenge, targets: out.targets.length });
    return redact(out);
  });

  R('challenge.status', {
    category: 'session',
    description:
      'Cheap challenge status for the target tab (top-frame probe, cached ~2.5s) plus any pending challenge.pick session. Use before deciding to solve; use challenge.detect for full frame geometry.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const hint = await challenge.quickHint(tabId);
    const pending = challenge.pendingOf(tabId);
    return redact({
      challenge: !!hint,
      hint: hint?.text || null,
      vendor: hint?.vendor || null,
      kind: hint?.kind || null,
      pending: pending ? { kind: pending.kind || 'grid', round: pending.round || 1, frame_id: pending.frameId, cells: pending.tiles?.length || 0 } : null,
      note: hint ? 'call challenge.solve (auto) — it clicks like a human and verifies page-side' : 'no challenge detected in the top document',
    });
  });

  R('challenge.wait', {
    category: 'session',
    timeoutMs: 45_000,
    description:
      'Wait for a challenge to clear WITHOUT clicking anything (useful after a human solved it, or for a Cloudflare interstitial / score challenge). Returns state solved only when the page admits it: token field non-empty, widget frame gone, or URL/title advanced. timeout is honest — it never reports solved on silence.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.solve(tabId, { mode: 'wait', timeout_ms: p.timeout_ms });
    audit.log({ action: 'challenge.wait', tabId, state: out.state, verified: out.verified === true });
    return redact(out);
  });

  R('challenge.solve', {
    category: 'interaction',
    timeoutMs: 70_000,
    risk: 'captcha',
    description:
      'Solve the detected challenge with real CDP input and human motion (bezier path, jittered hit point, dwell, human drag with overshoot). mode auto picks by kind: checkbox click → grid payload → canvas payload → slider drag (gap or drag-to-end) → press-and-hold → interstitial wait → passive for score-only. Returns state=solved ONLY with page-side evidence; needs_grid/needs_text return a screenshot (data_base64) for challenge.pick; needs_offset returns the slider image and asks for an offset. Warmup moves the pointer for ~1.4s first (real presence, not a teleport). mode:"human" brings the tab forward and waits for a person, then still verifies the page.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    if (p.async === true) {
      const job = challenge.startSolveJob(tabId, {
        mode: p.mode,
        timeout_ms: p.timeout_ms,
        warmup_ms: p.warmup_ms,
        warmup: p.warmup !== false,
        offset: p.offset,
        hold_ms: p.hold_ms,
        prompt: p.prompt,
      });
      audit.log({ action: 'challenge.solve', tabId, mode: p.mode || 'auto', async: true, job_id: job.job_id });
      return job;
    }
    const out = await challenge.solve(tabId, {
      mode: p.mode,
      timeout_ms: p.timeout_ms,
      warmup_ms: p.warmup_ms,
      warmup: p.warmup !== false,
      offset: p.offset,
      hold_ms: p.hold_ms,
      prompt: p.prompt,
    });
    audit.log({
      action: 'challenge.solve', tabId, mode: p.mode || 'auto',
      vendor: out.vendor, state: out.state, verified: out.verified === true,
      clicked: out.clicked ? { role: out.clicked.role, frame: out.clicked.frame_id } : undefined,
    });
    await reportBotBlock(tabId, out);
    return redact(out);
  });

  R('challenge.job', {
    category: 'session',
    description: 'Poll a challenge.solve {async:true} job: {state: running|done|error|lost, result, error}. "lost" means the service worker restarted before the solve finished — inspect the page instead of assuming success.',
  }, async (p = {}) => {
    if (!p.job_id) throw new Error('job_id required');
    return redact(await challenge.solveJobStatus(String(p.job_id)));
  });

  R('challenge.pick', {
    category: 'interaction',
    timeoutMs: 70_000,
    description:
      'Act on a challenge.solve payload. Grid: {"cells":[0-based row-major indices]} clicks those tiles with human motion, submits, waits, and returns the next round as a fresh needs_grid screenshot. Canvas (hCaptcha/Arkose drag puzzles): {"drag":{"from":[fx,fy],"to":[fx,fy]}} with fractions of the canvas box (0..1) — it drags the piece, commits the drop, and clicks Verify when the vendor exposes one. Text: {"answer":"…",confirm:true} — a deliberate, audited exception to the "验证码 never auto-filled" rule; it only types into the captcha field the detector found, never an OTP field. Slider: {"offset":N} drags the handle N CSS px.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.pick(tabId, p);
    audit.log({ action: 'challenge.pick', tabId, state: out.state, kind: out.state === 'needs_grid' ? 'grid' : undefined, cells: p.cells, verified: out.verified === true });
    return redact(out);
  });

  R('challenge.warmup', {
    category: 'session',
    description:
      'Human-presence warmup: bezier pointer walks with jitter plus occasional small wheel ticks for N ms (default 2000). Nothing is clicked. Use before a sensitive form or when a score-based challenge is present. MANDATORY before a coordinate click on a Turnstile checkbox — a cold click is the most reliable way to get flagged (validated live: warmup+click clears, bare click escalates to a hard block).',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.warmup(tabId, { ms: Math.min(Math.max(Number(p.ms) || 2000, 200), 10_000) });
    return { ...out, note: 'pointer activity recorded; no click, no key, no navigation' };
  });

  R('challenge.bg_dump', {
    category: 'diagnostics',
    description: 'Return the in-page canvas inventory (classes/backing size/CSS width) plus the dataURL the last canvasGap read — the decisive check for "is the solver reading the same canvas I am?"',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.callOp(tabId, 'bgDump', {});
    return out;
  });

  R('challenge.canvas_gap', {
    category: 'diagnostics',
    description: 'Direct probe of the in-page canvas gap detection (canvasGap op) — returns the computed offset/method/confidence for the visible slider widget, the per-method candidates (ncc / outline / dark_run) so a disagreement is auditable, and the input fingerprint (canvas digests + piece bbox) that produced the answer. Diagnostic; no input is dispatched.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.callOp(tabId, 'canvasGap', {}, Number(p.frame_id) || 0);
    audit.log({ action: 'challenge.canvas_gap', tabId, ok: !!out?.offset });
    return out;
  });

  R('challenge.cas_form', {
    category: 'sensing',
    description:
      'Read-only probe of a CAS/JSU-style login form (casLoginForm or form[action*=cas]). Returns username/submit viewport-center boxes, whether Edge credential autofill has materialized (pwdlen_audit = password LENGTH only — plaintext never leaves the page), visible error text, and whether the slider captcha layer is open. Edge autofill only materializes after the username field gains FOCUS: click the username coords, call this again, pwdlen_audit > 0 means ready — only then a trusted click on submit. If it stays 0 after two focus attempts the profile has no saved credential and a human must type it.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.callOp(tabId, 'casFormState', {});
    audit.log({ action: 'challenge.cas_form', tabId, present: !!out?.present, filled: out?.pwdlen_audit > 0 });
    return redact(out);
  });

  R('challenge.audit', {
    category: 'diagnostics',
    description:
      'Read-only automation-leak audit of the target document: navigator.webdriver, CDP globals, extension-context leaks, WebGL renderer, document visibility, plugin/language set. Returns verdict clean|risky|blocked with per-risk guidance. A mouse path cannot fix a blocked environment — this tells you which one you are in before blaming the captcha.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const out = await challenge.audit_environment(tabId);
    audit.log({ action: 'challenge.audit', tabId, verdict: out.verdict, risks: out.risks.length });
    return redact(out);
  });

  // exposed for tests: a stable name in the catalog is also documentation

  R('challenge.dom', {
    category: 'diagnostics',
    description: 'DOM skeleton of ONE frame (tag/class/role/aria/draggable/box/text, visible elements only). Use when a vendor widget is visible but detection cannot name it yet — it is the evidence a new signature is written from, never a click path.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const frameId = Number.isInteger(p.frame_id) ? p.frame_id : 0;
    const nodes = await challenge.domDumpOf(tabId, frameId, Math.min(Math.max(Number(p.max) || 140, 10), 400));
    return { frame_id: frameId, count: nodes.length, nodes };
  });

  R('challenge.help', { category: 'diagnostics', description: 'Vendor/kind coverage table and the honest limits of this build (no external solver, no token service).' }, async () => ({
    vendors: ['recaptcha', 'hcaptcha', 'turnstile', 'cloudflare (interstitial)', 'geetest', 'aliyun', 'arkose', 'awswaf', 'generic-slider', 'generic-grid', 'generic-canvas', 'generic-text'],
    modes: ['auto', 'checkbox', 'slider', 'grid', 'canvas', 'text', 'hold', 'human', 'wait'],
    verified_by: ['token field non-empty (length only)', 'widget frame gone', 'URL/title advanced'],
    not_supported: [
      'external token services (CapSolver/2Captcha) — no key, no outbound call in this build',
      'closed shadow DOM widgets (reported as unavailable, never clicked blind)',
      'audio challenges: no local ASR in this build; the human handoff (mode:"human") is the honest path',
      'canvas puzzles need one agent step: the screenshot is returned and the agent passes normalized drag coordinates (challenge.pick)',
      'Cloudflare Enterprise managed challenges that require a valid TLS/HTTP2 fingerprint — browser-side behaviour cannot fix those',
    ],
    ethics: 'This solves challenges on the browser session the user already controls. It does not farm tokens, does not create accounts, and refuses OTP/CVV fields (SENSITIVE_FIELD_NEVER). Site ToS still apply.',
  }));
}
