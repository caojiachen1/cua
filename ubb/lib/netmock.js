// Request interception (CDP Fetch domain): per-tab pause/fulfill rules so the
// agent can stub an API, force an error page, or block a tracker — without a
// proxy. Rules are per tab and die with the debugger session (a navigation
// kills the session on this build, and with it the interception — net.mock.list
// says so instead of pretending a mock survived).
//
// Safety rails: a paused request with NO matching rule is always continued
// immediately (a stale enablement must never wedge a page), catch-all patterns
// are refused at the tool boundary, and a failed fulfill/continue drops the
// tab's rules rather than leaving the wire hung.

import { cdp } from './cdp.js';
import { err } from './config.js';

let netmockSeq = 0;
const tabs = new Map(); // tabId → Map(ruleId → rule)

cdp.eventHooks.add((tabId, method, params) => {
  if (method !== 'Fetch.requestPaused') return;
  void handlePaused(tabId, params);
});

cdp.forgetHooks.add((tabId) => tabs.delete(tabId));

// glob → regex: only '*' is a wildcard, everything else is literal
function globToRegExp(pattern) {
  const esc = pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
  return new RegExp('^' + esc + '$', 'i');
}

function rulesOf(tabId) {
  return tabs.get(tabId) || new Map();
}

function matchRule(ruleMap, request) {
  const url = request?.url || '';
  const method = request?.method || '';
  for (const r of ruleMap.values()) {
    if (r.method && r.method.toUpperCase() !== method.toUpperCase()) continue;
    if (r.re.test(url)) return r;
  }
  return null;
}

async function handlePaused(tabId, params) {
  const ruleMap = rulesOf(tabId);
  const rule = matchRule(ruleMap, params.request);
  try {
    if (!rule) {
      await cdp.send(tabId, 'Fetch.continueRequest', { requestId: params.requestId });
      return;
    }
    rule.hits = (rule.hits || 0) + 1;
    if (rule.once && rule.hits > 1) {
      // fired once already: this and later hits pass through untouched
      await cdp.send(tabId, 'Fetch.continueRequest', { requestId: params.requestId });
      return;
    }
    const headers = Object.entries(rule.headers || {}).map(([name, value]) => ({ name, value: String(value) }));
    if (rule.content_type) headers.push({ name: 'Content-Type', value: rule.content_type });
    await cdp.send(tabId, 'Fetch.fulfillRequest', {
      requestId: params.requestId,
      responseCode: rule.status || 200,
      ...(headers.length ? { responseHeaders: headers } : {}),
      ...(rule.body_b64 ? { body: rule.body_b64 } : {}),
    });
  } catch (e) {
    // the debugger session is gone (navigation/detach): the rules are dead too
    dropTab(tabId);
  }
}

function dropTab(tabId) {
  tabs.delete(tabId);
}

/** Re-arm Fetch interception with the tab's CURRENT rules (disable → enable:
 *  a second bare Fetch.enable on a live session is not a pattern replacement). */
async function rearm(tabId) {
  await cdp.ensure(tabId);
  const rules = rulesOf(tabId);
  const list = [...rules.values()];
  if (!list.length) return;
  await cdp.send(tabId, 'Fetch.disable').catch(() => {});
  await cdp.send(tabId, 'Fetch.enable', {
    patterns: list.map((r) => ({ urlPattern: r.url_pattern, ...(r.method ? { method: r.method } : {}) })),
  }).catch((e) => {
    dropTab(tabId);
    throw err('MOCK_ENABLE_FAILED', `Fetch.enable refused: ${String(e?.message || e).slice(0, 140)}`);
  });
}

/**
 * Add a rule. opts: { url_pattern, method?, status?=200, body?, content_type?,
 * headers?, once? }
 * Body may be a string (encoded here) or {b64: '...'} for binary payloads.
 */
export async function setMock(tabId, opts) {
  const pattern = String(opts.url_pattern || '').trim();
  if (!pattern) throw err('BAD_PARAMS', 'url_pattern is required and must be a glob like "https://api.example.com/v1/*" — an empty pattern would pause every request on the tab');
  if (/^\*+$/.test(pattern)) throw err('BAD_PARAMS', 'refusing a catch-all pattern ("*") — scope the mock to the URLs you actually need');
  let body_b64 = null;
  if (typeof opts.body === 'string' && opts.body.length) {
    // UTF-8 → base64 without deprecated escape(): TextEncoder covers every
    // language a mocked page can answer with
    const bytes = new TextEncoder().encode(opts.body);
    let bin = '';
    for (const b of bytes) bin += String.fromCharCode(b);
    body_b64 = btoa(bin);
  } else if (opts.body && typeof opts.body === 'object' && typeof opts.body.b64 === 'string') {
    body_b64 = opts.body.b64;
  }
  let rules = tabs.get(tabId);
  if (!rules) {
    rules = new Map();
    tabs.set(tabId, rules);
  }
  const id = `m${++netmockSeq}`;
  rules.set(id, {
    id,
    url_pattern: pattern,
    re: globToRegExp(pattern),
    method: opts.method ? String(opts.method).toUpperCase() : null,
    status: Number(opts.status) || 200,
    body_b64,
    content_type: opts.content_type || null,
    headers: opts.headers && typeof opts.headers === 'object' ? opts.headers : null,
    once: opts.once === true,
    hits: 0,
  });
  await rearm(tabId);
  return { id, url_pattern: pattern, status: Number(opts.status) || 200, note: 'pauses matching requests on this tab only; the mock dies with the debugger session (a navigation kills it) — re-set it after navigating' };
}

export function listMocks(tabId) {
  const out = [];
  for (const r of rulesOf(tabId).values()) {
    out.push({ id: r.id, url_pattern: r.url_pattern, method: r.method, status: r.status, hits: r.hits || 0, once: r.once || false, content_type: r.content_type || undefined });
  }
  return out;
}

export async function clearMocks(tabId, id = null) {
  const rules = tabs.get(tabId);
  if (!rules) return { cleared: 0, disabled: false };
  let cleared = 0;
  if (id) {
    cleared = rules.delete(String(id)) ? 1 : 0;
  } else {
    cleared = rules.size;
    rules.clear();
  }
  if (rules.size === 0) {
    await cdp.send(tabId, 'Fetch.disable').catch(() => {});
    tabs.delete(tabId);
    return { cleared, disabled: true };
  }
  await rearm(tabId);
  return { cleared, disabled: false };
}

export function dropTabMocks(tabId) {
  dropTab(tabId);
}
