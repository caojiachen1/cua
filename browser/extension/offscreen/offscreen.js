// Clipboard bridge: execCommand works in extension pages with the
// clipboardRead/clipboardWrite permissions (the documented offscreen pattern).
// Binary ops go through navigator.clipboard (ClipboardItem) — a browser that
// refuses clipboard access to an unfocused extension page reports its own
// error; we never fake an ok.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type !== 'CUA_CLIPBOARD') return;
  (async () => {
    try {
      if (msg.op === 'write_bin') {
        if (typeof ClipboardItem === 'undefined') return { ok: false, error: 'ClipboardItem is not available in this browser build' };
        const bin = atob(String(msg.data_b64 || ''));
        const bytes = new Uint8Array(bin.length);
        for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
        const mime = String(msg.mime || 'image/png');
        await navigator.clipboard.write([new ClipboardItem({ [mime]: new Blob([bytes], { type: mime }) })]);
        return { ok: true, bytes: bytes.length };
      }
      if (msg.op === 'page_info') {
        return { ok: true, page: location.pathname, has_clipboard: typeof ClipboardItem !== 'undefined' };
      }
      if (msg.op === 'read_bin') {
        const items = await navigator.clipboard.read();
        for (const item of items) {
          const mime = item.types.find((t) => /^image\//.test(t));
          if (mime) {
            const blob = await item.getType(mime);
            const buf = new Uint8Array(await blob.arrayBuffer());
            let s = '';
            const CH = 0x8000;
            for (let i = 0; i < buf.length; i += CH) {
              s += String.fromCharCode.apply(null, buf.subarray(i, i + CH));
            }
            return { ok: true, mime, data_b64: btoa(s), bytes: buf.length };
          }
        }
        return { ok: false, error: 'no image on the clipboard (types: ' + items.flatMap((i) => i.types).join(', ') + ')' };
      }
      const ta = document.getElementById('ta');
      ta.value = '';
      if (msg.op === 'write') {
        ta.value = String(msg.text ?? '');
        ta.focus();
        ta.select();
        ta.setSelectionRange(0, ta.value.length);
        const ok = document.execCommand('copy');
        return { ok, chars: ta.value.length };
      }
      ta.focus();
      ta.select();
      const ok = document.execCommand('paste');
      return { ok, text: ta.value };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  })().then(sendResponse);
  return true; // async sendResponse
});
