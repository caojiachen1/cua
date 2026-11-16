// UBB REPL — compound browser actions in ONE daemon call.
//
// The per-tool round trip (snapshot → pick a ref → click → snapshot again)
// costs a model round trip per step, which is exactly the wrong shape for
// "iterate 40 rows, extract one field each, verify every step". This module
// runs agent-authored JavaScript inside a node:vm sandbox with a persistent
// per-session scope and a small tool API; every tool call inside the sandbox
// goes through the SAME dispatch path (slot routing, vault placeholder rules,
// extension-side confirm/audit) as a normal tool invocation — the REPL is a
// loop over the tool registry, never a bypass of it.
//
// Boundaries, stated honestly:
//  - node:vm is an isolation convenience, not a security boundary. The daemon
//    already runs on the user's machine with the user's rights, and REPL code
//    is authored by the agent the user is driving. No fs / no require / no
//    process: the sandbox has the tool API and nothing else.
//  - `{{secret:NAME}}` inside REPL code is refused (SECRET_SCOPE): placeholders
//    substitute ONLY inside fill/fill_form values, and that rule holds when
//    those values arrive via the sandbox's fill() too.
//  - A timed-out run abandons its promise; an in-flight tool call may still
//    land afterwards. The result says so. Console output from such stragglers
//    stays in the session history (S and history read on the next call).

import vm from 'node:vm';

const MAX_CODE_CHARS = 100_000;
const MAX_OUTPUT_CHARS = 16_000;
const MAX_SESSIONS = 4;
const SESSION_TTL_MS = 10 * 60_000;
const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_TIMEOUT_MS = 120_000;
const MAX_CONSOLE_LINES = 400;

const sessionsMap = new Map(); // session_id → session

function evictIdle() {
  const now = Date.now();
  for (const [id, s] of sessionsMap) {
    if (now - s.lastUsed > SESSION_TTL_MS) sessionsMap.delete(id);
  }
}

function cap(text, max = MAX_OUTPUT_CHARS) {
  const t = String(text ?? '');
  return t.length <= max ? t : t.slice(0, max) + `\n[output truncated at ${max} chars]`;
}

function stringify(v) {
  if (typeof v === 'string') return v;
  try {
    return JSON.stringify(v, (_k, val) => (typeof val === 'bigint' ? String(val) : val), 0) ?? String(v);
  } catch {
    return String(v);
  }
}

/**
 * repl.run / repl.reset. deps: { callExtension(name, method, params, ms),
 * prepareForward(method, params), log }
 */
export async function replMethod(agent, msg, deps) {
  const m = msg.method;
  if (m === 'repl.reset') {
    const id = String(msg.params?.session_id || 'default');
    const had = sessionsMap.delete(id);
    agent.send({ id: msg.id, result: { reset: true, session_id: id, existed: had } });
    return;
  }
  // repl.run
  const code = String(msg.params?.code ?? '');
  if (!code.trim()) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'code is required — the JavaScript to run in the sandbox' } });
    return;
  }
  if (code.length > MAX_CODE_CHARS) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: `code is ${code.length} chars — the cap is ${MAX_CODE_CHARS}` } });
    return;
  }
  if (/\{\{secret:/.test(code)) {
    agent.send({
      id: msg.id,
      error: { code: 'SECRET_SCOPE', message: '{{secret:NAME}} placeholders never substitute inside REPL code — put them in the VALUE of fill()/fillForm() parameters, which pass through the same vault gate as the plain tools' },
    });
    return;
  }
  const timeoutMs = Math.min(Math.max(Number(msg.params?.timeout_ms) || DEFAULT_TIMEOUT_MS, 1000), MAX_TIMEOUT_MS);
  const wanted = msg.params?.browser ? String(msg.params.browser) : null;
  const sessionId = /^[a-z0-9_-]{1,32}$/i.test(String(msg.params?.session_id || '')) ? String(msg.params.session_id) : 'default';

  evictIdle();
  let s = sessionsMap.get(sessionId);
  if (s && wanted && s.browser && s.browser !== wanted) {
    // a session is bound to the slot it first called into; switching slots mid-
    // session would silently move every later call — refuse and say why
    agent.send({
      id: msg.id,
      error: { code: 'SESSION_SLOT_CONFLICT', message: `session ${JSON.stringify(sessionId)} is bound to slot ${JSON.stringify(s.browser)}; pass browser:${JSON.stringify(s.browser)}, reset it with repl.reset {session_id}, or use a different session_id` },
    });
    return;
  }
  if (!s) {
    s = makeSession(sessionId, wanted, deps);
    sessionsMap.set(sessionId, s);
  } else if (!s.browser && wanted) {
    // an unbound session adopts the first slot it is ever given — after that
    // the conflict rule above keeps every later call on the same slot
    s.browser = wanted;
  }
  s.lastUsed = Date.now();
  const browser = s.browser;
  const t0 = Date.now();
  const startCalls = s.calls;
  const startConsole = s.consoleLines.length;

  let outcome;
  try {
    const script = new vm.Script(`(async () => {\n${code}\n})()`, { filename: `ubb-repl:${sessionId}` });
    const promise = script.runInContext(s.ctx, { timeout: 4000 });
    const result = await Promise.race([
      promise.then((v) => ({ ok: true, value: v }), (e) => ({ ok: false, error: e })),
      new Promise((resolve) => setTimeout(() => resolve({ timeout: true }), timeoutMs)),
    ]);
    if (result.timeout) {
      outcome = {
        timed_out: true,
        note: `abandoned after ${timeoutMs}ms — a pending tool call may still complete later; read S and tab state before re-running`,
      };
    } else if (result.ok) {
      outcome = { value: stringifyValue(result.value) };
    } else {
      outcome = { error: String(result.error?.message || result.error) };
    }
  } catch (e) {
    // syntax errors and sandbox rule violations (no require, no process, …)
    outcome = { error: String(e?.message || e) };
  }

  const calls = s.calls - startCalls;
  const outLines = s.consoleLines.slice(startConsole);
  const body = {
    session: sessionId,
    browser,
    calls,
    elapsed_ms: Date.now() - t0,
    output: cap(outLines.join('\n')),
    ...outcome,
  };
  deps.log(`repl.run session=${sessionId} slot=${browser} calls=${calls} elapsed=${body.elapsed_ms}ms ${outcome.error ? 'ERROR' : outcome.timed_out ? 'TIMEOUT' : 'ok'}`);
  agent.send({ id: msg.id, result: body });
}

function makeSession(sessionId, browser, deps) {
  const st = {
    id: sessionId,
    browser: browser || null,
    lastUsed: Date.now(),
    calls: 0,
    consoleLines: [],
  };
  const line = (level, args) => {
    st.consoleLines.push(`${level} ${args.map((a) => stringify(a)).join(' ')}`);
    if (st.consoleLines.length > MAX_CONSOLE_LINES) {
      st.consoleLines.splice(0, st.consoleLines.length - MAX_CONSOLE_LINES);
    }
  };

  const call = async (tool, params) => {
    if (typeof tool !== 'string' || !tool) throw new Error('call(tool, params): tool name required');
    if (st.browser == null) throw new Error('this session has no browser slot bound yet — pass browser:"<slot>" in repl.run (ubb browsers lists them)');
    st.calls++;
    let fwdParams = params && typeof params === 'object' ? params : {};
    // SAME vault gate as the plain tools: {{secret:}} substitutes only in
    // fill/fill_form values, malformed/unknown/strict-refused names fail here
    try {
      const fwd = deps.prepareForward(tool, fwdParams);
      if (fwd.outOfScope?.length) throw vaultError('SECRET_SCOPE');
      if (fwd.invalid?.length) throw vaultError('BAD_SECRET_NAME');
      if (fwd.missing?.length) throw vaultError('SECRET_UNKNOWN');
      if (fwd.denied?.length) throw vaultError('SECRET_ORIGIN_DENIED');
      fwdParams = fwd.params;
    } catch (e) {
      if (e && e.vault) throw e;
      // prepareForward itself throws only when the vault store cannot be read —
      // that is fatal for this call too, with its own honest shape
      throw new Error(`VAULT_UNREADABLE: ${String(e?.message || e)}`);
    }
    const r = await deps.callExtension(st.browser, tool, fwdParams, 60_000);
    if (r.error) {
      const err = new Error(r.error.message || String(r.error));
      err.code = r.error.code;
      throw err;
    }
    return r.result;
  };

  // Session scope object: the persistent handle across repl.run calls (a vm
  // IIFE keeps const/let function-scoped, so persistence is explicit — this is
  // the one deliberate difference from Aside's REPL, stated in the docs).
  const scope = {};

  const bound = (tool, map = (p) => p) => async (...args) => call(tool, map(...args));

  const sandbox = {
    S: scope, // persistent scope: S.x = value survives across calls
    call,
    snapshot: bound('snapshot', (p) => ({ ...(p || {}), diff: p?.diff ?? true })),
    snapshotFull: bound('snapshot', (p) => ({ ...(p || {}), diff: false })),
    click: bound('click', (a, b) => (typeof a === 'string' ? { ref: a, ...(b || {}) } : { ...a, ...b })),
    fill: bound('fill', (ref, value, p) => ({ ref, value, ...(p || {}) })),
    fillForm: bound('fill_form', (values, p) => ({ values, ...(p || {}) })),
    type: bound('type', (text, p) => ({ text, ...(p || {}) })),
    pressKey: bound('press_key', (key, modifiers, p) => ({ key, ...(modifiers ? { modifiers } : {}), ...(p || {}) })),
    hover: bound('hover', (ref, p) => ({ ref, ...(p || {}) })),
    scroll: bound('scroll', (p) => ({ ...(p || {}) })),
    readPage: bound('read_page', (p) => ({ ...(p || {}) })),
    evalJs: bound('evaluate', (expression) => ({ expression })),
    screenshot: bound('screenshot', (p) => ({ ...(p || {}) })),
    navigate: bound('navigate', (url, p) => ({ url, ...(p || {}) })),
    openTab: bound('tab.create', (url, p) => ({ url, ...(p || {}) })),
    closeTab: bound('tab.close', (tab_id, p) => ({ tab_id, ...(p || {}) })),
    tabs: bound('tab.list', (p) => ({ ...(p || {}) })),
    probe: bound('page.probe', (p) => ({ ...(p || {}) })),
    sleep: (ms) => new Promise((r) => setTimeout(r, Math.min(Math.max(Number(ms) || 0, 0), 30_000))),
    console: {
      log: (...a) => line('log', a),
      warn: (...a) => line('warn', a),
      error: (...a) => line('error', a),
    },
  };
  // deliberate omissions: no require/import, no process, no fs, no network
  // primitives beyond the tool API — this is a browser bridge, not a shell
  st.ctx = vm.createContext(sandbox);
  return st;
}

function vaultError(code) {
  const e = new Error(`${code}: the vault refused this call (check ubb secret list / ubb secret bind); nothing was forwarded`);
  e.vault = true;
  e.code = code;
  return e;
}

function stringifyValue(v) {
  if (v === undefined) return null;
  if (v === null || typeof v !== 'object') return v;
  try {
    const json = JSON.stringify(v);
    if (json && json.length <= MAX_OUTPUT_CHARS) return JSON.parse(json);
    return cap(json);
  } catch {
    return String(v);
  }
}
