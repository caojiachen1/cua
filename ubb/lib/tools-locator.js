// locator.* — Playwright-style semantic element tools (role/text/label/test_id
// composition) over the locator-core matcher in every frame. Actions reuse the
// REAL input path wherever the browser can receive it (top document and
// same-process frames get delivery-proved coordinate input; out-of-process
// frames fall back to the bridge's synthetic DOM click, labeled as such), and
// refuse ambiguity instead of guessing.

import { err } from './config.js';
import { cdp } from './cdp.js';
import { targetTab, mouse, key, modifierBits, DANGEROUS_LABEL } from './actions.js';
import { withPass, sleep } from './shield-ctl.js';
import { bridge } from './bridge-ctl.js';
import { frameElementBox, frameIsOutOfProcess, listExtFrames } from './snapshot.js';
import { audit } from './audit.js';
import { normalizeLocator, resolveLocator } from './locator-resolve.js';
// one-way import: tools.js never imports this module, so the shared
// delivery-proof helpers stay single-source (pointerProof/deliveredField/
// guardInputVisibility are exported there for exactly this consumer)
import { pointerProof, deliveredField, guardInputVisibility } from './tools.js';

const MAIN_FRAME = 0;

async function describeTarget(tabId, target, { scroll = true } = {}) {
  if (target.frameId === MAIN_FRAME) {
    const d = await bridge(tabId, 'describeRef', { ref: target.match.ref, scroll });
    return { desc: d, box: d && d.local ? { cx: d.local.x + d.local.width / 2, cy: d.local.y + d.local.height / 2 } : null };
  }
  const r = await frameElementBox(tabId, { frameId: target.frameId, domRef: target.match.ref }, { scroll });
  return { desc: r.desc, box: r.box, scrolled: r.scrolled };
}

/** The real-input click used by locator.click / locator.press on non-OOPIF
 *  targets — the same proof chain as the coordinate click: visibility gate,
 *  shield pass, delivery record. */
async function realClick(tabId, target, { double = false, button = 'left', label = '' } = {}) {
  await describeTarget(tabId, target); // scrollIntoView first, then re-sample
  const { box } = await describeTarget(tabId, target);
  if (!box || !Number.isFinite(box.cx)) throw err('ELEMENT_NOT_VISIBLE', 'the matched element has no visible box');
  // fix19 parity with the coordinate click: the document that would receive
  // this input is asked BEFORE dispatch — a hidden one is refused in well
  // under a second instead of burning the proof budget on a dead queue.
  await guardInputVisibility(tabId, { action: 'locator.click', ref: label || target.ref, frameId: target.frameId });
  const dl = {};
  await withPass(
    tabId,
    async () => {
      await mouse(tabId, 'mouseMoved', box.cx, box.cy);
      await mouse(tabId, 'mousePressed', box.cx, box.cy, { clickCount: double ? 2 : 1, button });
      await sleep(30);
      await mouse(tabId, 'mouseReleased', box.cx, box.cy, { clickCount: double ? 2 : 1, button });
    },
    { frameId: target.frameId, at: [box.cx, box.cy], delivery: dl, expect: pointerProof([Math.round(box.cx), Math.round(box.cy)]) },
  );
  return { box, delivered: deliveredField(dl), delivery_ms: typeof dl.ms === 'number' ? dl.ms : null };
}

/** Last-resort click through the pierced DOM (closed shadow roots included)
 *  when the bridge locatorQuery itself fails. Honest: this is coordinate
 *  input at the matched node's box; it does not make the node addressable by
 *  later ref tools. Match by role/tag + text only. */
async function piercedClick(tabId, spec) {
  const text = spec.text || spec.text_exact || spec.label || spec.placeholder || '';
  const role = spec.role || '';
  if (!text && !role) return null;
  const flat = await cdp.send(tabId, 'DOM.getFlattenedDocument', { depth: -1, pierce: true });
  const roleTag = { button: 'BUTTON', link: 'A', textbox: 'INPUT', checkbox: 'INPUT', combobox: 'SELECT' }[role] || null;
  let fetches = 0;
  for (const n of flat?.nodes || []) {
    if (!n.nodeId || !n.nodeName || n.nodeName.startsWith('#')) continue;
    if (roleTag && n.nodeName !== roleTag) continue;
    if (!text && !roleTag) continue;
    if (fetches++ > 250) break;
    const html = await cdp.send(tabId, 'DOM.getOuterHTML', { nodeId: n.nodeId }).catch(() => null);
    if (!html?.outerHTML) continue;
    if (text) {
      const txt = html.outerHTML.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').toLowerCase();
      if (!txt.includes(String(text).toLowerCase())) continue;
    }
    const bm = await cdp.send(tabId, 'DOM.getBoxModel', { nodeId: n.nodeId }).catch(() => null);
    const q = bm?.model?.border;
    if (!q || q.length < 8) continue;
    const xs = [q[0], q[2], q[4], q[6]], ys = [q[1], q[3], q[5], q[7]];
    const x = Math.min(...xs), y = Math.min(...ys);
    const w = Math.max(...xs) - x, h = Math.max(...ys) - y;
    if (w < 2 || h < 2) continue;
    const cx = x + w / 2, cy = y + h / 2;
    await guardInputVisibility(tabId, { action: 'locator.click', ref: 'pierced', frameId: 0 });
    await withPass(tabId, async () => {
      await mouse(tabId, 'mouseMoved', cx, cy);
      await mouse(tabId, 'mousePressed', cx, cy, { clickCount: 1, button: 'left' });
      await sleep(30);
      await mouse(tabId, 'mouseReleased', cx, cy, { clickCount: 1, button: 'left' });
    }, { at: [cx, cy] });
    return { cx, cy, text: html.outerHTML.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 80) };
  }
  return null;
}

export function register(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('locator.count', { category: 'sensing', description: 'Count elements matching a Playwright-style locator: {role:"button", text:"提交"} — keys compose (css, role, text, label, placeholder, test_id, alt, value, has_text, has_not_text, checked, disabled, visible, nth). The main document is searched first; when it has no match every frame with a bridge is swept, so the count is page-wide. No ambiguity error here — counting is sensing.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const spec = normalizeLocator(p.locator);
    if (p.frame_id != null) {
      const r = await bridge(tabId, 'locatorQuery', { spec }, p.frame_id).catch(() => ({ total: 0 }));
      return { count: r.total || 0, frame_id: p.frame_id };
    }
    let count = 0;
    try {
      count += (await bridge(tabId, 'locatorQuery', { spec }, MAIN_FRAME)).total || 0;
    } catch { /* no bridge in the top document — frames still count */ }
    if (!count) {
      const frames = (await listExtFrames(tabId)) || [];
      for (const f of frames) {
        if (f.frameId === MAIN_FRAME) continue;
        const r = await bridge(tabId, 'locatorQuery', { spec }, f.frameId).catch(() => null);
        count += r ? r.total || 0 : 0;
      }
    }
    return { count };
  });

  R('locator.all', { category: 'sensing', description: 'List the elements a locator matches (≤ limit per frame, default 10): role/name/text/tag/box/visible plus the @d ref you can act on. Use locator.count for page-wide totals; an action (locator.click/fill/…) refuses when a locator matches more than one element — that is where this listing tells you what to narrow.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const spec = normalizeLocator(p.locator);
    const limit = Math.min(Math.max(p.limit || 10, 1), 50);
    const rows = [];
    const push = (frameId, r) => {
      for (const m of r.matches || []) {
        rows.push({
          frame_id: frameId,
          ref: frameId ? `@d${frameId}.${String(m.ref).slice(2)}` : m.ref,
          tag: m.tag, role: m.role, name: m.name, text: m.text,
          visible: m.visible, disabled: m.disabled, checked: m.checked,
          box: m.box,
        });
      }
    };
    try {
      push(MAIN_FRAME, await bridge(tabId, 'locatorQuery', { spec, limit }, MAIN_FRAME));
    } catch { /* top document without a bridge: frames still list */ }
    if (p.frame_id == null || p.frame_id !== 0) {
      const frames = (await listExtFrames(tabId)) || [];
      for (const f of frames) {
        if (f.frameId === MAIN_FRAME || (p.frame_id != null && f.frameId !== p.frame_id)) continue;
        try {
          push(f.frameId, await bridge(tabId, 'locatorQuery', { spec, limit }, f.frameId));
        } catch { /* unreachable frame: skipped, not guessed */ }
      }
    }
    return { matches: rows, count: rows.length };
  });

  R('locator.text', { category: 'sensing', description: 'Text content of the ONE element a locator matches (Qoder-parity text_content/inner_text): collapsed textContent by default, own text nodes with text:"own". LOCATOR_AMBIGUOUS when several match.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    const d = await bridge(tabId, 'locatorQuery', { spec: p.locator, limit: 1 }, target.frameId);
    const m = (d.matches || [])[0];
    if (!m) throw err('LOCATOR_NOT_FOUND', 'the element vanished between resolve and read');
    return { text: p.text === 'own' ? (m.own_text ?? m.text) : m.text, name: m.name, tag: m.tag, frame_id: target.frameId };
  });

  R('locator.texts', { category: 'sensing', description: 'Collapsed text of EVERY element a locator matches, in DOM order, across frames (Qoder-parity all_text_contents): {texts:[…], frames:{frameId:count}}. No ambiguity refusal — reading is sensing.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const spec = normalizeLocator(p.locator);
    const limit = Math.min(Math.max(p.limit || 50, 1), 200);
    const texts = [];
    const perFrame = {};
    const collect = (frameId, r) => {
      perFrame[frameId] = (r.matches || []).length;
      for (const m of r.matches || []) texts.push(m.text || '');
    };
    try {
      collect(0, await bridge(tabId, 'locatorQuery', { spec, limit }, 0));
    } catch { /* top document without a bridge */ }
    const frames = (await listExtFrames(tabId)) || [];
    for (const f of frames) {
      if (f.frameId === 0) continue;
      try {
        collect(f.frameId, await bridge(tabId, 'locatorQuery', { spec, limit }, f.frameId));
      } catch { /* unreachable frame: skipped */ }
    }
    return { texts, count: texts.length, frames: perFrame };
  });

  R('locator.attribute', { category: 'sensing', description: 'One attribute (or the whole attributes map when name is omitted) of the ONE element a locator matches: id/name/class/href/src/type/alt/title/aria-label/data-testid come from the match snapshot; any other attribute is read live through the frame bridge. LOCATOR_AMBIGUOUS when several match.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    const d = await bridge(tabId, 'locatorQuery', { spec: p.locator, limit: 1 }, target.frameId);
    const m = (d.matches || [])[0];
    if (!m) throw err('LOCATOR_NOT_FOUND', 'the element vanished between resolve and read');
    if (p.name == null) return { attributes: m.attributes || {}, frame_id: target.frameId };
    const attrs = m.attributes || {};
    if (p.name in attrs) return { name: p.name, value: attrs[p.name], frame_id: target.frameId };
    const live = await bridge(tabId, 'describeRef', { ref: target.match.ref }, target.frameId).catch(() => null);
    const v = live && live.attributes ? live.attributes[p.name] : undefined;
    return { name: p.name, value: v === undefined ? null : v, frame_id: target.frameId };
  });

  R('locator.wait_for', { category: 'sensing', timeoutMs: 65_000, description: 'Wait until a locator reaches a state: "visible" (resolves and is rendered), "hidden" (attached but not rendered, or gone), "detached" (no longer matches anywhere). Polls every poll_ms (default 250) up to timeout_ms (default 5000); WAIT_TIMEOUT names the last state seen instead of lying about a condition it could not read.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const spec = normalizeLocator(p.locator);
    const state = String(p.state || 'visible');
    if (!['visible', 'hidden', 'detached'].includes(state)) {
      throw err('BAD_PARAMS', 'state must be visible | hidden | detached');
    }
    const timeoutMs = Math.min(Math.max(p.timeout_ms || 5000, 100), 60_000);
    const pollMs = Math.min(Math.max(p.poll_ms || 250, 50), 2000);
    const deadline = Date.now() + timeoutMs;
    let last = 'absent';
    for (;;) {
      let found = null;
      try {
        found = await resolveLocator(tabId, spec, { frame_id: p.frame_id ?? null });
        last = found.match.visible ? 'visible' : 'hidden';
      } catch (e) {
        if (e && e.code === 'LOCATOR_NOT_FOUND') last = 'absent';
        else if (e && e.code === 'LOCATOR_AMBIGUOUS') throw e;
        else last = 'unreachable';
      }
      const ok =
        state === 'visible' ? last === 'visible' :
        state === 'hidden' ? (last === 'hidden' || last === 'absent') :
        last === 'absent';
      if (ok) return { state, reached: true, last_seen: last, waited_ms: timeoutMs - (deadline - Date.now()) };
      if (Date.now() >= deadline) {
        throw err('WAIT_TIMEOUT', `locator ${JSON.stringify(spec)} never reached state "${state}" within ${timeoutMs}ms (last seen: ${last})`, { last_state: last });
      }
      await sleep(pollMs);
    }
  });

  R('locator.click', { category: 'interaction', description: 'Click ONE element chosen by a Playwright-style locator. More than one match is a refusal (LOCATOR_AMBIGUOUS with candidates), never a guess. Non-OOPIF targets get REAL coordinate input through the shield gate with the page-side delivery proof (delivered:true / INPUT_NOT_DELIVERED); an out-of-process iframe target falls back to the frame bridge\'s synthetic DOM click (isTrusted:false, reported via synthetic:true). Dangerous labels still need confirm:true. If the bridge locatorQuery itself fails (closed shadow UI), it falls back to a CDP pierced-DOM match by role/text and clicks that box (via:"pierced").' }, async (p = {}) => {
    const tabId = targetTab(p);
    let target = null;
    try {
      target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    } catch (e) {
      const fb = await piercedClick(tabId, p.locator || {}).catch(() => null);
      if (!fb) throw e;
      if (DANGEROUS_LABEL.test(fb.text || '') && p.confirm !== true) {
        throw err('NEEDS_CONFIRM', `"${fb.text}" looks like a destructive/paid action — retry with confirm:true after asking the user`);
      }
      audit.log({ action: 'locator.click', tabId, target: fb.text, via: 'pierced' });
      return { clicked: fb.text || 'pierced', via: 'pierced', at: [Math.round(fb.cx), Math.round(fb.cy)], delivered: true, note: 'bridge locatorQuery failed; matched in the CDP pierced DOM (closed shadow roots included) and clicked the box' };
    }
    const label = target.match.name || '';
    if (DANGEROUS_LABEL.test(label) && p.confirm !== true) {
      throw err('NEEDS_CONFIRM', `"${label}" looks like a destructive/paid action — retry with confirm:true after asking the user`);
    }
    if (target.oopif) {
      await bridge(tabId, 'clickDom', { ref: target.match.ref }, target.frameId);
      audit.log({ action: 'locator.click', tabId, target: label, via: 'locator-bridge', frame: target.frameId, synthetic: true });
      return { clicked: label || target.ref, via: 'locator-bridge', frame_id: target.frameId, synthetic: true };
    }
    const r = await realClick(tabId, target, { double: p.double === true, button: p.button === 'right' ? 'right' : 'left', label });
    audit.log({ action: 'locator.click', tabId, target: label, via: 'locator', delivered: r.delivered === true });
    return {
      clicked: label || target.ref,
      via: 'locator',
      at: [Math.round(r.box.cx), Math.round(r.box.cy)],
      frame_id: target.frameId,
      delivered: r.delivered,
      delivery_ms: r.delivery_ms,
    };
  });

  R('locator.fill', { category: 'interaction', description: 'Set the value of ONE form field chosen by a locator (React/Vue-safe native setter + change event, through that frame\'s bridge — works in every frame including OOPIFs and on background tabs). The bridge enforces the same field rules as fill: OTP/payment/card fields refused, password fields only for a daemon-resolved secret (use fill with {{secret:…}} + an @eN ref for those). More than one match is a refusal.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    if (typeof p.value !== 'string') throw err('BAD_PARAMS', 'value (string) required');
    const r = await bridge(tabId, 'fillDom', { ref: target.match.ref, value: p.value }, target.frameId);
    audit.log({ action: 'locator.fill', tabId, target: target.match.name, via: 'locator-bridge', frame: target.frameId });
    return {
      filled: target.match.name || target.ref,
      via: 'locator-bridge',
      frame_id: target.frameId,
      ...(r && typeof r === 'object' ? { result: r } : {}),
    };
  });

  R('locator.select_option', { category: 'interaction', description: 'Select an option (by value or visible text) on ONE <select> chosen by a locator, through that frame\'s bridge (synthetic change event, same as form.set select_option on a frame ref). More than one match is a refusal.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    const v = await bridge(tabId, 'selectDom', { ref: target.match.ref, value: String(p.value ?? '') }, target.frameId);
    audit.log({ action: 'locator.select_option', tabId, target: target.match.name, via: 'locator-bridge', frame: target.frameId });
    return { set: target.match.name || target.ref, via: 'locator-bridge', frame_id: target.frameId, result: v };
  });

  R('locator.hover', { category: 'interaction', description: 'Move the real pointer to ONE element chosen by a locator (delivery-gated like hover; CSS :hover still needs a visible window). Out-of-process iframe targets are refused with LOCATOR_UNSUPPORTED — a synthetic hover reveals nothing, so no fallback is offered. More than one match is a refusal.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    if (target.oopif) {
      throw err('LOCATOR_UNSUPPORTED', 'hover cannot reach an out-of-process iframe (no real pointer channel; a synthetic one would reveal nothing) — act on the frame\'s URL as a top-level tab, or use locator.all to find the element\'s ref and hover by @eN from a snapshot');
    }
    await describeTarget(tabId, target);
    const { box } = await describeTarget(tabId, target);
    if (!box || !Number.isFinite(box.cx)) throw err('ELEMENT_NOT_VISIBLE', 'the matched element has no visible box');
    await withPass(tabId, async () => mouse(tabId, 'mouseMoved', box.cx, box.cy), {
      frameId: target.frameId,
      at: [box.cx, box.cy],
    });
    audit.log({ action: 'locator.hover', tabId, target: target.match.name, via: 'locator' });
    return { hovered: target.match.name || target.ref, at: [Math.round(box.cx), Math.round(box.cy)], frame_id: target.frameId };
  });

  R('locator.press', { category: 'interaction', description: 'Focus ONE element chosen by a locator (real coordinate click, delivery-proved) and press a key with optional modifiers (e.g. key:"Enter", key:"a", modifiers:["ctrl"]). Out-of-process iframe targets are refused (LOCATOR_UNSUPPORTED): the focus click cannot reach them. More than one match is a refusal.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const target = await resolveLocator(tabId, p.locator, { frame_id: p.frame_id ?? null });
    if (target.oopif) {
      throw err('LOCATOR_UNSUPPORTED', 'press cannot reach an out-of-process iframe (the focusing click has no real-input channel there) — open the frame URL as a top-level tab instead');
    }
    if (!p.key) throw err('BAD_PARAMS', 'key required (e.g. "Enter", "a" with modifiers:["ctrl"])');
    const r = await realClick(tabId, target, { label: target.match.name || '' });
    await sleep(60); // let focus land before the burst, same as frameFillInput
    const mods = modifierBits(Array.isArray(p.modifiers) ? p.modifiers : []);
    await key(tabId, 'keyDown', p.key, { modifiers: mods });
    await key(tabId, 'keyUp', p.key, { modifiers: mods });
    audit.log({ action: 'locator.press', tabId, target: target.match.name, key: p.key, delivered: r.delivered });
    return { pressed: p.key, on: target.match.name || target.ref, frame_id: target.frameId, delivered: r.delivered, delivery_ms: r.delivery_ms };
  });
}
