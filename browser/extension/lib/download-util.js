// Shared download helpers (extension side): wait for a chrome.downloads item
// and sanitize a caller-supplied filename, which chrome.downloads treats as a
// path RELATIVE to the browser download directory.

export function safeFilename(name) {
  if (!name) return null;
  const s = String(name).replace(/\\/g, '/').replace(/^\/+/, '');
  if (!s || s.includes('..') || s.length > 200) {
    throw Object.assign(new Error('filename must be a relative path under the download directory (no leading /, no ..)'), { code: 'BAD_PARAMS' });
  }
  return s;
}

export async function waitDownload(id, timeoutMs, sleep) {
  const wait = sleep || ((ms) => new Promise((r) => setTimeout(r, ms)));
  const until = Date.now() + Math.min(Math.max(Number(timeoutMs) || 60000, 500), 300000);
  for (;;) {
    const [d] = await chrome.downloads.search({ id });
    if (d && (d.state === 'complete' || d.state === 'interrupted')) {
      return {
        download_id: id, state: d.state, filename: d.filename || null, url: d.url || '',
        mime: d.mime || null, bytes: d.fileSize || d.bytesReceived || null, error: d.error || null,
      };
    }
    if (Date.now() >= until) {
      return { download_id: id, state: d?.state || 'unknown', filename: d?.filename || null, mime: d?.mime || null, bytes: d?.bytesReceived || null, pending: true };
    }
    await wait(250);
  }
}
