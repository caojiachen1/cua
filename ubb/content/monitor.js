// Foreign-extension iframe monitor: neutralizes chrome-extension:// iframes
// from OTHER extensions that make chrome.debugger.attach fail on a tab
// (the same foreign-iframe problem breaks debugger attach for any extension).
// Runs passively; armed per the guard mode read from
// chrome.storage.local.ubb_guard_mode (Codex absorb wave, plan §3):
//   "always"  → arm unconditionally at startup, UBB_MONITOR is ignored
//   "off"     → never arm, UBB_MONITOR is ignored
//   "session" → the historical contract: arm only when UBB_MONITOR says so
// Unknown values fall back to "session"; onChanged hot-switches live
// (off disarms/stops the observer, always arms immediately).

(() => {
  if (window.__ubbMonitor) return;
  window.__ubbMonitor = true;

  let armed = false;
  let observer = null;
  const OWN = location.protocol === 'chrome-extension:' ? location.host : chrome.runtime?.id;

  const GUARD_MODES = new Set(['session', 'always', 'off']);
  const normMode = (v) => (GUARD_MODES.has(v) ? v : 'session');
  let mode = 'session';

  function neutralize(frame) {
    try {
      const src = frame.getAttribute('src') || '';
      let host = '';
      try {
        host = new URL(src, location.href).host;
      } catch {}
      if (src.startsWith('about:blank') || !src) return;
      if (!/^(chrome|edge)-extension:$/i.test(new URL(src, location.href).protocol)) return;
      if (host && host === OWN) return;
      frame.setAttribute('data-ubb-neutralized', src);
      frame.setAttribute('src', 'about:blank');
      const srcdoc = frame.getAttribute('srcdoc');
      if (srcdoc) frame.setAttribute('srcdoc', '');
    } catch {}
  }

  function scanAll(root) {
    try {
      root.querySelectorAll?.('iframe').forEach(neutralize);
      // shadow roots
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
      let el;
      while ((el = walker.nextNode())) {
        if (el.shadowRoot) scanAll(el.shadowRoot);
      }
    } catch {}
  }

  function arm(on) {
    armed = on;
    if (on) {
      if (observer) return; // already armed: a hot-switch or repeat message must not stack observers
      scanAll(document);
      observer = new MutationObserver((muts) => {
        if (!armed) return;
        for (const m of muts) {
          if (m.type === 'attributes') {
            if (m.target.nodeName === 'IFRAME') neutralize(m.target);
          } else {
            for (const n of m.addedNodes) {
              if (n.nodeName === 'IFRAME') neutralize(n);
              else if (n.querySelectorAll) n.querySelectorAll?.('iframe').forEach(neutralize);
            }
          }
        }
      });
      observer.observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ['src', 'srcdoc'] });
    } else if (observer) {
      observer.disconnect();
      observer = null;
    }
  }

  function applyMode(next) {
    mode = normMode(next);
    if (mode === 'always') {
      arm(true); // 无条件武装，不等 UBB_MONITOR
    } else if (mode === 'off') {
      arm(false); // 停 observer；随后的 UBB_MONITOR 一律忽略
    }
    // "session": 现状 —— 武装完全由 UBB_MONITOR 驱动，热切换不清当前状态
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg?.type === 'UBB_MONITOR') {
      if (mode === 'off') {
        sendResponse({ ok: true, armed: false, mode, note: 'guard off — UBB_MONITOR ignored' });
        return;
      }
      if (mode === 'always') {
        sendResponse({ ok: true, armed: true, mode, note: 'guard always — already armed, UBB_MONITOR ignored' });
        return;
      }
      arm(!!msg.on);
      sendResponse({ ok: true, armed: !!msg.on, mode });
    }
  });

  try {
    chrome.storage.local.get('ubb_guard_mode', (res) => {
      try {
        if (chrome.runtime.lastError) return; // 读不到就维持 session 档
        applyMode(res?.ubb_guard_mode);
      } catch {}
    });
    chrome.storage.onChanged.addListener((changes, area) => {
      try {
        if (area !== 'local' || !('ubb_guard_mode' in changes)) return;
        applyMode(changes.ubb_guard_mode.newValue);
      } catch {}
    });
  } catch { /* storage 不可用：维持 session 档现状 */ }
})();
