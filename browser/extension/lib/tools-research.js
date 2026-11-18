// Research workflow tools (2026-11-11 field rounds):
//   paper.resolve / paper.download — DOI → publisher → PDF candidates, with
//     paywall detection and a viewer-tab fallback (ScienceDirect pdfft).
//   pipeline.run / pipeline.status — generic item×steps runner with retries,
//     resumable results, and a delay between items.
//
// All of this runs in the user's browser through the same dispatcher; nothing
// here reads or stores a password.

import { err } from './config.js';
import { sessions } from './tabs.js';
import { evalJs, armTab, waitForLoad } from './actions.js';
import { sleep } from './shield-ctl.js';
import { audit } from './audit.js';
import { redactUrl, truncate, redact } from './redact.js';
import { downloadLedger } from './tools-downloads.js';
import * as challenge from './challenge.js';
import { safeFilename, waitDownload } from './download-util.js';

const RULES = [
  { re: /(^|\.)pubs\.acs\.org$/i, name: 'ACS', pdf: (d) => [`https://pubs.acs.org/doi/pdf/${d}?download=true`, `https://pubs.acs.org/doi/pdf/${d}`] },
  { re: /(^|\.)onlinelibrary\.wiley\.com$/i, name: 'Wiley', pdf: (d) => [`https://onlinelibrary.wiley.com/doi/pdfdirect/${d}?download=true`, `https://onlinelibrary.wiley.com/doi/pdf/${d}?download=true`] },
  // Wiley-hosted society subdomains (Chemistry Europe, AES, ACES...): same
  // pdfdirect contract as the main host, but each subdomain holds its OWN
  // Shibboleth session — a live-validated 2026-11 finding. The candidate list
  // is host-relative so the SSO round-trip lands back on the right subdomain.
  { re: /(^|\.)[a-z-]+\.onlinelibrary\.wiley\.com$/i, name: 'Wiley', pdf: (d, url) => {
    const h = (() => { try { return new URL(url).hostname; } catch { return 'onlinelibrary.wiley.com'; } })();
    return [`https://${h}/doi/pdfdirect/${d}?download=true`, `https://${h}/doi/pdf/${d}?download=true`];
  } },
  { re: /(^|\.)tandfonline\.com$/i, name: 'T&F', pdf: (d) => [`https://www.tandfonline.com/doi/pdf/${d}?download=true`] },
  { re: /(^|\.)pubs\.rsc\.org$/i, name: 'RSC', pdf: (d) => {
    const m = String(d).match(/^10\.1039\/([a-z])(\d)([a-z]{2})(\d+)([a-z])$/i);
    if (!m) return [];
    const year = `202${m[2]}`;
    const tail = `${m[1]}${m[2]}${m[3]}${m[4]}${m[5]}`;
    return [`https://pubs.rsc.org/en/content/articlepdf/${year}/${m[3].toLowerCase()}/${tail}`, `https://pubs.rsc.org/en/content/articlepdf/${year}/${m[3].toLowerCase()}/${tail.toUpperCase()}`];
  } },
  { re: /(^|\.)link\.springer\.com$/i, name: 'Springer', pdf: (d) => [`https://link.springer.com/content/pdf/${d}.pdf`] },
  { re: /(^|\.)nature\.com$/i, name: 'Nature', pdf: (d, url) => [`${String(url).replace(/[?#].*$/, '')}.pdf`] },
  { re: /(^|\.)iopscience\.iop\.org$/i, name: 'IOP', pdf: (d) => [`https://iopscience.iop.org/article/${d}/pdf`] },
  { re: /(^|\.)mdpi\.com$/i, name: 'MDPI', pdf: (d, url) => [`${String(url).replace(/[?#].*$/, '')}/pdf`] },
  { re: /(^|\.)frontiersin\.org$/i, name: 'Frontiers', pdf: (d) => [`https://www.frontiersin.org/articles/${d}/pdf`] },
  { re: /(^|\.)sciencedirect\.com$/i, name: 'Elsevier', pdf: () => [], viewer: true },
  { re: /(^|\.)linkinghub\.elsevier\.com$/i, name: 'Elsevier', pdf: () => [], viewer: true },
  { re: /(^|\.)matec-conferences\.org$/i, name: 'MATEC', pdf: () => [] },
];

function ruleFor(host) {
  return RULES.find((r) => r.re.test(host || '')) || null;
}

function v0Url(tabId) {
  // best-effort current URL without a full extractPaper round-trip
  return new Promise((res) => {
    chrome.tabs.get(tabId, (t) => res(chrome.runtime.lastError ? '' : (t?.url || '')));
  });
}

/**
 * Complete the campus CAS login on the current tab. Focus the username field
 * (that is what materializes Edge's saved-credential fill), verify via
 * casFormState's length bit, click submit with a real trusted click (DOM
 * .click() is ignored by the form's own binding), then solve the slider the
 * page pops. Returns {state:'logged-in'|'need-manual'|'error', detail}.
 */
async function casLogin(tabId) {
  const form = await challenge.callOp(tabId, 'casFormState', {}).catch(() => null);
  if (!form?.present) return { state: 'error', detail: 'no CAS form on this tab' };
  if (!form.username?.visible) return { state: 'error', detail: 'username field not visible' };
  // focus the username field via a trusted coordinate click — twice, with a
  // settle, because the first click may land while the page is still binding
  for (let i = 0; i < 2; i++) {
    await challenge.clickAt(tabId, form.username.x, form.username.y, { role: 'textbox' }).catch(() => {});
    await sleep(2200);
    const again = await challenge.callOp(tabId, 'casFormState', {}).catch(() => null);
    if (again?.pwdlen_audit > 0) break;
  }
  const filled = await challenge.callOp(tabId, 'casFormState', {}).catch(() => null);
  if (!(filled?.pwdlen_audit > 0)) {
    return { state: 'need-manual', detail: 'no saved credential materialized after focus — a human must type the password once' };
  }
  if (!filled.submit?.visible) return { state: 'error', detail: 'submit button not visible' };
  await challenge.clickAt(tabId, filled.submit.x, filled.submit.y, { role: 'button' }).catch(() => {});
  await sleep(4500);
  const det = await challenge.detect(tabId).catch(() => null);
  if (det?.challenge) {
    const s = await challenge.solve(tabId, { mode: 'auto', timeout_ms: 30000 }).catch((e) => ({ state: 'error', error: String(e?.message || e).slice(0, 160) }));
    if (s.state !== 'solved') return { state: 'error', detail: `captcha unsolved (${s.state})` };
  }
  await sleep(4000);
  const after = await challenge.callOp(tabId, 'casFormState', {}).catch(() => null);
  if (after?.present && !after.captcha_visible) {
    const err = String(after.error_text || '');
    return /input|error|失败/i.test(err) ? { state: 'error', detail: err.slice(0, 120) } : { state: 'logged-in', detail: 'form gone' };
  }
  return { state: 'logged-in', detail: 'navigated away from the CAS form' };
}

function absUrl(u, base) {
  try { return new URL(u, base).href; } catch { return null; }
}

async function extractPaper(tabId) {
  return evalJs(tabId, `(() => {
    const meta = (n) => { const m = document.querySelector('meta[name="' + n + '"]'); return m ? m.content : null; };
    const links = [...document.querySelectorAll('a[href]')].map((a) => ({
      t: (a.textContent || '').trim().slice(0, 100), h: a.href,
    })).slice(0, 500);
    const text = (document.body && document.body.innerText ? document.body.innerText : '').slice(0, 20000);
    return {
      url: location.href, title: document.title, ctype: document.contentType || '',
      meta_pdf: meta('citation_pdf_url'), meta_doi: meta('citation_doi'), links, text,
    };
  })()`);
}

function buildCandidates(v, doi) {
  const out = [];
  const seen = new Set();
  // A WebVPN-proxied page keeps the ORIGINAL publisher URLs in meta tags and
  // rule templates; fetching them directly leaves the proxy and loses the
  // institutional authorization. Map same-host URLs back onto the proxy prefix
  // the current page is served from.
  const proxy = (() => {
    // WebVPN products expose proxied pages as /https/<token>/... or
    // /https-443/<token>/... on their own host. Detect the shape, not a host.
    const m = String(v.url || '').match(/^(https?:\/\/[^/]+\/(?:https?|https-\d+)\/[^/]+\/)/);
    return m ? m[1] : null;
  })();
  const toProxy = (a) => {
    if (!proxy || !a) return a;
    if (a.startsWith(proxy)) return a;
    try {
      const u = new URL(a);
      if (/^\/(?:https?|https-\d+)\/[^/]+\//.test(u.pathname)) return a;
      return proxy + u.pathname.replace(/^\/+/, '') + u.search;
    } catch { return a; }
  };
  const add = (u, src) => {
    if (!u) return;
    let a = absUrl(u, v.url);
    if (!a || !/^https?:/i.test(a) || seen.has(a)) return;
    if (/supplement|\.zip|\.docx?|figure|cover|issue/i.test(a)) return;
    a = toProxy(a);
    seen.add(a);
    out.push({ url: a, src });
  };
  if (v.meta_pdf) add(v.meta_pdf, 'meta');
  const rule = ruleFor((() => { try { return new URL(v.url).hostname; } catch { return ''; } })());
  if (rule?.pdf && doi) for (const u of rule.pdf(doi, v.url)) add(u, 'rule');
  for (const l of v.links || []) {
    if (/pdfdirect|articlepdf|\/pdf\/|_pdf(\?|$)|\.pdf(\?|$)|pdfft/i.test(l.h)) add(l.h, 'link');
  }
  return out.slice(0, 10);
}

async function resolvePaper(p) {
  let tabId = p.tab_id;
  let created = false;
  if (tabId == null) {
    if (!sessions.active()) await sessions.start();
    const target = p.url || (p.doi ? `https://doi.org/${p.doi}` : null);
    if (!target) throw err('BAD_PARAMS', 'doi, url or tab_id required');
    const r = await sessions.createTab(target, { active: p.active !== false });
    tabId = r.tab_id;
    created = true;
    await armTab(tabId);
    await waitForLoad(tabId, p.timeout_ms || 30000).catch(() => false);
  }
  let det = await challenge.detect(tabId).catch(() => null);
  let solved = null;
  if (det?.challenge && p.solve !== false) {
    solved = await challenge.solve(tabId, { mode: 'auto', timeout_ms: 30000 }).catch((e) => ({ state: 'error', error: String(e?.message || e).slice(0, 200) }));
    await sleep(600);
    det = await challenge.detect(tabId).catch(() => null);
  }
  // CAS (campus SSO) interposition: the DOI redirect landed on a campus login
  // form instead of the publisher. Edge's credential autofill only materializes
  // after the username field gains FOCUS, so: click username → probe → submit
  // click → the slider pops → solve it. Passwords are never read or typed;
  // pwdlen_audit stays 0 after two focus attempts means no saved credential —
  // surface NEED-MANUAL-LOGIN instead of guessing. Live-validated 2026-11.
  let cas = null;
  if (p.cas_login !== false && /pass\.ujs\.edu\.cn|\/cas\/login/i.test(v0Url(tabId))) {
    cas = await casLogin(tabId).catch((e) => ({ state: 'error', error: String(e?.message || e).slice(0, 200) }));
    if (cas?.state === 'logged-in') {
      await sleep(2500);
      det = await challenge.detect(tabId).catch(() => null);
      if (det?.challenge && p.solve !== false) {
        solved = await challenge.solve(tabId, { mode: 'auto', timeout_ms: 30000 }).catch((e) => ({ state: 'error', error: String(e?.message || e).slice(0, 200) }));
      }
    }
  }
  const v = await extractPaper(tabId);
  const host = (() => { try { return new URL(v.url).hostname; } catch { return ''; } })();
  // the visible host may be the WebVPN proxy; the publisher is the host in
  // citation_pdf_url / citation_doi-bearing page links.
  const realHost = (() => {
    try { if (v.meta_pdf) return new URL(absUrl(v.meta_pdf, v.url)).hostname; } catch { /* ignore */ }
    return host;
  })();
  const rule = ruleFor(realHost) || ruleFor(host);
  const doi = p.doi || v.meta_doi || null;
  const candidates = buildCandidates(v, doi);
  const isPdf = /^application\/pdf$/i.test(v.ctype || '') || /\.pdf(\?|$)/i.test(v.url);
  if (isPdf) candidates.unshift({ url: v.url, src: 'current-tab' });
  const viewer = (() => {
    const l = (v.links || []).find((x) => /\/pdfft|view pdf|pdf download/i.test(`${x.h} ${x.t}`));
    return l ? absUrl(l.h, v.url) : null;
  })();
  const paywall = /(purchase pdf|get access|access through institution|sign in to access|buy article|购买全文|获取全文|institutional access)/i.test(v.text || '');
  return {
    tab_id: tabId, created, page_raw: v.url,
    page: { url: redactUrl(v.url), title: truncate(v.title, 200) },
    publisher: rule?.name || host, doi,
    access: candidates.length ? 'pdf-link' : (paywall ? 'paywalled' : (isPdf ? 'viewer' : 'unknown')),
    paywall,
    candidates,
    viewer,
    challenge: det?.challenge ? { vendor: det.vendor, kind: det.kind } : null,
    solve: solved ? { state: solved.state, verified: solved.verified } : null,
    cas: cas || null,
    probe: { ctype: v.ctype || null, meta_pdf: !!v.meta_pdf, links: (v.links || []).length },
  };
}

function publicPaper(res) {
  const { page_raw, viewer, ...rest } = res;
  return redact({ ...rest, viewer: viewer ? redactUrl(viewer) : null });
}

// ---------------------------------------------------------------- pipeline

const PIPE_KEY = 'cua_pipelines';
const RUNNING = new Map();

function applyItem(value, item) {
  if (typeof value === 'string') {
    if (value === '{{item}}') return item;
    return value.replace(/\{\{\s*item(?:\.([\w.$]+))?\s*\}\}/g, (_, path) => {
      if (!path) return typeof item === 'string' ? item : JSON.stringify(item);
      let cur = item;
      for (const k of path.split('.')) cur = cur == null ? undefined : cur[k];
      return cur == null ? '' : String(cur);
    });
  }
  if (Array.isArray(value)) return value.map((v) => applyItem(v, item));
  if (value && typeof value === 'object') {
    const out = {};
    for (const [k, v] of Object.entries(value)) out[k] = applyItem(v, item);
    return out;
  }
  return value;
}

function summarize(v, max = 1500) {
  try {
    const s = JSON.stringify(v);
    return s && s.length > max ? { truncated: true, head: s.slice(0, max) } : v;
  } catch { return String(v).slice(0, max); }
}

async function loadPipeline(id) {
  try {
    const st = await chrome.storage.session.get(PIPE_KEY);
    return (st?.[PIPE_KEY] || []).find((x) => x.id === id) || null;
  } catch { return null; }
}

async function savePipeline(rec) {
  try {
    const st = await chrome.storage.session.get(PIPE_KEY);
    const arr = (st?.[PIPE_KEY] || []).filter((x) => x.id !== rec.id);
    arr.push({ ...rec, results: (rec.results || []).slice(-200) });
    await chrome.storage.session.set({ [PIPE_KEY]: arr.slice(-10) });
  } catch { /* best-effort */ }
}

// ------------------------------------------------------------- institution

function registerResearch(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('paper.resolve', {
    category: 'sensing',
    timeoutMs: 90000,
    description: 'Open a DOI/URL (or use a tab), clear a challenge if needed, complete a campus CAS login if the redirect landed on one (focus→autofill→submit→slider, no password ever read), and report the publisher, access state and PDF candidates. Candidates come from citation_pdf_url, the page PDF anchors, and per-publisher URL rules (ACS/Wiley incl. society subdomains/T&F/RSC/Springer/Nature/IOP/MDPI/Frontiers/Elsevier viewer+linkinghub/MATEC). Paywall is detected from the page copy; nothing is downloaded here.',
  }, async (p = {}) => {
    const res = await resolvePaper({ ...p, active: p.active !== false });
    audit.log({ action: 'paper.resolve', tabId: res.tab_id, publisher: res.publisher, access: res.access, candidates: res.candidates.length, cas: res.cas?.state });
    return publicPaper(res);
  });

  R('paper.download', {
    category: 'interaction',
    timeoutMs: 240000,
    description: 'Resolve a DOI/URL and download the first candidate that produces a real PDF (mime/pdf filename); non-PDF responses are deleted and the next candidate is tried. Each candidate gets TWO channels: chrome.downloads first, then an in-page blob fetch (carries the tab Referer/CF clearance — beats SERVER_FORBIDDEN on Wiley pdfdirect, %PDF-magic-checked). For ScienceDirect it opens the View PDF viewer tab and downloads the signed URL it lands on. CAS campus-login interposition is handled inside resolve. filename is relative to the browser download directory. Returns the winning download record and every attempt.',
  }, async (p = {}) => {
    const res = await resolvePaper({ ...p, active: p.active !== false });
    const filename = safeFilename(p.filename);
    const tried = [];
    // Blob-fetch channel: an in-page fetch carries the tab's full Referer /
    // CF clearance / Sec-Fetch context that chrome.downloads lacks — the exact
    // gap behind SERVER_FORBIDDEN on Wiley pdfdirect (live-validated 2026-11).
    // Streams through the page as a Blob and re-enters chrome.downloads via a
    // blob: URL, so the file still lands in the normal download dir. %PDF magic
    // is checked because entitlement pages sometimes download as HTML.
    const tryBlob = async (url, src) => {
      let injected = null;
      try {
        injected = await evalJs(res.tab_id, `(async () => {
          try {
            const r = await fetch(${JSON.stringify(url)}, { credentials: 'include' });
            const buf = await r.arrayBuffer();
            const u8 = new Uint8Array(buf.slice(0, 5));
            if (!(u8[0] === 0x25 && u8[1] === 0x50 && u8[2] === 0x44 && u8[3] === 0x46)) {
              const head = new TextDecoder().decode(buf.slice(0, 120)).replace(/\\s+/g, ' ');
              return { ok: false, status: r.status, head };
            }
            const u = URL.createObjectURL(new Blob([buf], { type: 'application/pdf' }));
            return { ok: true, blobUrl: u, bytes: buf.byteLength };
          } catch (e) { return { ok: false, error: String(e).slice(0, 160) }; }
        })()`);
      } catch (e) {
        tried.push({ url: redactUrl(url), src: `${src}/blob`, error: String(e?.message || e).slice(0, 200) });
        return null;
      }
      const v = injected?.value || injected;
      if (!v?.ok) {
        tried.push({ url: redactUrl(url), src: `${src}/blob`, state: 'not-pdf', head: String(v?.head || v?.error || '').slice(0, 120) });
        return null;
      }
      let id = null;
      try {
        id = await chrome.downloads.download({ url: v.blobUrl, saveAs: false, ...(filename ? { filename } : {}) });
        downloadLedger.claim(id, res.tab_id);
        const rec = await waitDownload(id, p.timeout_ms || 60000, sleep);
        const ok = rec.state === 'complete' && (/pdf/i.test(rec.mime || '') || /\.pdf(\?|$)/i.test(rec.filename || ''));
        tried.push({ url: redactUrl(url), src: `${src}/blob`, state: rec.state, mime: rec.mime, bytes: v.bytes, ok });
        if (ok) return rec;
        if (rec.state === 'complete') {
          await chrome.downloads.removeFile(id).catch(() => {});
          await chrome.downloads.erase({ id }).catch(() => {});
        }
        return null;
      } catch (e) {
        tried.push({ url: redactUrl(url), src: `${src}/blob`, error: String(e?.message || e).slice(0, 200) });
        return null;
      } finally {
        if (id) await chrome.downloads.erase({ id }).catch(() => {});
        setTimeout(() => { try { URL.revokeObjectURL(v.blobUrl); } catch { /* page gone */ } }, 60000).unref?.();
      }
    };
    const tryOne = async (url, src) => {
      let id;
      try {
        id = await chrome.downloads.download({ url, saveAs: false, ...(filename ? { filename } : {}) });
      } catch (e) {
        tried.push({ url: redactUrl(url), src, error: String(e?.message || e).slice(0, 200) });
        return null;
      }
      downloadLedger.claim(id, res.tab_id);
      const rec = await waitDownload(id, p.timeout_ms || 60000, sleep);
      const ok = rec.state === 'complete' && (/pdf/i.test(rec.mime || '') || /\.pdf(\?|$)/i.test(rec.filename || ''));
      tried.push({ url: redactUrl(url), src, state: rec.state, mime: rec.mime, ok });
      if (!ok && rec.state === 'complete') {
        await chrome.downloads.removeFile(id).catch(() => {});
        await chrome.downloads.erase({ id }).catch(() => {});
      }
      return ok ? rec : null;
    };
    for (const c of res.candidates) {
      let rec = await tryOne(c.url, c.src);
      if (!rec) rec = await tryBlob(c.url, c.src);
      if (rec) {
        audit.log({ action: 'paper.download', tabId: res.tab_id, doi: res.doi, ok: true, src: rec.__src || c.src });
        return redact({ ...publicPaper(res), download: rec, tried });
      }
    }
    if (res.viewer) {
      let nt = null;
      try {
        nt = await chrome.tabs.create({ url: res.viewer, openerTabId: res.tab_id, active: false });
        await waitForLoad(nt.id, 30000).catch(() => false);
        await sleep(1500);
        const final = (await chrome.tabs.get(nt.id).catch(() => null))?.url || res.viewer;
        const rec = await tryOne(final, 'viewer');
        if (rec) {
          audit.log({ action: 'paper.download', tabId: res.tab_id, doi: res.doi, ok: true, src: 'viewer' });
          return redact({ ...publicPaper(res), download: rec, tried });
        }
      } finally {
        if (nt?.id) chrome.tabs.remove(nt.id).catch(() => {});
      }
    }
    audit.log({ action: 'paper.download', tabId: res.tab_id, doi: res.doi, ok: false, tried: tried.length });
    return redact({
      ...publicPaper(res), state: 'failed', tried,
      note: res.access === 'paywalled'
        ? 'the page shows a paywall; establish an institutional session (institution.carsi / institution.webvpn) and retry with that tab'
        : 'no candidate produced a PDF; inspect the tab and the tried list',
    });
  });

  R('pipeline.run', {
    category: 'orchestration',
    timeoutMs: 900000,
    description: 'Run steps[] for every item[] with {{item}} / {{item.field}} substitution, per-step retries, a delay between items, and results persisted for resume (pass the same job_id). Each step is a normal tool call: {tool:"wait_for", params:{...}}. pipeline.run/batch cannot be nested. Returns {job_id, completed, failed, results} and writes a compact record for pipeline.status.',
  }, async (p = {}, ctx) => {
    const items = Array.isArray(p.items) ? p.items : null;
    const steps = Array.isArray(p.steps) ? p.steps : null;
    if (!items || !steps || !steps.length) throw err('BAD_PARAMS', 'items[] and steps[] required');
    if (steps.some((s) => /^(pipeline\.run|batch)$/.test(String(s.tool || '')))) throw err('BAD_PARAMS', 'pipeline.run and batch cannot be nested');
    const jobId = p.job_id || `pl-${Math.random().toString(36).slice(2, 8)}${Date.now().toString(36).slice(-4)}`;
    const prev = await loadPipeline(jobId);
    const done = new Set(prev?.done || []);
    const results = prev?.results || [];
    const retries = Math.max(0, Math.min(Number(p.retries) || 0, 3));
    const delay = Math.max(0, Math.min(Number(p.delay_ms) || 0, 10000));
    const resume = p.resume !== false;
    RUNNING.set(jobId, { startedAt: Date.now(), items: items.length });
    let completed = 0;
    let failed = 0;
    for (let i = 0; i < items.length; i++) {
      if (ctx?.signal?.aborted) throw err('CANCELLED', 'cancelled by agent');
      if (resume && done.has(i)) {
        const old = results.find((x) => x.index === i);
        if (old?.ok) completed++; else failed++;
        continue;
      }
      const item = items[i];
      const rec = { index: i, item: summarize(item, 300), ok: true, steps: [], error: null };
      for (const step of steps) {
        if (ctx?.signal?.aborted) throw err('CANCELLED', 'cancelled by agent');
        const params = applyItem(step.params || {}, item);
        let reply = null;
        let lastErr = null;
        for (let attempt = 0; attempt <= retries; attempt++) {
          try {
            reply = await dispatcher.handle({
              id: `${jobId}-${i}-${Math.random().toString(36).slice(2, 6)}`,
              method: `tool.${step.tool}`,
              params,
            });
          } catch (e) {
            reply = { error: { code: 'DISPATCH_ERROR', message: String(e?.message || e).slice(0, 300) } };
          }
          lastErr = reply?.error || null;
          if (!lastErr) break;
          if (attempt < retries) await sleep(400);
        }
        if (lastErr) {
          rec.ok = false;
          rec.error = lastErr;
          rec.steps.push({ tool: step.tool, ok: false, error: lastErr });
          break;
        }
        rec.steps.push({ tool: step.tool, ok: true, result: summarize(reply?.result) });
      }
      done.add(i);
      results.push(rec);
      if (rec.ok) completed++; else failed++;
      await savePipeline({ id: jobId, items: items.length, done: [...done], results, updatedAt: Date.now() });
      if (delay) await sleep(delay);
    }
    RUNNING.delete(jobId);
    await savePipeline({ id: jobId, items: items.length, done: [...done], results, updatedAt: Date.now() });
    audit.log({ action: 'pipeline.run', job_id: jobId, completed, failed });
    return {
      job_id: jobId, completed, failed, total: items.length,
      results: results.map((r) => ({ index: r.index, ok: r.ok, error: r.error, steps: r.steps.map((s) => ({ tool: s.tool, ok: s.ok, error: s.error, result: s.result })) })),
    };
  });

  R('pipeline.status', {
    category: 'session',
    description: 'Read the persisted record of a pipeline.run job: {job_id, items, done[], results[], updatedAt}. Use it to resume after a tool timeout.',
  }, async (p = {}) => {
    if (!p.job_id) throw err('BAD_PARAMS', 'job_id required');
    const rec = await loadPipeline(String(p.job_id));
    if (!rec) throw err('NOT_FOUND', `no pipeline job "${p.job_id}" in this browser session`);
    return redact(rec);
  });


}

export function registerResearchTools(dispatcher) {
  registerResearch(dispatcher);
}
