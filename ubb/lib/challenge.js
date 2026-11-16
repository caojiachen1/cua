// UBB challenge engine (service-worker side).
//
// The page half lives in content/challenge.js and answers per frame: what
// vendor is here, what kind of widget, where its clickable point sits in that
// frame's viewport. This half turns those answers into top-viewport geometry
// (frameStepsFor + composeFrameBoxes), drives real CDP input there, and — the
// part browser-use outsources to its cloud — VERIFIES success from page-side
// evidence (token field / widget gone / navigation), never from a human's
// "done" click.
//
// States are deliberately three-valued, not one timeout:
//   absent   — nothing here (fast, cheap: one top-frame probe)
//   solved   — page admits the challenge is gone or a token exists
//   solving  — the widget advanced to a new round (grid) / still present
//   timeout  — nothing changed inside the budget; say so, don't fake it
//   failed   — a click was dispatched but the page kept the challenge
//
// No external solver, no token service, no server. Human handoff is a real
// solver mode, not a fallback we hide.

import { cdp } from './cdp.js';
import { audit } from './audit.js';
import { err } from './config.js';
import { sleep, withPass, withHiddenShield } from './shield-ctl.js';
import { evalJs, runHelp } from './actions.js';
import { listExtFrames, frameStepsFor, composeFrameBoxes } from './snapshot.js';
import * as human from './human.js';

const PROBE_TIMEOUT_MS = 8000;
const FRAME_BUDGET = 12;
const WAIT_POLL_MS = 500;
const HINT_TTL_MS = 2500;

const hintCache = new Map(); // tabId → {at, hint}
const pending = new Map(); // tabId → grid/answer session for challenge.pick
const jobs = new Map(); // job_id → {id, tabId, state, startedAt, result, error}
const JOB_KEY = 'ubb_challenge_jobs';

function now() { return Date.now(); }

// Long solves can outlive a single RPC (Cloudflare interstitials with retries,
// human handoffs). This is the "submit, poll" half: the promise lives in the
// service worker; a record survives in chrome.storage.session so a restarted
// worker can answer "lost" instead of pretending nothing was submitted.
async function persistJobs() {
  try {
    const arr = [...jobs.values()].slice(-20).map((j) => ({
      id: j.id, tabId: j.tabId, state: j.state, startedAt: j.startedAt, endedAt: j.endedAt || null,
    }));
    await chrome.storage.session.set({ [JOB_KEY]: arr });
  } catch { /* storage is best-effort; the in-memory answer is still honest */ }
}

export function startSolveJob(tabId, opts = {}) {
  const id = `cj-${Math.random().toString(36).slice(2, 10)}${Date.now().toString(36).slice(-4)}`;
  const rec = { id, tabId, state: 'running', startedAt: now(), result: null, error: null };
  jobs.set(id, rec);
  persistJobs();
  solve(tabId, opts).then((r) => {
    rec.state = 'done';
    rec.result = r;
    rec.endedAt = now();
    persistJobs();
  }).catch((e) => {
    rec.state = 'error';
    rec.error = String(e?.message || e).slice(0, 500);
    rec.endedAt = now();
    persistJobs();
  });
  audit.log({ action: 'challenge.solve.async', tabId, job_id: id, mode: opts.mode || 'auto' });
  return { job_id: id, state: 'running', tab_id: tabId, started_at: rec.startedAt, note: 'poll challenge.job {job_id} — the solve keeps running while the tool returns' };
}

export async function solveJobStatus(id) {
  const rec = jobs.get(id);
  if (rec) {
    return {
      job_id: id, state: rec.state, tab_id: rec.tabId, started_at: rec.startedAt, ended_at: rec.endedAt || null,
      result: rec.result || null, error: rec.error || null,
    };
  }
  try {
    const st = await chrome.storage.session.get(JOB_KEY);
    const saved = (st?.[JOB_KEY] || []).find((x) => x.id === id);
    if (!saved) throw err('NOT_FOUND', `no challenge job "${id}" — it was not submitted in this browser session`);
    return {
      job_id: id, state: saved.state === 'running' ? 'lost' : saved.state, tab_id: saved.tabId,
      started_at: saved.startedAt, ended_at: saved.endedAt || null, result: null, error: null,
      note: saved.state === 'running'
        ? 'the service worker restarted before this job finished; its in-memory result is gone. Inspect the page with challenge.status / challenge.detect instead of assuming success.'
        : 'restored from storage; only the final state survived, not the payload',
    };
  } catch (e) {
    if (e?.code) throw e;
    throw err('NOT_FOUND', `no challenge job "${id}"`);
  }
}

/**
 * Trusted input into a document Windows considers hidden never drains into the
 * renderer. The generic input tools learn this after a 7s delivery wait; here
 * we refuse BEFORE dispatch, exactly like guardInputVisibility in lib/tools.js
 * (same error code/shape) without importing the 150k module.
 */
async function guardVisible(tabId, action) {
  const state = await evalJs(tabId, 'document.visibilityState').catch(() => null);
  if (state === null || state === 'visible') return;
  throw err('INPUT_NOT_DELIVERED', `${action}: document.visibilityState = ${JSON.stringify(state)} — a hidden document does not drain trusted input, so nothing was clicked. Bring the window to the foreground (or create the tab with active:true in a visible window) and retry; screenshots/detection still work while hidden.`, {
    tab_id: tabId, stage: 'pre-dispatch', visibility_state: state, dispatched: false, events_on_wire: 0, waited_ms: 0,
  });
}

// --------------------------------------------------------------- transport

async function challengeCall(tabId, op, args = {}, frameId = 0) {
  const sent = chrome.tabs.sendMessage(tabId, { type: 'UBB_CHALLENGE', op, args }, { frameId });
  const r = await Promise.race([
    sent.catch((e) => ({ __error: e })),
    sleep(PROBE_TIMEOUT_MS).then(() => ({ __timeout: true })),
  ]);
  if (r?.__timeout) {
    throw err('CHALLENGE_PROBE_TIMEOUT', `frame ${frameId} did not answer challenge op "${op}" within ${PROBE_TIMEOUT_MS}ms — the frame may be frozen or its content script absent (sandboxed/srcdoc). Nothing was clicked.`);
  }
  if (r?.__error) {
    const msg = String(r.__error?.message || r.__error);
    if (/Receiving end does not exist|could not establish connection/i.test(msg)) {
      throw err('CHALLENGE_UNAVAILABLE', `no challenge sensor in frame ${frameId} (sandboxed iframe, about:blank, or the extension was just reloaded) — detection covers frames the sensor can reach; reload the page once. Nothing was clicked.`);
    }
    throw err('CHALLENGE_PROBE_FAILED', msg);
  }
  if (!r || r.ok !== true) throw err('CHALLENGE_PROBE_FAILED', String(r?.error || `challenge op ${op} failed`));
  return r.result;
}

async function framesOf(tabId) {
  const all = await listExtFrames(tabId).catch(() => null);
  if (!Array.isArray(all) || !all.length) return [{ frameId: 0, parentFrameId: null, url: '' }];
  return all;
}

function vendorFrame(url, extra = '') {
  return /(google\.com\/recaptcha|recaptcha\.net|hcaptcha\.com|challenges\.cloudflare\.com|geetest|arkoselabs|funcaptcha|awswaf|yidun|aliyunCaptcha|tcaptcha|captcha)/i.test(`${url} ${extra}`);
}

// --------------------------------------------------------------- geometry

async function toTop(tabId, frameId, box) {
  if (frameId === 0) return box;
  const { steps } = await frameStepsFor(tabId, frameId);
  const b = composeFrameBoxes(steps, {
    x: box.x, y: box.y, width: box.width, height: box.height,
  });
  return b;
}

function boxOut(b) {
  if (!b) return null;
  return {
    x: Math.round(b.x), y: Math.round(b.y),
    width: Math.round(b.width), height: Math.round(b.height),
    cx: Math.round(b.cx ?? b.x + b.width / 2),
    cy: Math.round(b.cy ?? b.y + b.height / 2),
  };
}

const cfTargetCache = new Map(); // tabId → {at, epoch, target}

/**
 * Cloudflare's managed-challenge Turnstile renders inside a cross-origin
 * `challenges.cloudflare.com` iframe whose host element the interstitial page
 * keeps in a CLOSED shadow root. The page bridge cannot see into it and the
 * frame's own document reports nothing clickable, so a detector that only
 * trusts the bridge answers targets:[] while a "Verify you are human" checkbox
 * is plainly on screen. The iframe ELEMENT is still a node of the top document,
 * and DOM.getFlattenedDocument({pierce:true}) reaches closed shadow roots, so
 * its layout box is recoverable through CDP.
 *
 * Turnstile's normal widget is 300x65 (the compact 150x140 variant is refused
 * here rather than guessed). Its checkbox is a 24px square about 8px in from
 * the left edge and vertically centred, so the point a person clicks is
 * (left + 20, top + height/2).
 */
async function cfWidgetTarget(tabId, epoch = null) {
  const hit = cfTargetCache.get(tabId);
  if (hit && hit.epoch === epoch && now() - hit.at < 1200) return hit.target;
  let target = null;
  try {
    const flat = await cdp.send(tabId, 'DOM.getFlattenedDocument', { depth: -1, pierce: true });
    const boxes = [];
    for (const n of flat?.nodes || []) {
      if (n.nodeName !== 'IFRAME' || !n.nodeId) continue;
      const a = n.attributes || [];
      let src = '';
      for (let i = 0; i + 1 < a.length; i += 2) if (a[i] === 'src') { src = a[i + 1]; break; }
      if (!/challenges\.cloudflare\.com\/cdn-cgi\/challenge-platform\//i.test(src)) continue;
      const bm = await cdp.send(tabId, 'DOM.getBoxModel', { nodeId: n.nodeId }).catch(() => null);
      const q = bm?.model?.border;
      if (!q || q.length < 8) continue;
      const xs = [q[0], q[2], q[4], q[6]], ys = [q[1], q[3], q[5], q[7]];
      const box = {
        x: Math.min(...xs), y: Math.min(...ys),
        width: Math.max(...xs) - Math.min(...xs),
        height: Math.max(...ys) - Math.min(...ys),
      };
      if (box.width >= 160 && box.width <= 460 && box.height >= 32 && box.height <= 120) boxes.push(box);
      if (boxes.length >= 4) break;
    }
    const b = boxes.sort((p, q) => Math.abs(p.width - 300) - Math.abs(q.width - 300))[0];
    if (b) {
      target = {
        x: Math.round(b.x), y: Math.round(b.y),
        width: Math.round(b.width), height: Math.round(b.height),
        cx: Math.round(b.x + 20), cy: Math.round(b.y + b.height / 2),
        name: 'cloudflare-turnstile', role: 'checkbox', frame_id: 0,
        source: 'cf-frame-owner', shadow: 'closed',
      };
    }
  } catch { /* CDP refused or the tree is degenerate → honest "no target" */ }
  cfTargetCache.set(tabId, { at: now(), epoch, target });
  return target;
}

// ---------------------------------------------------------------- detection

const SEVERITY = { interstitial: 6, grid: 5, canvas: 5, slider: 4, hold: 3, text: 3, checkbox: 2, unknown: 1, score: 0 };

/**
 * Probe every frame that could host a widget. The top document always answers
 * (it sees every <iframe src>), so a page with no challenge costs one message.
 * Child frames are only entered when the top's iframe scan names a vendor or
 * the frame URL does — no broadcast, no guessing.
 */
export async function detect(tabId, { deep = true } = {}) {
  const t0 = now();
  const probes = [];
  let top = null;
  try {
    top = await challengeCall(tabId, 'probe', { quick: !deep }, 0);
    probes.push({ frameId: 0, r: top });
  } catch (e) {
    // A restricted page can still be detected from the tab itself, but no
    // sensor means no geometry: report honestly.
    return {
      challenge: false, unavailable: true, detail: String(e?.message || e),
      vendor: null, kind: 'unknown', confidence: 'none', frames: [], targets: [],
      evidence: [], target: null, response_filled: false, elapsed_ms: now() - t0,
    };
  }

  const all = await framesOf(tabId);
  const topUrls = (top.findings || []).map((f) => f.src || '').join(' ');
  // Only enter child frames when the top document actually named a vendor (or
  // a token already proves a challenge). Otherwise a normal page with 12 ad
  // iframes would pay 12 probes per snapshot for nothing.
  const topSpeaks = (top.findings || []).length > 0 || (top.tokens || []).some((t) => t.present);
  if (deep && topSpeaks) {
    const candidates = all
      .filter((f) => f.frameId !== 0)
      .filter((f) => vendorFrame(f.url || '', topUrls) || (f.parentFrameId === 0 && !/^about:|^data:|^blob:/.test(f.url || '')))
      .slice(0, FRAME_BUDGET);
    for (const f of candidates) {
      try {
        const r = await challengeCall(tabId, 'probe', { quick: false }, f.frameId);
        probes.push({ frameId: f.frameId, r, url: f.url });
      } catch { /* a dead frame is not a challenge */ }
    }
  }

  // merge findings (top first: a top-level widget beats a child echo)
  const findings = [];
  for (const p of probes) {
    for (const f of p.r.findings || []) {
      findings.push({ ...f, frame_id: p.frameId, frame_url: p.r.frame?.url || p.url || '' });
    }
  }
  const tokens = probes.flatMap((p) => p.r.tokens || []);
  const tokenPresent = tokens.some((t) => t.length > 0);

  // Belt and braces for vendor-shaped waiting rooms whose copy is not in the
  // text family (ScienceDirect renders a CF challenge in its own page shell:
  // title "Just a moment...", body "Are you a robot?"). A present
  // cf-turnstile-response field plus a challenge-platform frame plus a
  // challenge-looking tab title is the same vendor fact, proved three ways.
  if (!findings.length) {
    const cfFrame = all.some((f) => /challenges\.cloudflare\.com\/cdn-cgi\/challenge-platform\//i.test(f.url || ''));
    const cfToken = tokens.some((t) => /cf-turnstile-response/i.test(t.name || '') && t.present);
    if (cfFrame && cfToken) {
      const tab = await chrome.tabs.get(tabId).catch(() => null);
      if (/just a moment|are you a robot|attention required|checking your browser/i.test(tab?.title || '')) {
        findings.push({
          vendor: 'cloudflare', kind: 'interstitial', confidence: 'high', role: 'interstitial',
          evidence: [{ type: 'title', value: String(tab.title).slice(0, 120) }, { type: 'token', value: 'cf-turnstile-response present' }],
          prompt: 'cloudflare challenge page (vendor-detected from tab title + turnstile field + challenge frame)',
        });
      }
    }
  }

  // a grid/checkbox claim from a child frame only counts when that frame is a
  // real vendor frame; a random page with 9 images must not look like captcha
  const ranked = findings
    .filter((f) => f.kind !== 'score' || findings.length === 1)
    .sort((a, b) => (SEVERITY[b.kind] || 0) - (SEVERITY[a.kind] || 0) || (b.confidence === 'high' ? 1 : 0) - (a.confidence === 'high' ? 1 : 0));
  const primary = ranked[0] || findings[0] || null;

  // targets: compose every candidate into the top viewport. Failure to compose
  // is reported as an unclickable target, never silently dropped.
  const targets = [];
  for (const p of probes) {
    for (const t of p.r.targets || []) {
      if (!t.box || t.box.width < 4 || t.box.height < 4) continue;
      try {
        const b = await toTop(tabId, p.frameId, t.box);
        if (b.width < 4 || b.height < 4) continue;
        targets.push({ ...boxOut(b), name: t.name || '', role: t.role || 'click', frame_id: p.frameId, selector: t.selector || null });
      } catch (e) {
        targets.push({ frame_id: p.frameId, name: t.name || '', role: t.role || 'click', unclickable: String(e?.code || e?.message || e).slice(0, 160) });
      }
    }
  }
  targets.sort((a, b) => (a.unclickable ? 1 : 0) - (b.unclickable ? 1 : 0));

  const kind = primary?.kind || 'unknown';
  const vendor = primary?.vendor || findings.map((f) => f.vendor).find(Boolean) || 'unknown';

  // The bridge cannot name Cloudflare's checkbox (closed shadow root), so an
  // interstitial with no clickable target gets its widget box resolved through
  // CDP instead of being reported as "nothing to click".
  if (kind === 'interstitial' && findings.some((f) => f.vendor === 'cloudflare')
      && !targets.some((t) => !t.unclickable && t.role === 'checkbox')) {
    const cf = await cfWidgetTarget(tabId, top?.frame?.epoch || null).catch(() => null);
    if (cf) targets.unshift(cf);
  }
  const viewport = top?.frame?.viewport || null;
  const challenge = !!primary && kind !== 'score';
  const needsGrid = kind === 'grid';
  const noteByKind = {
    interstitial: 'interstitial: this is a waiting-room page, not the site. Do not fill forms; wait for it to clear or solve the visible checkbox.',
    score: 'score-only: no click is required (invisible v3/Turnstile). Continue the task; the page decides.',
    grid: 'image grid: screenshot is provided; call challenge.pick with the cell indices that match the prompt.',
    canvas: 'canvas puzzle (hCaptcha/Arkose): screenshot is provided; call challenge.pick {drag:{from:[fx,fy],to:[fx,fy]}} with fractions of the canvas (0..1).',
    slider: 'slider: solve finds the gap from pixels (or drags to the far right for Aliyun 2.0); if confidence is low, pass offset from the screenshot.',
    text: 'text captcha: screenshot is provided; call challenge.pick {answer, confirm:true}.',
    hold: 'press-and-hold: solve holds the button for a human-like duration.',
    checkbox: 'checkbox: solve clicks it with a human path, then waits for the grid or the token.',
  };

  return {
    challenge,
    vendor,
    kind,
    viewport,
    viewport_zero: !!viewport && (viewport.w < 100 || viewport.h < 100),
    confidence: primary?.confidence || 'none',
    role: primary?.role || null,
    frames: probes.map((p) => ({
      frame_id: p.frameId,
      url: (p.r.frame?.url || p.url || '').slice(0, 300),
      is_top: p.frameId === 0,
      findings: (p.r.findings || []).map((f) => `${f.vendor || '?'}:${f.kind}`),
    })),
    targets,
    target: targets.find((t) => !t.unclickable) || null,
    evidence: findings.flatMap((f) => (f.evidence || []).map((e) => ({ ...e, frame_id: f.frame_id }))).slice(0, 20),
    response_filled: tokenPresent,
    response_fields: tokens.map((t) => ({ name: t.name, present: t.present, length: t.length })),
    prompt: primary?.prompt || '',
    slide_to_end: !!primary?.slide_to_end,
    // true means an extra step is required (agent vision for grid/canvas/text,
    // a person for hold) — never that the widget is unsolvable.
    requires_human: ['grid', 'canvas', 'text', 'hold'].includes(kind),
    solvable: ['checkbox', 'slider', 'grid', 'canvas', 'text', 'hold', 'interstitial', 'score'].includes(kind),
    note: noteByKind[kind] || null,
    elapsed_ms: now() - t0,
  };
}

/** Cheap banner for snapshot(): top frame only, cached, never throws. */
export async function quickHint(tabId) {
  const hit = hintCache.get(tabId);
  if (hit && now() - hit.at < HINT_TTL_MS) return hit.hint;
  let hint = null;
  try {
    const r = await challengeCall(tabId, 'probe', { quick: true }, 0);
    const f = (r.findings || []).find((x) => x.kind !== 'score');
    if (f) {
      const frame = (r.findings || []).find((x) => x.role === 'anchor' || x.role === 'challenge' || x.role === 'widget');
      hint = {
        vendor: f.vendor || 'unknown',
        kind: f.kind,
        text: `!! challenge: ${f.vendor || 'unknown'} (${f.kind}${frame?.role ? `, ${frame.role}` : ''}) — call challenge.solve before clicking through; do NOT reload or navigate while it runs`,
      };
    }
  } catch { /* snapshots must not fail because a challenge probe did */ }
  hintCache.set(tabId, { at: now(), hint });
  return hint;
}

/** Probe one specific frame for a screenshot-backed map (grid/slider/text). */
async function mapInFrames(tabId, op) {
  const all = await framesOf(tabId);
  const order = [0, ...all.filter((f) => f.frameId !== 0).map((f) => f.frameId)];
  for (const frameId of order) {
    try {
      const r = await challengeCall(tabId, op, {}, frameId);
      if (r) return { frameId, map: r };
    } catch { /* keep looking */ }
  }
  return null;
}

// ------------------------------------------------------------------ shots

async function shot(tabId, clip) {
  return withHiddenShield(tabId, async () => {
    const s = await cdp.send(tabId, 'Page.captureScreenshot', {
      format: 'png',
      clip: {
        x: Math.max(0, clip.x), y: Math.max(0, clip.y),
        width: Math.max(8, clip.width), height: Math.max(8, clip.height), scale: 1,
      },
    });
    return { format: 'png', data_base64: s.data, clip };
  });
}

function padBox(b, p = 8) {
  return { x: b.x - p, y: b.y - p, width: b.width + 2 * p, height: b.height + 2 * p };
}

function union(a, b) {
  if (!a) return b;
  if (!b) return a;
  const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
  return { x, y, width: Math.max(a.x + a.width, b.x + b.width) - x, height: Math.max(a.y + a.height, b.y + b.height) - y };
}

// ---------------------------------------------------------------- actions

async function humanClickAt(tabId, point, { frameId = 0, role = 'click' } = {}) {
  const enabled = await human.isEnabled().catch(() => false);
  await guardVisible(tabId, 'challenge.click');
  const dl = {};
  await withPass(tabId, async () => {
    if (enabled) {
      await human.humanClick(tabId, { x: point.cx - 6, y: point.cy - 6, width: 12, height: 12, cx: point.cx, cy: point.cy });
    } else {
      // still a path, not a teleport: a short bezier walk before pressing is
      // the cheapest signal every behavioural scorer uses
      await human.humanMoveTo(tabId, point.cx + (Math.random() * 12 - 6), point.cy + (Math.random() * 12 - 6));
      await human.humanMoveTo(tabId, point.cx, point.cy);
      await sleep(40 + Math.random() * 90);
      await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x: point.cx, y: point.cy, button: 'left', buttons: 1, clickCount: 1 });
      await sleep(45 + Math.random() * 70);
      await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x: point.cx, y: point.cy, button: 'left', buttons: 1, clickCount: 1 });
    }
  }, { frameId, at: [point.cx, point.cy], delivery: dl });
  return dl;
}

/** Trusted human-path click at viewport coords, for callers outside this
 * module (tools-research CAS login loop). point: {cx, cy}; frameId default 0. */
export async function clickAt(tabId, cx, cy, { frameId = 0, role = 'click' } = {}) {
  return humanClickAt(tabId, { cx, cy, frame_id: frameId }, { frameId, role });
}

async function humanHoldAt(tabId, point, ms = 3200) {
  await guardVisible(tabId, 'challenge.hold');
  await withPass(tabId, async () => {
    await human.humanMoveTo(tabId, point.cx, point.cy);
    await sleep(120 + Math.random() * 180);
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x: point.cx, y: point.cy, button: 'left', buttons: 1, clickCount: 1 });
    // tiny tremor while held — a perfectly still pointer for 3s is its own tell
    const until = now() + ms;
    while (now() < until) {
      await sleep(140 + Math.random() * 120);
      await cdp.send(tabId, 'Input.dispatchMouseEvent', {
        type: 'mouseMoved', x: point.cx + (Math.random() * 2 - 1), y: point.cy + (Math.random() * 2 - 1), button: 'left', buttons: 1,
      }).catch(() => {});
    }
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x: point.cx, y: point.cy, button: 'left', buttons: 1, clickCount: 1 });
  }, { frameId: 0, at: [point.cx, point.cy] });
}

// -------------------------------------------------------------- verification

/**
 * Page-side proof. A human saying "done" is a hint; a non-empty token field or
 * a vanished widget is evidence. Returns a shape callers can quote verbatim.
 */
export async function verify(tabId, baseline = null) {
  // Top document only: every mainstream vendor writes its token into the TOP
  // page (g-recaptcha-response / h-captcha-response / cf-turnstile-response)
  // and the top DOM is also where a vanished widget iframe becomes visible.
  // Probing all frames every 500ms would turn a wait into a CPU fire.
  let top = null;
  try {
    top = await challengeCall(tabId, 'verify', {}, 0);
  } catch { /* sensor absent → fall back to tab URL below */ }
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  const url = top?.url || tab?.url || '';
  const title = top?.title || tab?.title || '';
  const token = !!top?.token_present;
  const visible = (top?.findings || []).filter((f) => f.kind !== 'score');
  return {
    response_filled: token,
    response_fields: (top?.tokens || []).filter((t) => t.length > 0).map((t) => ({ name: t.name, length: t.length })),
    still_present: visible.length > 0,
    findings: visible.slice(0, 10),
    url,
    title,
    url_changed: !!(baseline?.url && url && url !== baseline.url),
    title_changed: !!(baseline?.title && title && title !== baseline.title),
    doc_epoch: top?.epoch || null,
    epoch_changed: !!(baseline?.epoch && top?.epoch && top.epoch !== baseline.epoch),
    frames_alive: top ? 1 : 0,
  };
}

async function baselineOf(tabId) {
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  const ps = await challengeCall(tabId, 'pageState', {}, 0).catch(() => null);
  return { url: tab?.url || ps?.url || '', title: tab?.title || ps?.title || '', epoch: ps?.epoch || null };
}

// ------------------------------------------------------------------- solve

async function waitForChange(tabId, baseline, { timeout_ms = 9000 } = {}) {
  const t0 = now();
  let last = null;
  while (now() - t0 < timeout_ms) {
    last = await verify(tabId, baseline).catch(() => null);
    if (last) {
      if (last.response_filled) return { state: 'solved', verified: true, evidence: last, waited_ms: now() - t0 };
      // A navigation is only progress when it did not land on ANOTHER challenge
      // page: Cloudflare's own retry (`__cf_chl_rt_tk`) is a fresh document too,
      // and calling that "solved" is exactly the false positive this check ends.
      const stillChallenge = (last.findings || []).some((f) => f.kind !== 'score');
      // A challenge frame that navigated away entirely is proof the widget
      // advanced; the new page may carry no sensor findings at all.
      if (!stillChallenge && (last.epoch_changed || (!last.still_present && (last.url_changed || last.title_changed)))) {
        return { state: 'solved', verified: true, evidence: last, waited_ms: now() - t0 };
      }
    }
    await sleep(WAIT_POLL_MS);
  }
  return { state: 'timeout', verified: false, evidence: last, waited_ms: now() - t0 };
}

async function gridPayload(tabId, detection) {
  const hit = await mapInFrames(tabId, 'gridMap');
  if (!hit) return null;
  const { frameId, map } = hit;
  const tiles = [];
  for (const t of map.tiles || []) {
    const b = await toTop(tabId, frameId, t.box).catch(() => null);
    if (!b) continue;
    tiles.push({ i: t.i, ...boxOut(b) });
  }
  if (!tiles.length) return null;
  const gridBox = await toTop(tabId, frameId, map.grid_box).catch(() => map.grid_box);
  const submit = map.submit ? { label: map.submit.label, ...boxOut(await toTop(tabId, frameId, map.submit.box).catch(() => null)) } : null;
  const clip = padBox(gridBox, 10);
  const image = await shot(tabId, clip);
  pending.set(tabId, {
    at: now(), frameId, baseline: { url: detection?.frames?.find((f) => f.frame_id === 0)?.url || '', title: '' },
    tiles, submit, gridBox, prompt: map.prompt || '', vendor: map.vendor || detection?.vendor || '', round: 1,
  });
  return {
    state: 'needs_grid',
    vendor: map.vendor || detection?.vendor || 'unknown',
    prompt: map.prompt || '',
    grid: { rows: map.rows, cols: map.cols, cells: tiles.map((t) => t.i), frame_id: frameId },
    cells: tiles.map((t) => ({ i: t.i, x: t.x, y: t.y, w: t.width, h: t.height })),
    submit,
    round: 1,
    data_base64: image.data_base64,
    format: image.format,
    clip: image.clip,
    instruction: `Look at the image. Call challenge.pick {"cells":[indices...]} with 0-based row-major indices of the tiles that match the prompt${submit ? '; submit defaults to true' : ''}.`,
  };
}

async function canvasPayload(tabId, detection) {
  const hit = await mapInFrames(tabId, 'canvasMap');
  if (!hit) return null;
  const { frameId, map } = hit;
  const box = await toTop(tabId, frameId, map.canvas).catch(() => null);
  if (!box) return null;
  // hCaptcha paints its canvas asynchronously: a shot taken the instant the
  // element exists is a black rectangle. One settle beat is cheaper than a
  // wrong answer built on an empty image.
  await sleep(700);
  const image = await shot(tabId, padBox(box, 6));
  const submitBox = map.submit ? await toTop(tabId, frameId, map.submit).catch(() => null) : null;
  pending.set(tabId, {
    at: now(), kind: 'canvas', frameId, canvas: box, prompt: map.prompt || '',
    submit: submitBox,
    vendor: detection?.vendor || 'unknown', round: 1, baseline: await baselineOf(tabId),
  });
  return {
    state: 'needs_canvas',
    vendor: detection?.vendor || 'unknown',
    prompt: map.prompt || '',
    canvas: boxOut(box),
    submit: submitBox ? boxOut(submitBox) : null,
    round: 1,
    data_base64: image.data_base64,
    format: image.format,
    clip: image.clip,
    instruction: 'Look at the image and solve the visual puzzle. Coordinates are FRACTIONS of the canvas box: challenge.pick {"drag":{"from":[fx,fy],"to":[fx,fy]}} (0,0 = top-left, 1,1 = bottom-right).',
  };
}

async function sliderSolve(tabId, detection, { offset = null, timeout_ms = 12000 } = {}) {
  const hit = await mapInFrames(tabId, 'sliderMap');
  if (!hit) return { state: 'failed', detail: 'slider widget not found in any frame' };
  const { frameId, map } = hit;
  const handle = await toTop(tabId, frameId, map.handle).catch(() => null);
  if (!handle) return { state: 'failed', detail: 'slider handle has no composed box (frame pairing ambiguous)' };
  const track = map.track ? await toTop(tabId, frameId, map.track).catch(() => null) : null;
  // Distance to the END of the track, measured edge-to-edge: Aliyun's handle
  // overhangs its wrapper by ~20px, so `track.width - handle.width` alone
  // under-drags and the widget rejects the proof.
  const maxDx = track
    ? Math.max(8, track.x + track.width - handle.x - handle.width - 2)
    : 320;
  let dx = offset;
  let method = 'caller';
  let confidence = 1;
  let g = null;        // canvasGap result (method/offset/score) for debug
  let candidates = null; // per-method candidates from canvasGap, for auditing
  let gErr = null;     // canvasGap probe error text
  const top3Ref = { v: null };
  if (dx == null && (detection?.slide_to_end || map.to_end)) {
    // no gap exists: the challenge is "drag to the far right"
    dx = maxDx; method = 'slide-to-end'; confidence = 1;
  }
  if (dx == null) {
    // 1) try canvas pixels in the page (no CORS taint → exact template match)
    let gErr = null;
    g = await challengeCall(tabId, 'canvasGap', {}, frameId).catch((e) => { gErr = String(e?.message || e).slice(0, 200); return null; });
    if (!g || !Number.isFinite(g.offset)) {
      // widget may still be mid-redraw — one settle + retry
      await sleep(700);
      g = await challengeCall(tabId, 'canvasGap', {}, frameId).catch((e) => { gErr = String(e?.message || e).slice(0, 200); return null; });
    }
    if (g && Number.isFinite(g.offset)) {
      dx = g.offset; method = g.method; confidence = g.confidence;
      top3Ref.v = g.outline_top3 || null;
      candidates = g.candidates || null;
    }
  }
  let image = null;
  if (dx == null) {
    // 2) screenshot the widget, decode it in the page, find the punched hole
    const parts = [map.handle, map.track, map.bg, map.slice].filter(Boolean);
    let clip = parts.reduce((a, b) => union(a, b), null);
    if (!clip) return { state: 'failed', detail: 'slider geometry missing' };
    clip = padBox(clip, 14);
    const s = await shot(tabId, clip);
    image = s;
    const analysis = await challengeCall(tabId, 'analyzeImage', {
      data_b64: s.data_base64,
      clip,
      piece_box: map.slice || map.handle,
      track_box: map.bg || map.track,
      handle_w: map.handle ? map.handle.width : 40,
    }, frameId).catch(() => null);
    if (analysis?.ok) {
      dx = analysis.offset_css; method = analysis.method; confidence = analysis.confidence;
      if (map.slice && map.bg) {
        // offset is relative to the band start; correct for where the piece sits
        dx = Math.max(0, Math.round(analysis.gap_x_in_band - (map.slice.x - map.bg.x)));
      }
    }
    if (dx == null) {
      pending.set(tabId, { at: now(), kind: 'slider', frameId, vendor: detection?.vendor || 'geetest', round: 1, baseline: { url: (await chrome.tabs.get(tabId).catch(() => null))?.url || '' } });
      return {
        state: 'needs_offset', vendor: detection?.vendor || 'geetest',
        data_base64: image.data_base64, format: image.format, clip: image.clip,
        detail: 'automatic gap detection found no confident candidate',
        canvas_gap_dbg: { g: g ? { method: g.method, offset: g.offset, score: g.score, margin: g.margin } : null, g2: (typeof g2 !== 'undefined' && g2) ? { method: g2.method, offset: g2.offset } : null, gErr },
        instruction: 'Read the image: pass the horizontal distance from the piece\'s current left edge to the gap, in CSS px, as challenge.pick {"offset": N}.',
        geometry: { handle: boxOut(handle), track: map.track ? boxOut(map.track) : null, bg: map.bg ? boxOut(map.bg) : null, slice: map.slice ? boxOut(map.slice) : null },
      };
    }
  }
  // RETRY ON FAILED VERIFY: the widget swaps to a new image after a rejected
  // drag, so each retry recomputes on fresh pixels. Live soak: single-attempt
  // hit rate ~50%; three attempts ≈87%, five ≈97%. Only retries when the
  // caller did NOT pin an explicit offset (an explicit offset means the human
  // asked for exactly that value).
  let travel = Math.round(Math.min(maxDx, Math.max(4, dx)));
  const base = await baselineOf(tabId);
  await guardVisible(tabId, 'challenge.drag');
  await withPass(tabId, async () => {
    await human.humanMoveTo(tabId, handle.cx, handle.cy);
    await sleep(90 + Math.random() * 140);
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x: handle.cx, y: handle.cy, button: 'left', buttons: 1, clickCount: 1 });
    await human.humanDragPath(tabId, { x: handle.cx, y: handle.cy }, travel, { duration_ms: 700 + Math.random() * 420 });
    await sleep(120 + Math.random() * 160);
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x: handle.cx + travel, y: handle.cy, button: 'left', buttons: 1, clickCount: 1 });
  }, { frameId, at: [handle.cx, handle.cy] });
  // slidercaptcha (campus SSO) proves itself inside the widget: the hidden
  // sliderCaptchaDynamicCode gets a value and/or the label turns into success
  // BEFORE the page submits. Poll it first; fall back to page navigation.
  if (detection?.vendor === 'slidercaptcha') {
    const t0b = now();
    let st = null;
    while (now() - t0b < Math.min(timeout_ms, 6000)) {
      st = await challengeCall(tabId, 'slidercaptchaState', {}, frameId).catch(() => null);
      if (st && (st.code_len > 0 || st.success)) {
        return {
          state: 'solved', vendor: 'slidercaptcha', method, confidence,
          dragged: travel, verified: true, evidence: { slidercaptcha: st },
          waited_ms: now() - t0b,
        };
      }
      await sleep(300);
    }
  }
  const after = await waitForChange(tabId, base, { timeout_ms });
  const firstAttempt = {
    state: after.state === 'solved' ? 'solved' : after.state,
    vendor: detection?.vendor || 'geetest',
    method, confidence,
    dragged: travel,
    outline_top3: top3Ref.v,
    candidates,
    verified: after.verified,
    evidence: after.evidence,
    waited_ms: after.waited_ms,
    ...(image ? { data_base64: image.data_base64, format: image.format, clip: image.clip } : {}),
  };
  return firstAttempt;
}

async function checkboxSolve(tabId, detection, { timeout_ms = 9000 } = {}) {
  const target = detection.targets.find((t) => t.role === 'checkbox' && !t.unclickable);
  if (!target) {
    return {
      state: 'failed',
      vendor: detection.vendor,
      detail: 'a checkbox challenge was detected but no clickable point was composed (sandboxed iframe, ambiguous frame pairing, or the widget is hidden)',
      targets: detection.targets.slice(0, 4),
      next: 'take a screenshot and click the checkbox by coordinates, or finish it with mode:"human"',
    };
  }
  const base = await baselineOf(tabId);
  await humanClickAt(tabId, target, { frameId: target.frame_id, role: 'checkbox' });
  // A v2 checkbox either fills the token (low-risk pass) or opens bframe. Give
  // the page a moment, then re-detect: the grid is a NEW state, not a failure.
  const t0 = now();
  let det = detection;
  while (now() - t0 < timeout_ms) {
    await sleep(WAIT_POLL_MS);
    det = await detect(tabId).catch(() => det);
    if (det.response_filled) {
      return { state: 'solved', vendor: detection.vendor || det.vendor, verified: true, evidence: { response_filled: true, response_fields: det.response_fields }, waited_ms: now() - t0, clicked: target };
    }
    if (!det.challenge) {
      return { state: 'solved', vendor: detection.vendor || det.vendor, verified: true, evidence: { challenge_gone: true }, waited_ms: now() - t0, clicked: target };
    }
    if (det.kind === 'grid' || det.kind === 'canvas') {
      const payload = det.kind === 'canvas' ? await canvasPayload(tabId, det) : await gridPayload(tabId, det);
      if (payload) return { ...payload, clicked: target, waited_ms: now() - t0 };
    }
  }
  return {
    state: 'failed',
    vendor: detection.vendor || det.vendor,
    verified: false,
    clicked: target,
    evidence: { still_present: true, response_filled: det.response_filled, kind: det.kind },
    waited_ms: now() - t0,
    next: 'the click landed but the challenge stayed. If a grid opened, call challenge.solve again (it returns the grid). Otherwise try mode:"human".',
  };
}

async function interstitialSolve(tabId, detection, { timeout_ms = 20000 } = {}) {
  const base = await baselineOf(tabId);
  // Cloudflare's interstitial sometimes auto-clears; clicking the checkbox the
  // instant the page loads can reset it. Wait for the widget to be drawn, then
  // click like a person. A failed verification comes back as a NEW document
  // with the same checkbox (new Ray ID); that retry is not success and it is
  // also not the end — click again, at most 3 attempts.
  const t0 = now();
  let clicks = 0;
  let lastClickAt = 0;
  let clickEpoch = null;
  const MAX_CLICKS = 3;
  while (now() - t0 < timeout_ms) {
    await sleep(700);
    const v = await verify(tabId, base).catch(() => null);
    const stillChallenge = (v?.findings || []).some((f) => f.kind !== 'score');
    const advanced = !!v && (v.url_changed || v.title_changed || v.epoch_changed);
    // "no challenge findings" alone is not progress — the top probe can miss
    // text for a beat while the interstitial boots. Require the page to have
    // actually moved, or a token to exist.
    if (v && (v.response_filled || (!v.still_present && !stillChallenge && advanced))) {
      return {
        state: 'solved', vendor: detection.vendor, verified: true,
        evidence: { url_changed: v.url_changed, title_changed: v.title_changed, findings: v.findings, response_filled: v.response_filled },
        waited_ms: now() - t0, clicks,
      };
    }
    const det = await detect(tabId).catch(() => detection);
    if (!det.challenge) return { state: 'solved', vendor: detection.vendor, verified: true, evidence: { challenge_gone: true }, waited_ms: now() - t0, clicks };
    // first attempt: after the waiting-room had a chance to clear on its own;
    // retries: only after the page actually reset to a new document
    const retried = clickEpoch != null && v?.doc_epoch && v.doc_epoch !== clickEpoch;
    const ready = clicks === 0 ? now() - t0 > 3500 : retried && now() - lastClickAt > 2500;
    if (clicks < MAX_CLICKS && ready) {
      const t = det.targets.find((x) => x.role === 'checkbox' && !x.unclickable);
      if (t) {
        // pointer presence before touching the widget; on later attempts this
        // also keeps the session looking alive through the verification spinner
        await human.warmup(tabId, { ms: clicks === 0 ? 700 : 1200 }).catch(() => null);
        await humanClickAt(tabId, t, { frameId: t.frame_id, role: 'checkbox' }).catch(() => {});
        clicks += 1;
        lastClickAt = now();
        clickEpoch = v?.doc_epoch || null;
      }
    }
  }
  const v = await verify(tabId, base).catch(() => null);
  return {
    state: 'timeout',
    vendor: detection.vendor,
    verified: false,
    evidence: v,
    waited_ms: now() - t0,
    clicks,
    next: clicks
      ? 'the checkbox was clicked and the widget kept resetting. Do not reload; try challenge.solve again with mode:"human", or let a person click it.'
      : 'the waiting-room never produced a clickable checkbox. If this is Cloudflare, the widget may be a variant this build refuses to guess at (non-300x65); use mode:"human".',
  };
}

async function textPayload(tabId, detection) {
  const hit = await mapInFrames(tabId, 'textMap');
  if (!hit) return null;
  const { frameId, map } = hit;
  const img = await toTop(tabId, frameId, map.img_box).catch(() => null);
  const input = await toTop(tabId, frameId, map.input_box).catch(() => null);
  const submit = map.submit ? await toTop(tabId, frameId, map.submit).catch(() => null) : null;
  if (!img || !input) return null;
  const clip = padBox(union(img, map.input_box ? img : input), 8);
  const image = await shot(tabId, clip);
  pending.set(tabId, { at: now(), frameId, kind: 'text', input, submit, img, baseline: { url: (await chrome.tabs.get(tabId).catch(() => null))?.url || '' } });
  return {
    state: 'needs_text',
    vendor: detection?.vendor || 'generic',
    data_base64: image.data_base64,
    format: image.format,
    clip: image.clip,
    image_box: boxOut(img),
    input: boxOut(input),
    submit: submit ? boxOut(submit) : null,
    instruction: 'Read the image and call challenge.pick {"answer":"...","confirm":true}. The answer is typed with a human cadence into the captcha field only.',
  };
}

/**
 * solve(tabId, {mode}) — the one-call path.
 * modes: auto (default) | checkbox | slider | grid | text | hold | human | wait
 */
export async function solve(tabId, opts = {}) {
  const mode = ['auto', 'checkbox', 'slider', 'grid', 'canvas', 'text', 'hold', 'human', 'wait'].includes(opts.mode) ? opts.mode : 'auto';
  const t0 = now();
  const detection = await detect(tabId, { deep: true });
  if (detection.viewport_zero && mode !== 'wait' && mode !== 'human') {
    throw err('VIEWPORT_ZERO', 'the tab viewport is 0x0 (window minimized or created off-screen): every element box collapses, so a drag/click would be geometry on a broken layout. Restore the window first — `window.restore {tab_id}` or `dev.focus {confirm:true,restore_window:true}` (explicit opt-in: it un-minimizes the window) — then call challenge.solve again. Nothing was clicked.', {
      tab_id: tabId, viewport: detection.viewport,
    });
  }

  if (mode === 'human') {
    const r = await runHelp(tabId, opts.prompt || `请完成人机验证（${detection.vendor || 'captcha'}）`, [], Math.min(Number(opts.timeout_ms) || 25000, 25000), null, true);
    const v = await verify(tabId, await baselineOf(tabId)).catch(() => null);
    return {
      state: r.outcome === 'completed' ? 'solved' : r.outcome,
      mode: 'human',
      verified: !!v?.response_filled,
      human: r,
      evidence: v,
      detail: v?.response_filled ? 'a token exists after the human step' : 'the human step ended; page-side evidence is in evidence — do not treat "完成" alone as proof',
      detection,
    };
  }

  if (!detection.challenge && mode === 'auto' && detection.kind !== 'score') {
    if (detection.response_filled) {
      return { state: 'solved', vendor: detection.vendor, verified: true, evidence: { response_filled: true }, detection, elapsed_ms: now() - t0 };
    }
    return { state: 'absent', detail: 'no human-verification widget on this page (top frame + vendor frames probed)', detection, elapsed_ms: now() - t0 };
  }

  if (mode === 'wait') {
    const w = await waitForChange(tabId, await baselineOf(tabId), { timeout_ms: Number(opts.timeout_ms) || 9000 });
    return { ...w, vendor: detection.vendor, detection };
  }

  if (mode === 'auto' && detection.kind === 'score') {
    const w = await waitForChange(tabId, await baselineOf(tabId), { timeout_ms: Math.min(Number(opts.timeout_ms) || 2500, 5000) });
    return {
      state: w.state === 'solved' ? 'solved' : 'passive',
      vendor: detection.vendor, verified: w.state === 'solved',
      detail: 'score-based challenge (invisible reCAPTCHA v3 / Turnstile): no click exists; the page scores the session on its own. Continue the task — do not reload to "retry".',
      evidence: w.evidence, detection,
    };
  }

  // human-like presence before touching anything: a cold pointer that lands on
  // the widget 50ms after load is the single easiest behavioural tell
  if (opts.warmup !== false && ['auto', 'checkbox', 'slider', 'hold'].includes(mode)) {
    await human.warmup(tabId, { ms: Math.min(Number(opts.warmup_ms) || 1400, 5000) }).catch(() => null);
  }

  if (mode === 'grid' || (mode === 'auto' && detection.kind === 'grid')) {
    const p = await gridPayload(tabId, detection);
    return p || { state: 'failed', detail: 'grid challenge reported but no tile geometry could be read', detection };
  }
  if (mode === 'canvas' || (mode === 'auto' && detection.kind === 'canvas')) {
    const p = await canvasPayload(tabId, detection);
    return p || { state: 'failed', detail: 'canvas challenge reported but its box could not be composed (iframes/protected canvas)', detection };
  }
  if (mode === 'text' || (mode === 'auto' && detection.kind === 'text')) {
    const p = await textPayload(tabId, detection);
    return p || { state: 'failed', detail: 'text captcha reported but its image/input pair could not be composed', detection };
  }
  if (mode === 'slider' || (mode === 'auto' && detection.kind === 'slider')) {
    // The widget re-renders its canvases right after popping (or after a
    // try-again reset) — an immediate canvasGap getImageData races the redraw
    // and returns empty, silently dropping the solve to the unreliable
    // screenshot path (yjsgll soak, round 1 of every batch). Settle first.
    await sleep(1200);
    const out = await sliderSolve(tabId, detection, { offset: opts.offset, timeout_ms: Number(opts.timeout_ms) || 12000 });
    return { ...out, detection };
  }
  if (mode === 'hold' || (mode === 'auto' && detection.kind === 'hold')) {
    const target = detection.targets.find((t) => t.role === 'hold' && !t.unclickable);
    if (!target) return { state: 'failed', detail: 'hold challenge has no clickable point', detection };
    await humanHoldAt(tabId, target, Number(opts.hold_ms) || 3200);
    const w = await waitForChange(tabId, await baselineOf(tabId), { timeout_ms: Number(opts.timeout_ms) || 8000 });
    return { ...w, vendor: detection.vendor, clicked: target, detection };
  }
  if (mode === 'auto' && detection.kind === 'interstitial') {
    return { ...(await interstitialSolve(tabId, detection, { timeout_ms: Number(opts.timeout_ms) || 20000 })), detection };
  }
  if (mode === 'checkbox' || detection.kind === 'checkbox' || detection.kind === 'unknown') {
    return { ...(await checkboxSolve(tabId, detection, { timeout_ms: Number(opts.timeout_ms) || 9000 })), detection };
  }
  return { state: 'unavailable', detail: `no solver for kind "${detection.kind}"`, detection };
}

// ------------------------------------------------------------------- pick

export function pendingOf(tabId) {
  const p = pending.get(tabId);
  if (!p) return null;
  if (now() - p.at > 5 * 60_000) { pending.delete(tabId); return null; }
  return { kind: p.kind || 'grid', round: p.round || 1, frameId: p.frameId, tiles: p.tiles || null, canvas: p.canvas || null, at: p.at };
}

/**
 * challenge.pick — act on a pending grid / text / slider payload.
 * Grid cells are 0-based row-major indices. Text answers require confirm:true
 * and only ever reach the detected captcha field (never an OTP input).
 */
export async function pick(tabId, p = {}) {
  const st = pending.get(tabId);
  if (!st) {
    return { state: 'no_pending', detail: 'no challenge.pick session on this tab (SW restarted, or challenge.solve has not returned needs_grid/needs_text). Call challenge.solve again.' };
  }

  if (st.kind === 'text') {
    if (p.confirm !== true) {
      throw err('NEEDS_CONFIRM', 'typing a captcha answer is a deliberate exception to the "验证码字段 never auto-filled" rule — retry with confirm:true after deciding this is a captcha (not an OTP).');
    }
    const answer = String(p.answer ?? '');
    if (!answer) throw err('BAD_PARAMS', 'answer is required for a text captcha');
    if (answer.length > 64) throw err('BAD_PARAMS', 'answer looks too long for a captcha');
    // focus the detected input with a REAL click, then type with cadence
    await guardVisible(tabId, 'challenge.answer');
    const target = { cx: st.input.cx, cy: st.input.cy };
    await humanClickAt(tabId, { ...st.input, ...target }, { frameId: st.frameId, role: 'text-input' });
    await sleep(120);
    // The trusted click focuses the field in a normal document. If the shield's
    // own host layer was mid-transition (or the vendor re-rendered the widget
    // between screenshot and click), focus can land elsewhere; the answer must
    // still go into the field the DETECTOR found. Focus by point, never by a
    // guessed selector: the rect is the one the screenshot was taken from.
    const focusInfo = await evalJs(tabId, `(() => {
      const cx=${Number(st.input.cx)}, cy=${Number(st.input.cy)};
      const els=[...document.querySelectorAll('input,textarea')];
      const hit=els.find((e)=>{const r=e.getBoundingClientRect();return cx>=r.left&&cx<=r.right&&cy>=r.top&&cy<=r.bottom;});
      const before=document.activeElement&&(document.activeElement.id||document.activeElement.tagName);
      if(hit){hit.focus();if(typeof hit.setSelectionRange==='function')hit.setSelectionRange(0,(hit.value||'').length);}
      return {before, focused: hit ? (hit.id||hit.tagName) : null, same: document.activeElement===hit};
    })()`).catch(() => null);
    // select existing text through page JS (ctrl+A is unreliable on some inputs)
    await evalJs(tabId, `(()=>{const a=document.activeElement;if(a&&typeof a.setSelectionRange==='function'){a.setSelectionRange(0,(a.value||'').length)}return true})()`).catch(() => {});
    // Keys must travel inside a pass: the shield's capture layer eats keydown
    // outside one (the same reason fill() wraps typing in withPass{keys:true}).
    const humanOn = await human.isEnabled().catch(() => false);
    await withPass(tabId, async (gate) => {
      if (humanOn) await human.humanType(tabId, answer);
      else {
        await gate.check(); // raw insertText proves itself
        await cdp.send(tabId, 'Input.insertText', { text: answer });
      }
    }, { frameId: st.frameId, keys: true, at: [st.input.cx, st.input.cy] });
    await sleep(180 + Math.random() * 220);
    let submitted = false;
    if (st.submit) {
      await humanClickAt(tabId, { ...st.submit, cx: st.submit.x + st.submit.width / 2, cy: st.submit.y + st.submit.height / 2 }, { frameId: st.frameId, role: 'submit' });
      submitted = true;
    } else {
      await cdp.send(tabId, 'Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' });
      await cdp.send(tabId, 'Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
      submitted = true;
    }
    const w = await waitForChange(tabId, st.baseline || {}, { timeout_ms: Number(p.timeout_ms) || 8000 });
    audit.log({ action: 'challenge.pick', tabId, kind: 'text', submitted, state: w.state });
    return {
      state: w.state === 'solved' ? 'solved' : 'submitted',
      verified: w.state === 'solved',
      submitted,
      focus: focusInfo,
      typed_chars: answer.length,
      evidence: w.evidence,
      note: w.state === 'solved' ? 'page-side evidence changed after the answer' : 'answer submitted; the page shows no success signal yet — do not resend blindly',
    };
  }

  if (st.kind === 'slider' || (p.offset != null && st.frameId != null && !st.tiles)) {
    return sliderSolve(tabId, { vendor: st.vendor }, { offset: Number(p.offset), timeout_ms: Number(p.timeout_ms) || 12000 });
  }

  if (st.kind === 'canvas') {
    const frac = p.drag && Array.isArray(p.drag.from) && Array.isArray(p.drag.to) ? p.drag : null;
    if (!frac) throw err('BAD_PARAMS', 'this is a canvas challenge: pass {"drag":{"from":[fx,fy],"to":[fx,fy]}} with fractions of the canvas box (0..1)');
    const clamp = (v) => Math.max(0, Math.min(1, Number(v)));
    const nums = [frac.from[0], frac.from[1], frac.to[0], frac.to[1]].map(Number);
    if (!nums.every(Number.isFinite)) throw err('BAD_PARAMS', 'drag.from / drag.to must be two finite numbers each ([fx,fy], 0..1)');
    const fx = [clamp(nums[0]), clamp(nums[1])];
    const tx = [clamp(nums[2]), clamp(nums[3])];
    const from = { x: st.canvas.x + st.canvas.width * fx[0], y: st.canvas.y + st.canvas.height * fx[1] };
    const to = { x: st.canvas.x + st.canvas.width * tx[0], y: st.canvas.y + st.canvas.height * tx[1] };
    await guardVisible(tabId, 'challenge.drag');
    await withPass(tabId, async () => {
      await human.humanMoveTo(tabId, from.x, from.y);
      await sleep(80 + Math.random() * 120);
      await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x: from.x, y: from.y, button: 'left', buttons: 1, clickCount: 1 });
      await sleep(120 + Math.random() * 160);
      await human.humanDragTo(tabId, from, to, { duration_ms: 700 + Math.random() * 500 });
      await sleep(150 + Math.random() * 180);
      await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x: to.x, y: to.y, button: 'left', buttons: 1, clickCount: 1 });
    }, { frameId: st.frameId, at: [from.x, from.y] });
    // A drop sometimes leaves the piece "in hand" (the widget only commits on a
    // second pointerdown). Check for the vendor's Verify button first; if it is
    // there, click it; if not, click the target once to commit, then look again.
    await sleep(260);
    let submitBox = null;
    const findSubmit = async () => {
      const m = await mapInFrames(tabId, 'canvasMap').catch(() => null);
      if (!m?.map?.submit) return null;
      return toTop(tabId, m.frameId, m.map.submit).catch(() => null);
    };
    submitBox = await findSubmit();
    if (!submitBox) {
      await humanClickAt(tabId, { x: to.x - 5, y: to.y - 5, width: 10, height: 10, cx: to.x, cy: to.y }, { frameId: st.frameId, role: 'canvas-drop' });
      // the Verify control is rendered a beat after the drop, not in the same frame
      for (let i = 0; i < 6 && !submitBox; i++) {
        await sleep(300);
        submitBox = await findSubmit();
      }
    }
    if (submitBox) {
      await humanClickAt(tabId, { x: submitBox.x, y: submitBox.y, width: submitBox.width, height: submitBox.height, cx: submitBox.cx, cy: submitBox.cy }, { frameId: st.frameId, role: 'canvas-submit' });
    }
    const w = await waitForChange(tabId, st.baseline || {}, { timeout_ms: Number(p.timeout_ms) || 9000 });
    if (w.state === 'solved') {
      pending.delete(tabId);
      audit.log({ action: 'challenge.pick', tabId, kind: 'canvas', state: 'solved' });
      return { state: 'solved', verified: true, drag: { from: fx, to: tx }, evidence: w.evidence, round: st.round };
    }
    const det = await detect(tabId).catch(() => null);
    if (det?.challenge && det.kind === 'canvas') {
      const payload = await canvasPayload(tabId, det);
      if (payload) {
        audit.log({ action: 'challenge.pick', tabId, kind: 'canvas', state: 'needs_canvas' });
        return { ...payload, previous_drag: { from: fx, to: tx }, round: (st.round || 1) + 1, note: 'the challenge stayed or advanced; read the new screenshot' };
      }
    }
    audit.log({ action: 'challenge.pick', tabId, kind: 'canvas', state: 'failed' });
    return { state: 'wrong_or_unchanged', verified: false, drag: { from: fx, to: tx }, evidence: w.evidence, next: 'take a fresh screenshot and try a different item/target; do not repeat blindly' };
  }

  // grid
  const cells = Array.isArray(p.cells) ? p.cells.map((n) => Number(n)).filter((n) => Number.isInteger(n)) : [];
  if (!cells.length) throw err('BAD_PARAMS', 'cells must be a non-empty array of 0-based row-major tile indices');
  const byIndex = new Map((st.tiles || []).map((t) => [t.i, t]));
  const chosen = cells.map((i) => byIndex.get(i)).filter(Boolean);
  if (chosen.length !== cells.length) {
    return { state: 'bad_cells', detail: `cell index out of range; this round has ${st.tiles.length} tiles (0..${st.tiles.length - 1})`, cells: st.tiles.map((t) => t.i) };
  }
  for (const t of chosen) {
    await humanClickAt(tabId, { cx: t.cx, cy: t.cy, x: t.x, y: t.y, width: t.width, height: t.height }, { frameId: st.frameId, role: 'grid-cell' });
    await sleep(140 + Math.random() * 180);
  }
  await sleep(250 + Math.random() * 250);
  if (p.submit !== false && st.submit) {
    await humanClickAt(tabId, { cx: st.submit.cx, cy: st.submit.cy, x: st.submit.x, y: st.submit.y, width: st.submit.width, height: st.submit.height }, { frameId: st.frameId, role: 'submit' });
  }
  const w = await waitForChange(tabId, st.baseline || {}, { timeout_ms: Number(p.timeout_ms) || 9000 });
  if (w.state === 'solved') {
    pending.delete(tabId);
    audit.log({ action: 'challenge.pick', tabId, kind: 'grid', cells, state: 'solved' });
    return { state: 'solved', verified: true, cells, submitted: p.submit !== false, evidence: w.evidence, round: st.round };
  }
  // still there: either a new round (new tiles / prompt) or a wrong answer
  const det = await detect(tabId).catch(() => null);
  if (det?.challenge && det.kind === 'grid') {
    const payload = await gridPayload(tabId, det);
    if (payload) {
      audit.log({ action: 'challenge.pick', tabId, kind: 'grid', cells, state: 'needs_grid' });
      return { ...payload, previous_cells: cells, round: (st.round || 1) + 1, note: 'the widget advanced to another round' };
    }
  }
  if (det && !det.challenge) {
    pending.delete(tabId);
    return { state: 'solved', verified: true, cells, evidence: w.evidence, note: 'challenge disappeared after the answer' };
  }
  audit.log({ action: 'challenge.pick', tabId, kind: 'grid', cells, state: 'failed' });
  return {
    state: 'wrong_or_unchanged',
    verified: false,
    cells,
    evidence: w.evidence,
    next: 'take a fresh challenge.solve screenshot and pick again; do not repeat the same cells blindly',
  };
}

// ------------------------------------------------------------------ warmup

export async function warmup(tabId, opts = {}) {
  return human.warmup(tabId, opts);
}

export async function domDumpOf(tabId, frameId = 0, max = 140) {
  return challengeCall(tabId, 'domDump', { max }, frameId);
}

/** Generic content-challenge op caller (casFormState, slidercaptchaState, …). */
export async function callOp(tabId, op, args = {}, frameId = 0) {
  return challengeCall(tabId, op, args, frameId);
}

// ------------------------------------------------------------------- audit

/**
 * Read-only automation-leak audit. This is the honest counterweight to "just
 * click like a human": if the page can tell it is CDP, no mouse path saves you.
 */
export async function audit_environment(tabId) {
  const expr = `(() => {
    const out = { webdriver: navigator.webdriver, ua: navigator.userAgent, platform: navigator.platform,
      languages: navigator.languages, plugins: navigator.plugins ? navigator.plugins.length : -1,
      hardwareConcurrency: navigator.hardwareConcurrency, deviceMemory: navigator.deviceMemory || null,
      chrome_runtime: !!(window.chrome && window.chrome.runtime), chrome_obj: !!window.chrome,
      viewport: [innerWidth, innerHeight], outer: [outerWidth, outerHeight], dpr: devicePixelRatio,
      cdp_artifacts: [], webgl: null, notification: (typeof Notification !== 'undefined') ? Notification.permission : null };
    for (const k of Object.keys(window)) {
      if (/^(__playwright|__puppeteer|__selenium|cdc_|_Selenium|domAutomation|__nightmare)/.test(k)) out.cdp_artifacts.push(k);
    }
    for (const k of Object.keys(document)) {
      if (/^\\$?cdc_|^__webdriver|^__driver/.test(k)) out.cdp_artifacts.push('document.' + k);
    }
    try {
      const c = document.createElement('canvas');
      const gl = c.getContext('webgl') || c.getContext('experimental-webgl');
      if (gl) {
        const dbg = gl.getExtension('WEBGL_debug_renderer_info');
        out.webgl = dbg ? { vendor: gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL), renderer: gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) } : { vendor: 'masked', renderer: 'masked' };
      }
    } catch (e) { out.webgl = { error: String(e).slice(0, 80) }; }
    return out;
  })()`;
  const env = await evalJs(tabId, expr).catch((e) => ({ error: String(e?.message || e) }));
  const risks = [];
  if (env.webdriver === true) risks.push({ level: 'high', code: 'NAVIGATOR_WEBDRIVER', detail: 'navigator.webdriver is true — every behavioural score is already lost; this build is driving a WebDriver-controlled browser, not the user profile' });
  if (env.cdp_artifacts && env.cdp_artifacts.length) risks.push({ level: 'high', code: 'CDP_ARTIFACTS', detail: `automation globals present: ${env.cdp_artifacts.join(', ')}` });
  if (env.chrome_runtime === false) risks.push({ level: 'medium', code: 'NO_EXTENSION_CONTEXT', detail: 'window.chrome.runtime is missing in the page main world — normal for a page, but some builds leak it when an automation framework injects scripts' });
  if (!env.webgl || env.webgl.renderer === 'masked') risks.push({ level: 'low', code: 'WEBGL_MASKED', detail: 'WebGL renderer is masked; a real profile usually reports a concrete GPU — not a blocker, just less "normal"' });
  const visible = await evalJs(tabId, 'document.visibilityState').catch(() => null);
  if (visible !== 'visible') risks.push({ level: 'medium', code: 'DOCUMENT_HIDDEN', detail: `document.visibilityState is ${JSON.stringify(visible)} — trusted input is refused before dispatch and behavioural timing looks unnatural` });
  return {
    environment: env,
    risks,
    verdict: risks.some((r) => r.level === 'high') ? 'blocked' : risks.some((r) => r.level === 'medium') ? 'risky' : 'clean',
    note: 'UBB drives the user\'s real profile through chrome.debugger; no fingerprint patches are installed by this extension. If verdict is blocked, fix the environment before blaming the challenge solver.',
  };
}
