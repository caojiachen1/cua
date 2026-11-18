// Codex-style in-page agent cursor (absorb wave, plan §8): a small overlay dot
// with a short zh bubble that glides to where the last successful write action
// landed, then fades after 1.2s. Pure cosmetics — pointer-events:none, the
// highest sane z-index (below the shield's 2147483647 so the veil still covers
// it), and every failure stays swallowed inside this file.
//
// Wiring (plan §3): content/bridge.js dispatches `cua-agent-action`
// CustomEvents on the TOP window after each successful top-document write
// action (DOM-fallback path). CDP real-input actions never enter bridge.js —
// the background instead sends `CUA_CURSOR {x, y, label, op?}` via
// chrome.tabs.sendMessage(..., {frameId:0}) after each successful one (CORE-2).
// Both entrances share one render path below. Only the top frame renders — a
// subframe never sees those window events, and the message listener is only
// ever registered here.

(() => {
  if (window.__cuaAgentCursor) return;
  window.__cuaAgentCursor = true;

  let isTop = true;
  try { isTop = window.top === window; } catch { isTop = false; }
  if (!isTop) return;

  const FADE_AFTER_MS = 1200;
  const CSS = `
#cua-agent-cursor{position:fixed;left:0;top:0;z-index:2147483646;pointer-events:none;
  opacity:0;transition:transform .18s cubic-bezier(.2,.8,.25,1),opacity .35s ease;
  will-change:transform;font-family:system-ui,-apple-system,"Segoe UI","Microsoft YaHei",sans-serif}
#cua-agent-cursor.cua-on{opacity:1}
.cua-cursor-dot{position:absolute;left:-7px;top:-7px;width:14px;height:14px;border-radius:50%;
  background:#3b82f6;border:2px solid #fff;box-shadow:0 1px 6px rgba(0,0,0,.35)}
#cua-agent-cursor.cua-on .cua-cursor-dot{animation:cua-cursor-pulse 1.2s ease-in-out infinite}
.cua-cursor-bubble{position:absolute;left:14px;top:-30px;max-width:260px;white-space:nowrap;
  overflow:hidden;text-overflow:ellipsis;background:rgba(17,24,39,.92);color:#fff;font-size:12px;
  line-height:1;padding:6px 9px;border-radius:999px;box-shadow:0 2px 8px rgba(0,0,0,.3)}
#cua-agent-cursor.cua-flip .cua-cursor-bubble{left:auto;right:14px}
#cua-agent-cursor.cua-below .cua-cursor-bubble{top:14px}
@keyframes cua-cursor-pulse{0%,100%{transform:scale(1)}50%{transform:scale(1.25)}}
`;

  let enabled = true;
  let listening = false;
  let root = null;
  let bubbleEl = null;
  let placed = false; // first placement jumps without the glide transition
  let hideTimer = null;

  function ensureRoot() {
    if (root) return root;
    // an extension reload tears the old world down but leaves its nodes in the
    // shared DOM — drop any stale overlay with the same id before re-creating
    document.getElementById('cua-agent-cursor')?.remove();
    document.getElementById('cua-agent-cursor-style')?.remove();
    const style = document.createElement('style');
    style.id = 'cua-agent-cursor-style';
    style.textContent = CSS;
    (document.head || document.documentElement).appendChild(style);
    root = document.createElement('div');
    root.id = 'cua-agent-cursor';
    const dot = document.createElement('div');
    dot.className = 'cua-cursor-dot';
    bubbleEl = document.createElement('div');
    bubbleEl.className = 'cua-cursor-bubble';
    root.appendChild(dot);
    root.appendChild(bubbleEl);
    (document.documentElement || document.body).appendChild(root);
    return root;
  }

  // detail.label (composed by the dispatcher) wins; bare ops still get a
  // readable bubble so future emitters (press_key/scroll live outside bridge.js)
  // degrade gracefully instead of printing an English op name.
  function labelFor(op, label, key) {
    const raw = typeof label === 'string' ? label.replace(/\s+/g, ' ').trim() : '';
    if (raw) return raw.slice(0, 40);
    switch (op) {
      case 'click': return '点击';
      case 'fill': case 'type': return '输入';
      case 'select': return '选择';
      case 'scroll': return '滚动';
      case 'press_key': {
        const k = key != null ? String(key).slice(0, 16) : '';
        return k ? `按键 ${k}` : '按键';
      }
      default: return '动作';
    }
  }

  // THE single render path: both entrances land here with viewport coords.
  function render(x, y, op, label, key) {
    const x0 = Number(x);
    const y0 = Number(y);
    if (!Number.isFinite(x0) || !Number.isFinite(y0)) return;
    const r = ensureRoot();
    // clamp so the dot + bubble can never chase a target off-screen
    const cx = Math.min(Math.max(x0, 10), Math.max(window.innerWidth - 10, 10));
    const cy = Math.min(Math.max(y0, 10), Math.max(window.innerHeight - 10, 10));
    bubbleEl.textContent = labelFor(op, label, key);
    const bw = bubbleEl.offsetWidth || 0;
    r.classList.toggle('cua-flip', cx + 18 + bw > window.innerWidth - 8);
    r.classList.toggle('cua-below', cy < 44);
    if (!placed) {
      r.style.transition = 'none';
      r.style.transform = `translate(${cx}px, ${cy}px)`;
      void r.offsetWidth; // flush so the NEXT move actually glides
      r.style.transition = '';
      placed = true;
    } else {
      r.style.transform = `translate(${cx}px, ${cy}px)`;
    }
    r.classList.add('cua-on');
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      try { root?.classList.remove('cua-on'); } catch {}
      hideTimer = null;
    }, FADE_AFTER_MS);
  }

  function onAction(ev) {
    try {
      const d = ev.detail || {};
      render(d.x, d.y, String(d.op || '').toLowerCase(), typeof d.label === 'string' ? d.label : '', d.key);
    } catch { /* cosmetic only */ }
  }

  // Background-side entrance (CUA_CURSOR, frameId:0 → this top frame only).
  // Fire-and-forget on the sender side: this listener never calls sendResponse
  // and must not return true, so the message channel closes immediately and a
  // render failure can never leak into the response path.
  function onCursorMsg(msg) {
    if (msg?.type !== 'CUA_CURSOR') return;
    try {
      render(
        msg.x, msg.y,
        typeof msg.op === 'string' && msg.op ? msg.op : 'auto',
        typeof msg.label === 'string' ? msg.label : '',
        msg.key,
      );
    } catch { /* cosmetic only */ }
  }

  function attach() {
    if (listening || !enabled) return;
    window.addEventListener('cua-agent-action', onAction);
    try { chrome.runtime.onMessage.addListener(onCursorMsg); } catch { /* no runtime: event path only */ }
    listening = true;
  }

  // guard off: remove both listeners and every leftover node (hot update)
  function detach() {
    if (listening) {
      window.removeEventListener('cua-agent-action', onAction);
      try { chrome.runtime.onMessage.removeListener?.(onCursorMsg); } catch {}
      listening = false;
    }
    if (hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
    if (root) { root.remove(); root = null; bubbleEl = null; placed = false; }
    document.getElementById('cua-agent-cursor-style')?.remove();
  }

  attach(); // 默认开：先挂上，storage 回答 false 再摘（见下）

  try {
    if (chrome.storage?.local?.get) {
      chrome.storage.local.get('cua_cursor_enabled', (res) => {
        try {
          if (chrome.runtime.lastError) return; // 读不到就保持默认开
          enabled = res?.cua_cursor_enabled !== false;
          if (enabled) attach();
          else detach();
        } catch { /* cosmetic */ }
      });
      chrome.storage.onChanged.addListener((changes, area) => {
        try {
          if (area !== 'local' || !('cua_cursor_enabled' in changes)) return;
          enabled = changes.cua_cursor_enabled.newValue !== false;
          if (enabled) attach();
          else detach();
        } catch { /* cosmetic */ }
      });
    }
  } catch { /* storage 不可用：维持默认开 */ }
})();
