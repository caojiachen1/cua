// Codex-style in-page agent cursor: a small overlay dot with a zh bubble that
// GLIDES to where the agent is about to act, shows what it is doing while the
// input is on the wire, then reports what it did (ok/fail) with a click ripple
// before fading. Pure cosmetics — pointer-events:none, the highest sane
// z-index (below the shield's 2147483647 so the veil still covers it), and
// every failure stays swallowed inside this file.
//
// Entrances (three, all funnel into render()):
//   1. content/bridge.js dispatches `cua-agent-action` CustomEvents on the TOP
//      window after each successful top-document write op (DOM-fallback path:
//      fill/click/select via callOnNode). detail carries phase:'post'.
//   2. The background sends `CUA_CURSOR {phase, x, y, op, label, key, ok}`
//      via chrome.tabs.sendMessage(..., {frameId:0}) around every CDP real-input
//      action — lib/actions.js mouse()/key() emit act/post, and the interaction
//      tools emit pre ("即将 …") before dispatch. Sent to frameId:0 even for
//      subframe targets: the coordinates are top-document viewport coords and
//      only the top frame renders.
//   3. Storage changes hot-toggle the whole overlay (popup 「高级 · 安全开关」).
//
// Only the top frame renders — a subframe never sees those window events, and
// the message listener is only ever registered here.

(() => {
  if (window.__cuaAgentCursor) return;
  window.__cuaAgentCursor = true;

  let isTop = true;
  try { isTop = window.top === window; } catch { isTop = false; }
  if (!isTop) return;

  const FADE_AFTER_MS = 1400;
  const PRE_HOLD_MS = 2600; // "即将 …" lingers until the act/post replaces it
  const CSS = `
#cua-agent-cursor{position:fixed;left:0;top:0;z-index:2147483646;pointer-events:none;
  opacity:0;transition:transform .18s cubic-bezier(.2,.8,.25,1),opacity .35s ease;
  will-change:transform;font-family:system-ui,-apple-system,"Segoe UI","Microsoft YaHei",sans-serif}
#cua-agent-cursor.cua-on{opacity:1}
.cua-cursor-dot{position:absolute;left:-7px;top:-7px;width:14px;height:14px;border-radius:50%;
  background:#3b82f6;border:2px solid #fff;box-shadow:0 1px 6px rgba(0,0,0,.35)}
#cua-agent-cursor.cua-on .cua-cursor-dot{animation:cua-cursor-pulse 1.2s ease-in-out infinite}
#cua-agent-cursor.cua-pre .cua-cursor-dot{background:#f59e0b;border-color:#fff}
#cua-agent-cursor.cua-act .cua-cursor-dot{background:#3b82f6;border-color:#fff}
#cua-agent-cursor.cua-post .cua-cursor-dot{background:#22c55e;border-color:#fff}
#cua-agent-cursor.cua-fail .cua-cursor-dot{background:#ef4444;border-color:#fff}
.cua-cursor-ring{position:absolute;left:-9px;top:-9px;width:18px;height:18px;border-radius:50%;
  border:2px solid #22c55e;opacity:0}
.cua-cursor-ring.cua-run{animation:cua-cursor-ripple .6s ease-out forwards}
@keyframes cua-cursor-ripple{0%{transform:scale(.4);opacity:.9}
  100%{transform:scale(2.6);opacity:0}}
.cua-cursor-bubble{position:absolute;left:14px;top:-28px;max-width:300px;white-space:nowrap;
  overflow:hidden;text-overflow:ellipsis;background:rgba(0,0,0,.9);color:#fff;font-size:11.5px;
  line-height:1;padding:5px 8px;border-radius:999px;box-shadow:0 1px 6px rgba(0,0,0,.4);
  border:1px solid rgba(255,255,255,.2)}
#cua-agent-cursor.cua-fail .cua-cursor-bubble{background:rgba(0,0,0,.95)}
.cua-cursor-bubble .cua-cursor-kind{opacity:.65;margin-right:4px}
#cua-agent-cursor.cua-flip .cua-cursor-bubble{left:auto;right:14px}
#cua-agent-cursor.cua-below .cua-cursor-bubble{top:14px}
@keyframes cua-cursor-pulse{0%,100%{transform:scale(1)}50%{transform:scale(1.25)}}
`;

  let enabled = true;
  let listening = false;
  let root = null;
  let bubbleEl = null;
  let ringEl = null;
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
    ringEl = document.createElement('div');
    ringEl.className = 'cua-cursor-ring';
    bubbleEl = document.createElement('div');
    bubbleEl.className = 'cua-cursor-bubble';
    root.appendChild(ringEl);
    root.appendChild(dot);
    root.appendChild(bubbleEl);
    (document.documentElement || document.body).appendChild(root);
    return root;
  }

  // detail.label (composed by the dispatcher) wins; bare ops still get a
  // readable bubble so every emitter degrades gracefully instead of printing
  // an English op name.
  function labelFor(op, label, key) {
    const raw = typeof label === 'string' ? label.replace(/\s+/g, ' ').trim() : '';
    if (raw) return raw.slice(0, 40);
    switch (op) {
      case 'click': return '点击';
      case 'hover': return '悬停';
      case 'fill': case 'type': return '输入';
      case 'select': return '选择';
      case 'scroll': return '滚动';
      case 'drag': return '拖拽';
      case 'press_key': {
        const k = key != null ? String(key).slice(0, 16) : '';
        return k ? `按键 ${k}` : '按键';
      }
      default: return '动作';
    }
  }

  let lastX = null;
  let lastY = null;
  // THE single render path: every entrance lands here with viewport coords and
  // a phase ('pre' | 'act' | 'post' | '' — the bridge's legacy events are post).
  // x/y may be null (key events have no point) — the dot then stays where it
  // last was and only the bubble updates.
  function render(x, y, op, label, key, phase, ok) {
    const x0 = Number.isFinite(Number(x)) ? Number(x) : lastX;
    const y0 = Number.isFinite(Number(y)) ? Number(y) : lastY;
    if (!Number.isFinite(x0) || !Number.isFinite(y0)) return;
    lastX = x0;
    lastY = y0;
    const r = ensureRoot();
    // clamp so the dot + bubble can never chase a target off-screen
    const cx = Math.min(Math.max(x0, 10), Math.max(window.innerWidth - 10, 10));
    const cy = Math.min(Math.max(y0, 10), Math.max(window.innerHeight - 10, 10));
    const kind = labelFor(op, label, key);
    const p = phase === 'pre' || phase === 'act' || phase === 'post' ? phase : 'post';
    // colour the dot by phase and prefix the bubble with what the phase means
    for (const cls of ['cua-pre', 'cua-act', 'cua-post', 'cua-fail']) r.classList.remove(cls);
    const kindEl = document.createElement('span');
    kindEl.className = 'cua-cursor-kind';
    if (p === 'pre') {
      r.classList.add('cua-pre');
      kindEl.textContent = '即将';
      bubbleEl.textContent = '';
      bubbleEl.appendChild(kindEl);
      bubbleEl.appendChild(document.createTextNode(kind));
    } else if (p === 'act') {
      r.classList.add('cua-act');
      kindEl.textContent = '正在';
      bubbleEl.textContent = '';
      bubbleEl.appendChild(kindEl);
      bubbleEl.appendChild(document.createTextNode(kind));
    } else {
      r.classList.add(ok === false ? 'cua-fail' : 'cua-post');
      kindEl.textContent = ok === false ? '✗' : '已';
      bubbleEl.textContent = '';
      bubbleEl.appendChild(kindEl);
      bubbleEl.appendChild(document.createTextNode(kind));
    }
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
    // success ripple on the post phase
    if (p === 'post' && ok !== false && ringEl) {
      try {
        ringEl.classList.remove('cua-run');
        void ringEl.offsetWidth;
        ringEl.classList.add('cua-run');
      } catch { /* cosmetic */ }
    }
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      try { root?.classList.remove('cua-on'); } catch {}
      hideTimer = null;
    }, p === 'pre' ? PRE_HOLD_MS : FADE_AFTER_MS);
  }

  function onAction(ev) {
    try {
      const d = ev.detail || {};
      render(d.x, d.y, String(d.op || '').toLowerCase(), typeof d.label === 'string' ? d.label : '', d.key,
        typeof d.phase === 'string' ? d.phase : 'post', d.ok);
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
        typeof msg.phase === 'string' ? msg.phase : 'act',
        msg.ok,
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
    if (root) { root.remove(); root = null; bubbleEl = null; ringEl = null; placed = false; }
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
