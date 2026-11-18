// Locator resolution shared by the locator.* tools: run the locator-core
// matcher in the main frame first, then (when the main frame found nothing)
// in every ext frame with a bridge. Returns ONE unambiguous target with its
// @d ref and frame-viewport box; ambiguity is a refusal, never a guess.

import { err } from './config.js';
import { bridge } from './bridge-ctl.js';
import { listExtFrames, frameIsOutOfProcess } from './snapshot.js';

const MAIN_FRAME = 0;

const ALLOWED_KEYS = new Set([
  'css', 'role', 'text', 'text_exact', 'label', 'label_exact', 'placeholder',
  'placeholder_exact', 'test_id', 'alt', 'value', 'has_text', 'has_not_text',
  'checked', 'disabled', 'visible', 'nth',
]);

/** Validate + normalize the locator grammar at the tool boundary. */
export function normalizeLocator(spec) {
  if (!spec || typeof spec !== 'object' || Array.isArray(spec)) {
    throw err('BAD_PARAMS', 'locator must be an object, e.g. {"role":"button","text":"提交"}');
  }
  const keys = Object.keys(spec);
  if (!keys.length) {
    throw err('BAD_PARAMS', 'locator is empty — give at least one of: css, role, text, label, placeholder, test_id, alt, value, has_text, has_not_text, checked, disabled, visible, nth');
  }
  const unknown = keys.filter((k) => !ALLOWED_KEYS.has(k));
  if (unknown.length) {
    throw err('BAD_PARAMS', `unknown locator key(s) ${unknown.join(', ')} — allowed: ${[...ALLOWED_KEYS].join(', ')}`);
  }
  const out = { ...spec };
  if (out.nth != null) {
    out.nth = Number(out.nth);
    if (!Number.isInteger(out.nth) || out.nth < 0) throw err('BAD_PARAMS', 'locator.nth must be a non-negative integer');
  }
  return out;
}

/** Query ONE frame through its bridge. frame 0 = main document. */
async function queryFrame(tabId, frameId, spec, limit) {
  try {
    const r = await bridge(tabId, 'locatorQuery', { spec, limit }, frameId);
    return r || { total: 0, matches: [] };
  } catch (e) {
    // a frame without a bridge (srcdoc/blank/file) or a dead one contributes
    // zero candidates — the main-frame query is the primary sense anyway
    if (e && e.code === 'FRAME_UNREACHABLE') return { total: 0, matches: [], unreachable: true };
    throw e;
  }
}

/**
 * Resolve a locator to exactly one element.
 * @returns {{frameId, ref, match, oopif, scanned_frames}} — `ref` is the
 *   bridge's `@dN` (main) / `@d<frameId>.<n>` (child frame) form; `match.box`
 *   is in the target frame's own viewport.
 */
export async function resolveLocator(tabId, specRaw, { frame_id = null, limit = 20 } = {}) {
  const spec = normalizeLocator(specRaw);
  if (frame_id != null) {
    const r = await queryFrame(tabId, frame_id, spec, 2);
    if (!r.total) throw err('LOCATOR_NOT_FOUND', `no element matches ${JSON.stringify(spec)} in frame ${frame_id}`);
    if (r.total > 1) throw err('LOCATOR_AMBIGUOUS', `${r.total} elements match ${JSON.stringify(spec)} in frame ${frame_id} — narrow it (add text/role, or locator.nth)`, { candidates: r.matches.slice(0, 5).map(shortMatch) });
    const m = r.matches[0];
    return { frameId: frame_id, ref: `@d${frame_id}.${m.ref.slice(2)}`, match: m, oopif: await frameIsOutOfProcess(tabId, frame_id), scanned_frames: [frame_id] };
  }
  const main = await queryFrame(tabId, MAIN_FRAME, spec, 2);
  if (main.total === 1) {
    return { frameId: MAIN_FRAME, ref: main.matches[0].ref, match: main.matches[0], oopif: false, scanned_frames: [MAIN_FRAME] };
  }
  if (main.total > 1) {
    throw err('LOCATOR_AMBIGUOUS', `${main.total} elements match ${JSON.stringify(spec)} in the main document — narrow it (add text/role, or locator.nth)`, { candidates: main.matches.slice(0, 5).map(shortMatch) });
  }
  // main document came up empty: sweep the child frames (bounded, in order)
  const frames = (await listExtFrames(tabId)) || [];
  const scanned = [MAIN_FRAME];
  const across = [];
  for (const f of frames) {
    if (f.frameId === MAIN_FRAME) continue;
    if (scanned.length > 24) break;
    scanned.push(f.frameId);
    const r = await queryFrame(tabId, f.frameId, spec, 2);
    for (const m of r.matches) {
      across.push({ frameId: f.frameId, match: m });
      if (across.length > 1) break;
    }
    if (across.length > 1) break;
  }
  if (!across.length) {
    throw err('LOCATOR_NOT_FOUND', `no element matches ${JSON.stringify(spec)} in the main document or any frame`, { unreachable_frames: main.unreachable ? 1 : undefined });
  }
  if (across.length > 1) {
    throw err('LOCATOR_AMBIGUOUS', `${across.length} elements match ${JSON.stringify(spec)} across frames — pass frame_id, or narrow the spec`, {
      candidates: across.slice(0, 5).map((c) => ({ ...shortMatch(c.match), frame_id: c.frameId })),
    });
  }
  const { frameId, match } = across[0];
  return { frameId, ref: `@d${frameId}.${match.ref.slice(2)}`, match, oopif: await frameIsOutOfProcess(tabId, frameId), scanned_frames: scanned };
}

function shortMatch(m) {
  return { role: m.role, name: m.name, tag: m.tag, text: (m.text || '').slice(0, 60) };
}
