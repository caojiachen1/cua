// Lightweight page probe: one cheap read that answers "did this page change?"
// for SPA work (pagination, tab switches, AJAX lists) without a full snapshot.
//
// doc_id is minted per document, so a navigation is distinguishable from a
// re-render; text_hash covers innerText changes that keep the URL.

export async function pageProbe(tabId, evalJs) {
  return evalJs(tabId, `(() => {
    if (!window.__cua_probe_doc) window.__cua_probe_doc = Math.random().toString(36).slice(2, 10);
    const t = (document.body && document.body.innerText ? document.body.innerText : '').slice(0, 50000);
    let h = 2166136261;
    for (let i = 0; i < t.length; i++) { h ^= t.charCodeAt(i); h = Math.imul(h, 16777619); }
    return {
      url: location.href,
      title: document.title,
      doc_id: window.__cua_probe_doc,
      text_hash: (h >>> 0).toString(16),
      text_len: t.length,
      first_text: t.trim().slice(0, 120),
      ready: document.readyState,
    };
  })()`);
}
