// Tab badge (Codex absorb wave, plan §7): composes the session mark onto the
// tab's favicon — deliverable = green dot + white check, handoff = amber dot +
// white pause bars, mark:null restores the original icon. Top frame only; the
// background re-sends CUA_BADGE on webNavigation.onCompleted, so a fresh
// document re-applies its mark without this file having to read storage.
//
// Known boundary: an SPA that swaps its own <link rel~=icon> while a mark is
// shown can win the tab strip until the next CUA_BADGE (documented limitation).

(() => {
  if (window.__cuaTabBadge) return;
  window.__cuaTabBadge = true;

  let isTop = true;
  try { isTop = window.top === window; } catch { isTop = false; }
  if (!isTop) return;

  const MARKS = {
    deliverable: { color: '#22c55e', glyph: 'check' },
    handoff: { color: '#f59e0b', glyph: 'pause' },
  };

  let current = undefined; // undefined = never marked in this document
  let originals = null;    // [{link, href}] captured before the first override
  let created = [];        // icon <link> elements this file added itself
  let gen = 0;             // invalidates in-flight canvas work on a newer mark

  const iconLinks = () => [...document.querySelectorAll('link[rel~="icon"]')];

  function capture() {
    if (originals) return originals;
    originals = iconLinks().map((link) => ({ link, href: link.getAttribute('href') }));
    return originals;
  }

  function applyBadge(dataUrl) {
    const list = capture();
    for (const { link } of list) link.setAttribute('href', dataUrl);
    if (!list.length) {
      // page declares no icon link (e.g. bare /favicon.ico default): add one
      const link = document.createElement('link');
      link.rel = 'icon';
      link.setAttribute('href', dataUrl);
      (document.head || document.documentElement).appendChild(link);
      created.push(link);
    }
  }

  function restore() {
    gen++;
    if (!originals) return;
    for (const { link, href } of originals) {
      if (href === null) link.removeAttribute('href');
      else link.setAttribute('href', href);
    }
    for (const link of created) link.remove();
    created = [];
    originals = null; // re-capture on the next mark (an SPA may have swapped icons meanwhile)
  }

  function withBadge(ctx, spec) {
    // bottom-right dot: dark hairline (visible on light favicons), white ring,
    // colored core, then the glyph
    ctx.beginPath(); ctx.arc(23, 23, 9.5, 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(15,23,42,.55)'; ctx.lineWidth = 2.5; ctx.stroke();
    ctx.beginPath(); ctx.arc(23, 23, 8, 0, Math.PI * 2);
    ctx.strokeStyle = '#ffffff'; ctx.lineWidth = 2.5; ctx.stroke();
    ctx.beginPath(); ctx.arc(23, 23, 6.5, 0, Math.PI * 2);
    ctx.fillStyle = spec.color; ctx.fill();
    ctx.strokeStyle = '#ffffff'; ctx.lineWidth = 2.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    if (spec.glyph === 'check') {
      ctx.beginPath(); ctx.moveTo(19.8, 23.2); ctx.lineTo(22.2, 25.6); ctx.lineTo(26.6, 20.2); ctx.stroke();
    } else {
      ctx.beginPath(); ctx.moveTo(21, 20.2); ctx.lineTo(21, 25.8); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(25.2, 20.2); ctx.lineTo(25.2, 25.8); ctx.stroke();
    }
  }

  // 32×32 compose; toDataURL throws on a tainted canvas (cross-origin favicon)
  // — the caller falls back to a solid base on that.
  function compose(mark, img) {
    const c = document.createElement('canvas');
    c.width = 32; c.height = 32;
    const ctx = c.getContext('2d');
    if (img) {
      try { ctx.drawImage(img, 0, 0, 32, 32); } catch { /* keep blank under the badge */ }
    } else {
      ctx.fillStyle = '#0f172a';
      ctx.fillRect(0, 0, 32, 32);
    }
    withBadge(ctx, MARKS[mark]);
    return c.toDataURL('image/png');
  }

  function setMark(mark) {
    const myGen = ++gen;
    const finish = (img) => {
      if (myGen !== gen) return; // a newer mark / restore superseded this draw
      let url = null;
      try { url = compose(mark, img); } catch { /* tainted or canvas refused */ }
      if (!url) {
        try { url = compose(mark, null); } catch { return; } // no canvas: keep the original favicon
      }
      applyBadge(url);
    };
    // try the page's own favicons first (anonymous CORS keeps the canvas
    // clean); any failure or timeout falls through to the solid base
    const urls = capture().map((o) => o.href).filter((h) => !!h).slice(0, 4);
    let idx = 0;
    const attempt = () => {
      if (idx >= urls.length) return finish(null);
      const url = urls[idx++];
      const img = new Image();
      let done = false;
      const timer = setTimeout(() => { if (!done) { done = true; attempt(); } }, 1500);
      img.onload = () => { if (done) return; done = true; clearTimeout(timer); finish(img); };
      img.onerror = () => { if (done) return; done = true; clearTimeout(timer); attempt(); };
      img.crossOrigin = 'anonymous';
      img.src = url;
    };
    attempt();
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg?.type !== 'CUA_BADGE') return;
    const mark = msg.mark === null ? null : msg.mark;
    if (mark !== null && !(mark in MARKS)) {
      sendResponse({ ok: false, error: 'BAD_MARK', mark: msg.mark });
      return;
    }
    if (mark === current) {
      // idempotent: the same mark twice must not re-compose or double-restore
      sendResponse({ ok: true, mark, badge_set: mark !== null, unchanged: true });
      return;
    }
    current = mark;
    if (mark === null) restore();
    else setMark(mark);
    sendResponse({ ok: true, mark, badge_set: mark !== null });
  });
})();
