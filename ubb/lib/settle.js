// Post-action settle: after an input action, wait until the page has absorbed
// it before the model is invited to look again. The old flow returned the
// instant the click was PROVEN delivered and left "when to look" to the model,
// which meant blind sleeps between steps; this module replaces the sleep with
// evidence: in-flight requests drain, a main-frame navigation reaches an
// interactive readiness probe, and the DOM goes mutation-quiet.
//
// Every wait here is best-effort with a budget: settle NEVER throws and NEVER
// extends past its deadline — it reports what it saw (settled / navigation /
// pending_requests / quiet) so the caller stays honest either way.

import { cdp } from './cdp.js';

const HOOK_WINDOW_MS = 100; // let the action's own event handlers kick off their work
const POLL_MS = 120;
const READY_MIN_TEXT_CHARS = 200;
const READY_MIN_INTERACTIVE = 3;
const QUIET_SAMPLES = 2; // consecutive mutation-free samples

const PROBE_FN = `(function(){
  try {
    if (!window.__ubbDomMonitor) {
      const m = { mutations: 0 };
      const obs = new MutationObserver(function(recs){
        for (var i = 0; i < recs.length; i++) {
          m.mutations += recs[i].addedNodes.length + recs[i].removedNodes.length + 1;
        }
      });
      obs.observe(document.documentElement, { childList: true, subtree: true, attributes: true, characterData: true });
      window.__ubbDomMonitor = m;
    }
  } catch (e) { window.__ubbDomMonitor = { mutations: 0, err: String(e && e.message || e) }; }
  var body = document.body;
  var inter = 'a[href],[role="button"],[role="link"],[role="menuitem"],[role="option"],button,input:not([type="hidden"]),select,textarea,[onclick]';
  var land = '[role="main"],[role="navigation"],[role="banner"],[role="contentinfo"],main,nav,header,footer';
  return {
    interactive: document.querySelectorAll(inter).length,
    landmarks: document.querySelectorAll(land).length,
    text_chars: body ? body.textContent.length : 0,
    ready: document.readyState,
    mut: (window.__ubbDomMonitor && window.__ubbDomMonitor.mutations) || 0,
    monitor_err: (window.__ubbDomMonitor && window.__ubbDomMonitor.err) || null,
  };
})()`;

async function probe(tabId) {
  // main world: the monitor must survive across polls, and a MAIN-world
  // MutationObserver is what watches the page the user sees
  const r = await cdp.send(tabId, 'Runtime.evaluate', {
    expression: PROBE_FN,
    awaitPromise: false,
    returnByValue: true,
  }).catch(() => null);
  return r?.result?.value || null;
}

function pendingRequests(st, since) {
  const out = new Set();
  for (const r of st?.net || []) {
    if (r.t >= since && !r.finished && !r.error) out.add(r.requestId);
  }
  return out;
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Wait for the page to absorb an action. Budget-bounded, never throws.
 * opts: { budget_ms (default 4000, cap 15000), request_budget_ms (default 2500),
 *         redirect_guard_ms (default 700) }
 */
export async function settleAfterAction(tabId, opts = {}) {
  const budget = Math.min(Math.max(Number(opts.budget_ms) || 4000, 200), 15000);
  const requestBudget = Math.min(Math.max(Number(opts.request_budget_ms) || 2500, 0), budget);
  const redirectGuard = Math.min(Math.max(Number(opts.redirect_guard_ms) || 700, 0), 3000);
  const t0 = Date.now();
  const deadline = t0 + budget;
  const out = { settled: false, navigation: false, pending_requests: 0, quiet: false, elapsed_ms: 0 };

  try {
    let st = cdp.buffers(tabId);
    if (!st) {
      out.reason = 'no debugger session';
      out.elapsed_ms = Date.now() - t0;
      return out;
    }
    if (st.dialogs?.length) {
      out.reason = 'dialog_open';
      out.elapsed_ms = Date.now() - t0;
      return out;
    }
    const gen0 = st.gen;
    const reqDeadline = t0 + requestBudget;

    await sleep(HOOK_WINDOW_MS);

    // ---- phase 1: drain the requests the action kicked off
    let pending = pendingRequests(st, t0 - 50);
    while (pending.size > 0 && Date.now() < reqDeadline) {
      st = cdp.buffers(tabId);
      if (!st) break;
      if (st.gen !== gen0) {
        out.navigation = true; // navigation reset the network buffer
        break;
      }
      for (const id of pending) {
        const r = st.net.find((x) => x.requestId === id);
        if (!r || r.finished || r.error) pending.delete(id);
      }
      if (pending.size === 0) break;
      await sleep(POLL_MS);
    }
    out.pending_requests = pending.size;

    // ---- phase 2: navigation readiness / DOM quiet
    if (out.navigation || (cdp.buffers(tabId)?.gen ?? gen0) !== gen0) {
      out.navigation = true;
      let guardArmed = false;
      let guardUntil = 0;
      while (Date.now() < deadline) {
        const p = await probe(tabId);
        const g = cdp.buffers(tabId)?.gen;
        if (g !== gen0) {
          // a second navigation (redirect chain): restart the readiness window
          guardArmed = false;
          guardUntil = 0;
          await sleep(POLL_MS);
          continue;
        }
        if (!p) {
          await sleep(POLL_MS);
          continue;
        }
        const looksReady =
          p.ready === 'complete' &&
          (p.text_chars >= READY_MIN_TEXT_CHARS || p.interactive >= READY_MIN_INTERACTIVE || p.landmarks >= 1);
        // redirect guard: a document that renders nothing interactive is often
        // a waypoint — hold off before calling it ready (Aside's guard, kept)
        if (looksReady && p.interactive === 0 && !guardArmed) {
          guardArmed = true;
          guardUntil = Date.now() + redirectGuard;
        }
        if (looksReady && (!guardArmed || Date.now() >= guardUntil)) {
          out.settled = true;
          break;
        }
        await sleep(POLL_MS);
      }
    } else {
      // ---- DOM quiet: cumulative mutation counter stays flat
      let lastMut = -1;
      let quietRun = 0;
      while (Date.now() < deadline) {
        const p = await probe(tabId);
        if (!p) {
          await sleep(POLL_MS);
          continue;
        }
        if (p.monitor_err) {
          out.monitor_error = p.monitor_err;
          break; // an observing failure must not masquerade as quiet
        }
        const g = cdp.buffers(tabId)?.gen;
        if (g !== gen0) {
          out.navigation = true;
          break;
        }
        quietRun = lastMut >= 0 && p.mut === lastMut ? quietRun + 1 : 0;
        lastMut = p.mut;
        if (quietRun >= QUIET_SAMPLES) {
          out.settled = true;
          out.quiet = true;
          break;
        }
        await sleep(POLL_MS);
      }
    }
  } catch (e) {
    out.settled = false;
    out.error = String(e?.message || e).slice(0, 160);
  }
  out.elapsed_ms = Date.now() - t0;
  return out;
}
