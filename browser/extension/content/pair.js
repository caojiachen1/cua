// Self-pairing for profiles launched by cua with --load-extension.
//
// The daemon's /__cua_pair page carries only a single-use nonce in its URL.
// This content script forwards that nonce to the background, which asks the
// daemon for the credentials, so a hostile local page cannot point the
// extension at a token of its own.
(() => {
  try {
    if (window.top !== window) return;
    const u = new URL(location.href);
    if (u.pathname !== '/__cua_pair') return;
    const nonce = u.searchParams.get('nonce');
    if (!nonce) return;

    let tries = 0;
    const tick = () => {
      tries += 1;
      chrome.runtime.sendMessage({ type: 'CUA_PAIR_PAGE', nonce }, (r) => {
        if (chrome.runtime.lastError) return; // background not ready yet
        if (r && r.ok) return;
        if (tries < 12) setTimeout(tick, 1200);
      });
    };
    setTimeout(tick, 400);
  } catch (e) {
    /* never break the page */
  }
})();
