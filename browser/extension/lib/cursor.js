// Cursor preview emitter (background/service-worker side).
//
// The in-page overlay (content/agent-cursor.js, top frame only) renders three
// phases around every real input: `pre` ("即将 …" — emitted by the interaction
// tools before dispatch), `act` ("正在 …" — emitted per CDP dispatch from
// lib/actions.js mouse()/key(), throttled for event bursts like drags), and
// `post` ("已 …" + ripple, or a red ✗ on refusal/failure).
//
// Delivery is lazy-inject + fire-and-forget: try tabs.sendMessage(frameId:0)
// first; when no listener answers (page loaded before the extension loaded, or
// a content-script resync has not happened yet), inject the overlay with
// chrome.scripting once and resend. Every failure is swallowed — the preview
// must never be able to fail the action it decorates.

let enabled = true;
try {
  if (chrome.storage?.local?.get) {
    chrome.storage.local.get('cua_cursor_enabled', (res) => {
      try { enabled = res?.cua_cursor_enabled !== false; } catch {}
    });
    chrome.storage.onChanged.addListener((changes, area) => {
      try {
        if (area === 'local' && 'cua_cursor_enabled' in changes) {
          enabled = changes.cua_cursor_enabled.newValue !== false;
        }
      } catch {}
    });
  }
} catch { /* keep default on */ }

// mouseMoved bursts (drags, challenge slides) would fire dozens of messages;
// coalesce them to at most one per 120ms per tab, always sending the LATEST
// coordinates (trailing edge) so the dot still lands where the drag ended.
const lastMove = new Map(); // tabId -> {timer, spec}

export function cursorEmit(tabId, spec) {
  try {
    if (!enabled || !tabId) return;
    if (spec.phase === 'act' && (spec.op === 'hover' || spec.op === 'drag')) {
      const prev = lastMove.get(tabId);
      if (prev && prev.timer) clearTimeout(prev.timer);
      const timer = setTimeout(() => {
        try { lastMove.delete(tabId); } catch {}
        send(tabId, lastMove.get(tabId)?.spec || spec);
      }, 120);
      lastMove.set(tabId, { timer, spec });
      return;
    }
    send(tabId, spec);
  } catch { /* cosmetic only */ }
}

async function send(tabId, spec) {
  try {
    if (!spec) return;
    const msg = { type: 'CUA_CURSOR', ...spec };
    try {
      await chrome.tabs.sendMessage(tabId, msg, { frameId: 0 });
      return;
    } catch { /* no listener in that frame yet — inject and retry */ }
    try {
      await chrome.scripting.executeScript({
        target: { tabId },
        files: ['content/agent-cursor.js'],
      });
    } catch { /* restricted page — the overlay simply will not show there */ }
    await chrome.tabs.sendMessage(tabId, msg, { frameId: 0 }).catch(() => {});
  } catch { /* cosmetic only */ }
}
