#!/usr/bin/env node
// UBB bridge daemon — connects ANY agent to the UBB browser extension(s).
//
//   node ubb-bridge.mjs --port 8765 --token <token>
//
// Roles:
//   - extensions: one WS client PER browser profile, subprotocol `ubb-auth.<token>`;
//     each registers under the browser name its hello carries (default 'default')
//   - agents: many WS clients, subprotocol `ubb-agent.<token>`,
//     plus one implicit stdio agent (newline-delimited JSON)
//
// Agent message (WS or stdio), one JSON object per line/frame:
//   → {"id":1,"method":"tool.navigate","params":{"url":"example.com"}}
//   → {"id":2,"method":"tool.snapshot","params":{},"browser":"work"}   ← slot hint
//   ← {"id":1,"result":{...}} | {"id":1,"error":{"code":"...","message":"..."}}
//   ← {"method":"event.tab.navigated","params":{...}}
// Cancel: {"method":"cancel","params":{"id":1}}   Status: {"id":0,"method":"status"}
// Daemon-side (never reach an extension): meta.browsers · pair.register · browser.launch
//
// Extension→daemon (reverse RPC, same authenticated socket), id prefix `s`:
//   → {"id":"s3","method":"secret.resolve","params":{"name":"github_pw"}}
//   ← {"id":"s3","result":{"value":"…"}}   answered to the extension ONLY, never broadcast

import http from 'node:http';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { attachWs } from './wslib.mjs';
import { prepareForward, resolveSecret, setSecret, deleteSecret, listNames, vaultInfo, secretBindings } from './cli/secret.mjs';
import { replMethod } from './ubb-repl.mjs';
import { scanBrowsers } from './diag.mjs';

const args = process.argv.slice(2);
function argOf(name, def) {
  const i = args.indexOf(`--${name}`);
  if (i >= 0 && args[i + 1]) return args[i + 1];
  const env = { port: process.env.UBB_PORT, token: process.env.UBB_TOKEN };
  return env[name] || def;
}

const PORT = Number(argOf('port', 8765));
const TOKEN = String(argOf('token', ''));
if (!TOKEN) {
  console.error('[ubb-bridge] ERROR: --token <token> (or UBB_TOKEN env) is required; copy it from the extension popup.');
  process.exit(1);
}

const log = (...a) => console.error(`[ubb-bridge ${new Date().toISOString()}]`, ...a);

const BROWSER_NAME_RE = /^[a-z0-9_-]{1,32}$/;
const PAIR_NONCE_TTL_MS = 120_000;

const extensions = new Map(); // browser name → { ws, info, lastFrame, gen }
let extGen = 0;
let seq = 0;
const routes = new Map(); // internalId → { agent, agentId, timer, browser }
const pairNonces = new Map(); // nonce → { name, minted }

// Extension→daemon reverse RPC (the secret vault). The extension mints these ids
// as `s<seq>`; the agent→extension routes above use `x<seq>` — disjoint prefixes,
// so a reverse reply can never be mistaken for an agent-route reply.
const EXT_REQ_ID = /^s\d{1,10}$/;
const SECRET_RPC = new Set(['secret.resolve', 'secret.list', 'secret.set', 'secret.delete', 'secret.bindings']);

// ---------------------------------------------------------------- slots

function listBrowsers() {
  return [...extensions.entries()]
    .map(([name, e]) => ({
      name,
      build: e.info?.build ?? null,
      browser: e.info?.browser ?? null,
      version: e.info?.version ?? null,
      instance_id: e.info?.instance_id ?? null,
      workspace: e.info?.workspace === true,
    }))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** Route an agent request to a slot. No hint → the single slot (back-compat);
 *  several slots → refuse and name what exists. */
function selectExtension(want) {
  if (want != null) {
    const e = extensions.get(want);
    if (!e) {
      return { error: { code: 'BROWSER_NOT_FOUND', message: `no browser slot named ${JSON.stringify(want)} is connected; connected: ${[...extensions.keys()].join(', ') || '(none)'}` } };
    }
    return { ext: e, name: want };
  }
  if (extensions.size === 1) {
    const [name, e] = [...extensions.entries()][0];
    return { ext: e, name };
  }
  if (extensions.size === 0) {
    return { error: { code: 'EXTENSION_NOT_CONNECTED', message: 'load the UBB extension and check its popup' } };
  }
  return { error: { code: 'BROWSER_REQUIRED', message: `${extensions.size} browser slots are connected and no "browser" was given — pass the slot name (ubb --browser <name> / params.browser): ${[...extensions.keys()].join(', ')}` } };
}

function dropExtension(name, reason) {
  const e = extensions.get(name);
  if (!e) return;
  extensions.delete(name);
  try { e.ws.close(1000, reason); } catch {}
  log(`extension disconnected: ${JSON.stringify(name)}`);
  for (const [intId, r] of [...routes]) {
    if (r.browser !== name) continue;
    clearTimeout(r.timer);
    routes.delete(intId);
    r.agent.send({ id: r.agentId, error: { code: 'EXTENSION_DISCONNECTED', message: 'extension went away' } });
  }
}

// ---------------------------------------------------------------- agents

class StdioAgent {
  send(obj) {
    process.stdout.write(JSON.stringify(obj) + '\n');
  }
}
const stdioAgent = new StdioAgent();
const wsAgents = new Set();

function broadcast(obj) {
  stdioAgent.send(obj);
  for (const a of wsAgents) a.send(obj);
}

/** Forward an agent request to ONE slot and route the reply back. */
function dispatchToExtension(agent, msg, want) {
  if (INTERNAL_ONLY.has(String(msg.method))) {
    agent.send({ id: msg.id, error: { code: 'INTERNAL_ONLY', message: `${msg.method} moves credential values and is only callable by the daemon itself — use session.transfer {from,to,origin,confirm:true}, which keeps the values off this reply, the audit and every model context` } });
    return;
  }
  const picked = selectExtension(want);
  if (picked.error) {
    agent.send({ id: msg.id, error: picked.error });
    return;
  }
  const { ext, name } = picked;
  // §5 fallback channel: {{secret:NAME}} is resolved here, in the daemon, and
  // only in the values of fill / fill_form and in replay / replay.run's `vars`.
  // `type` was REMOVED from the set after a live incident: it has no target
  // field, so nothing can prove the focused box is not an OTP/CVV input — a
  // placeholder there stays literal. Every other method is forwarded verbatim.
  let fwd;
  try {
    fwd = prepareForward(msg.method, msg.params);
  } catch (e) {
    // a corrupt vault must fail THIS call, not kill the daemon
    log('forward preparation failed for', msg.method, '—', e?.code || 'VAULT_ERROR');
    agent.send({ id: msg.id, error: { code: 'VAULT_UNREADABLE', message: `the local secret store could not be read (${e?.code || 'VAULT_ERROR'}); fix ~/.ubb/secrets.json or unset UBB_SECRETS_FILE, then retry` } });
    return;
  }
  if (fwd.outOfScope && fwd.outOfScope.length) {
    // paths only — never the placeholder or a resolved value
    log('rejected out-of-scope secret placeholder for', msg.method, '—', fwd.outOfScope.join(','));
    agent.send({ id: msg.id, error: { code: 'SECRET_SCOPE', message: `{{secret:...}} is substituted only inside ${fwd.tool === 'fill_form' ? 'values[*]' : 'value'} — found in ${fwd.outOfScope.slice(0, 3).join(', ')}; nothing was forwarded` } });
    return;
  }
  if (fwd.invalid.length) {
    log('rejected malformed secret placeholder for', msg.method, '—', fwd.invalid.join(','));
    agent.send({ id: msg.id, error: { code: 'BAD_SECRET_NAME', message: `malformed secret name "${fwd.invalid[0]}" (allowed: letters, digits, _ . - up to 64 chars)` } });
    return;
  }
  if (fwd.missing.length) {
    log('unknown secret name for', fwd.tool, '—', fwd.missing.join(','));
    agent.send({ id: msg.id, error: { code: 'SECRET_UNKNOWN', message: `no secret named "${fwd.missing[0]}" in the local vault — list names with: ubb secret list; add one with: ubb secret add <name>. Never put the plaintext value in a tool argument.` } });
    return;
  }
  // strict mode + an UNBOUND secret is origin-independent: it would be refused
  // on every host, so refuse here and let no plaintext cross this socket.
  if (fwd.denied && fwd.denied.length) {
    log('strict mode refused unbound secret(s) for', fwd.tool, '—', fwd.denied.join(','), '(value not forwarded)');
    agent.send({ id: msg.id, error: { code: 'SECRET_ORIGIN_DENIED', message: `secret "${fwd.denied[0]}" has no origin binding and secret_origin_mode is strict — nothing was forwarded. Bind it as a human: \`ubb secret bind ${fwd.denied[0]} <host>\`, or relax with \`ubb secret mode off\`.` } });
    return;
  }
  if (fwd.replaced) log('resolved', fwd.replaced, 'secret placeholder(s) for', fwd.tool, '(value not logged)');
  const intId = `x${++seq}`;
  // A route that never receives a reply must not leak forever (record_start
  // is the longest legitimate call, capped at 10 minutes).
  const timer = setTimeout(() => {
    const r = routes.get(intId);
    if (!r) return;
    routes.delete(intId);
    r.agent.send({ id: r.agentId, error: { code: 'ROUTE_TIMEOUT', message: 'no reply from the extension within 15 minutes; the action outcome is unknown' } });
  }, 15 * 60_000);
  timer.unref?.();
  routes.set(intId, { agent, agentId: msg.id, timer, browser: name });
  ext.ws.send(JSON.stringify({ id: intId, method: msg.method, params: fwd.params }));
}

/**
 * Promise-based one-shot tool call to a slot, for daemon-side multi-step
 * methods (session.transfer, repl.run). Values returned here stay INSIDE the
 * daemon — a caller decides what reaches an agent, and session.transfer
 * forwards counts only. The reply mechanics reuse the same routes map and the
 * same 15-minute safety net shape as dispatchToExtension, with its own timer.
 */
function callExtension(name, method, params, timeoutMs = 30_000) {
  return new Promise((resolve) => {
    const picked = selectExtension(name);
    if (picked.error) {
      resolve({ error: picked.error });
      return;
    }
    const { ext, name: slot } = picked;
    const intId = `x${++seq}`;
    const timer = setTimeout(() => {
      if (routes.delete(intId)) {
        resolve({ error: { code: 'ROUTE_TIMEOUT', message: `no reply from slot ${JSON.stringify(slot)} within ${timeoutMs}ms` } });
      }
    }, timeoutMs);
    timer.unref?.();
    // a synthetic agent: the route reply path calls send() exactly once
    const synthetic = {
      send(obj) {
        clearTimeout(timer);
        routes.delete(intId);
        if (obj.error) resolve({ error: obj.error });
        else resolve({ result: obj.result });
      },
    };
    routes.set(intId, { agent: synthetic, agentId: intId, timer, browser: slot });
    ext.ws.send(JSON.stringify({ id: intId, method, params: params || {} }));
  });
}

// ------------------------------------------------------------- daemon methods

/** Methods answered in the daemon, never forwarded to an extension. */
function daemonMethod(agent, msg) {
  const m = msg.method;
  if (m === 'status') {
    const { name: agentSlot, auto, userSlot, autoOpenUser } = agentSlotConfig();
    agent.send({
      id: msg.id,
      result: {
        extension_connected: extensions.size > 0,
        browsers: listBrowsers(),
        inflight: routes.size,
        agent_slot: agentSlot,
        auto_workspace: auto,
        user_browser: userSlot,
        auto_open_user_browser: autoOpenUser,
      },
    });
    return true;
  }
  if (m === 'meta.browsers' || m === 'browsers.list') {
    agent.send({ id: msg.id, result: { browsers: listBrowsers() } });
    return true;
  }
  if (m === 'pair.register') {
    const name = String(msg.params?.name || '');
    if (!BROWSER_NAME_RE.test(name)) {
      agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'pair.register needs a slot name matching [a-z0-9_-]{1,32}' } });
      return true;
    }
    const nonce = crypto.randomBytes(32).toString('hex');
    pairNonces.set(nonce, { name, minted: Date.now() });
    agent.send({ id: msg.id, result: { nonce, ttl_s: PAIR_NONCE_TTL_MS / 1000 } });
    return true;
  }
  if (m === 'session.transfer') {
    void sessionTransfer(agent, msg);
    return true;
  }
  if (m === 'repl.run' || m === 'repl.reset') {
    void replMethod(agent, msg, { callExtension, log, prepareForward });
    return true;
  }
  if (m === 'diag.browsers') {
    void diagBrowsersMethod(agent, msg);
    return true;
  }
  if (m === 'tunnel.start') {
    tunnelStart(agent, msg);
    return true;
  }
  if (m === 'tunnel.stop') {
    tunnelStop(agent, msg);
    return true;
  }
  if (m === 'tunnel.status') {
    agent.send({ id: msg.id, result: tunnelStatusResult() });
    return true;
  }
  return false;
}

// ------------------------------------------------------------- diag.browsers

/** Shared body for the agent channel and the extension reverse channel: the
 *  read-only multi-browser scan plus this daemon's own live state. */
async function diagBrowsersResult() {
  const scan = await scanBrowsers();
  const connected = extensions.size > 0;
  return {
    daemon: { port: PORT, port_ok: true, pairing_ok: connected, extension_connected: connected },
    browsers: scan.browsers,
    slots: listBrowsers(),
  };
}

async function diagBrowsersMethod(agent, msg) {
  try {
    agent.send({ id: msg.id, result: await diagBrowsersResult() });
  } catch (e) {
    agent.send({ id: msg.id, error: { code: 'DIAG_FAILED', message: `browser scan failed: ${String(e?.message || e)}` } });
  }
}

// -------------------------------------------------------------------- tunnel
// Opt-in Codex-style long-polling MCP channel (ABSORB-CODEX-PLAN §0.2/§9).
// SECURITY: a running tunnel exposes this daemon's tool surface to the REMOTE
// base-url it was started with. Nothing ever starts it on its own — only an
// explicit tunnel.start (ubb tunnel start --base-url … --token …) does, the
// state lives in this daemon's memory only (nothing lands on disk), and the
// default allow:"read" forwards just the read-only perception whitelist below.
// allow:"all" forwards the full tool surface a local agent could reach — the
// extension's own confirm/policy/shield gates still apply on every command, and
// every forwarded command is logged. The relay token is never echoed to agents.

const TUNNEL_POLL_TIMEOUT_MS = 15_000; // server long-poll hold, per the Codex protocol
const TUNNEL_BACKOFF_MAX_MS = 30_000; // exponential backoff ceiling (1s → 30s)
const TUNNEL_CMD_TIMEOUT_MS = 15_000; // hard cap per forwarded command

// Read-only perception whitelist for allow:"read" — the plan's frozen seven plus
// the two unambiguously read-only helpers (frames.probe is pure capability
// diagnosis, extract only reads page content). Anything else is refused here,
// before it ever reaches a slot.
const TUNNEL_READ_TOOLS = new Set([
  'snapshot',
  'read_page',
  'tab.list',
  'frames.list',
  'console.list',
  'network.list',
  'screenshot',
  'frames.probe',
  'extract',
]);

const tunnel = {
  running: false,
  base: '',
  token: '',
  allow: 'read',
  id: '',
  started_at: null,
  last_poll: null,
  stop_reason: null,
  stats: { polls: 0, commands: 0, errors: 0 },
};

/** Status snapshot for tunnel.status / tunnel.start replies — token redacted. */
function tunnelStatusResult() {
  return {
    running: tunnel.running,
    id: tunnel.id || null,
    base: tunnel.base || null,
    allow: tunnel.allow,
    started_at: tunnel.started_at,
    last_poll: tunnel.last_poll,
    stop_reason: tunnel.stop_reason,
    stats: { ...tunnel.stats },
    // the token authenticates to the REMOTE relay — never echoed in full
    token: tunnel.token ? `${tunnel.token.slice(0, 4)}…` : null,
  };
}

const tunnelSleep = (ms) => new Promise((r) => setTimeout(r, ms));

function tunnelStart(agent, msg) {
  const p = msg.params || {};
  let base = null;
  try {
    const u = new URL(String(p.base_url || ''));
    if (u.protocol === 'https:' || u.protocol === 'http:') base = String(p.base_url).replace(/\/+$/, '');
  } catch { /* falls through to BAD_PARAMS */ }
  if (!base) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'tunnel.start needs base_url as an absolute http(s) URL, e.g. "https://relay.example.com"' } });
    return;
  }
  if (!String(p.token || '')) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'tunnel.start needs the relay token (Authorization: Bearer <token> on every poll/response)' } });
    return;
  }
  const allow = p.allow === 'all' ? 'all' : 'read';
  const id = p.id != null && /^[A-Za-z0-9._-]{1,64}$/.test(String(p.id)) ? String(p.id) : `ubb-${crypto.randomBytes(4).toString('hex')}`;
  if (tunnel.running) {
    agent.send({ id: msg.id, error: { code: 'TUNNEL_ALREADY_RUNNING', message: `a tunnel is already running (id ${JSON.stringify(tunnel.id)}, base ${tunnel.base}) — tunnel.stop first` } });
    return;
  }
  Object.assign(tunnel, {
    running: true,
    base,
    token: String(p.token),
    allow,
    id,
    started_at: new Date().toISOString(),
    last_poll: null,
    stop_reason: null,
    stats: { polls: 0, commands: 0, errors: 0 },
  });
  log(allow === 'all'
    ? `tunnel started (WARNING allow=all: the FULL tool surface, writes included, is exposed to ${base}; id ${id})`
    : `tunnel started (allow=read: read-only tools only; base ${base}, id ${id})`);
  tunnelLoop().catch((e) => {
    tunnel.running = false;
    tunnel.stop_reason = `loop crashed: ${String(e?.message || e)}`;
    log('tunnel loop crashed', e?.message || e);
  });
  agent.send({ id: msg.id, result: tunnelStatusResult() });
}

function tunnelStop(agent, msg) {
  if (!tunnel.running) {
    agent.send({ id: msg.id, result: { running: false, stop_reason: tunnel.stop_reason || 'not running', stats: { ...tunnel.stats } } });
    return;
  }
  tunnel.running = false;
  tunnel.stop_reason = 'stopped by request';
  log('tunnel stopped by request');
  agent.send({ id: msg.id, result: tunnelStatusResult() });
}

/** One forwarded tunnel command, as a virtual agent: tunnel commands ride the
 *  SAME nameless-agent routing as a local WS/stdio call (selectExtension rules,
 *  secret placeholder handling, INTERNAL_ONLY refusal) via handleAgentMessage,
 *  which answers through the synthetic send() exactly once. A hung extension
 *  cannot wedge the loop: 15s cap, and the late reply lands in the
 *  already-settled no-op sink while the route's own 15-minute safety net
 *  reclaims it. */
function tunnelCallExtension(tool, params) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (payload) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(payload);
    };
    const timer = setTimeout(() => {
      tunnel.stats.errors++;
      finish({ error: { code: 'ROUTE_TIMEOUT', message: `tunnel command "${tool}" produced no reply within ${TUNNEL_CMD_TIMEOUT_MS}ms; the outcome is unknown` } });
    }, TUNNEL_CMD_TIMEOUT_MS);
    timer.unref?.();
    const synthetic = { send: (o) => finish(o?.error ? { error: o.error } : { result: o?.result ?? null }) };
    Promise.resolve(handleAgentMessage(synthetic, { id: ++seq, method: `tool.${tool}`, params })).catch((e) => {
      tunnel.stats.errors++;
      finish({ error: { code: 'INTERNAL', message: `tunnel dispatch failed: ${String(e?.message || e)}` } });
    });
  });
}

/** One command from the relay: allow-gate (read = whitelist only), then
 *  forward through the normal agent routing. */
async function tunnelRunCommand(tool, params) {
  if (!tool) return { error: { code: 'BAD_PARAMS', message: 'tunnel command carries no tool name' } };
  if (tunnel.allow !== 'all' && !TUNNEL_READ_TOOLS.has(tool)) {
    return {
      error: {
        code: 'TUNNEL_TOOL_DENIED',
        message: `allow:"${tunnel.allow}" forwards only read-only perception tools (${[...TUNNEL_READ_TOOLS].sort().join(', ')}); "${tool}" is refused here — run it through a local agent`,
      },
    };
  }
  if (tunnel.allow === 'all') log(`tunnel WARNING allow=all: executing remote command "${tool}"`);
  else log(`tunnel read command "${tool}"`);
  const out = await tunnelCallExtension(tool, params);
  if (out.error) tunnel.stats.errors++;
  return out;
}

/** The long-poll loop: GET {base}/tunnels/{id}/poll?limit=1&timeout_ms=15000
 *  with the relay token; 204 = idle (reset backoff), 200 = one command to run
 *  and POST back to /tunnels/{id}/response as {id, result|error}. Failures
 *  back off 1s→30s; 401/403 (auth) and 404/410 (tunnel gone) stop the loop. */
async function tunnelLoop() {
  let backoff = 1000;
  while (tunnel.running) {
    let res;
    try {
      res = await fetch(`${tunnel.base}/tunnels/${encodeURIComponent(tunnel.id)}/poll?limit=1&timeout_ms=${TUNNEL_POLL_TIMEOUT_MS}`, {
        headers: { Authorization: `Bearer ${tunnel.token}` },
        signal: AbortSignal.timeout(TUNNEL_POLL_TIMEOUT_MS + 10_000),
      });
    } catch (e) {
      tunnel.stats.errors++;
      if (!tunnel.running) break;
      log(`tunnel poll failed (${String(e?.message || e)}); backing off ${Math.round(backoff / 1000)}s`);
      await tunnelSleep(backoff);
      backoff = Math.min(backoff * 2, TUNNEL_BACKOFF_MAX_MS);
      continue;
    }
    if (!tunnel.running) break;
    tunnel.last_poll = new Date().toISOString();
    if (res.status === 401 || res.status === 403) {
      tunnel.stats.errors++;
      tunnel.running = false;
      tunnel.stop_reason = `relay rejected the token (HTTP ${res.status})`;
      log(`tunnel stopped: ${tunnel.stop_reason}`);
      break;
    }
    if (res.status === 404 || res.status === 410) {
      tunnel.running = false;
      tunnel.stop_reason = `tunnel gone at the relay (HTTP ${res.status})`;
      log(`tunnel stopped: ${tunnel.stop_reason}`);
      break;
    }
    if (res.status === 204) {
      tunnel.stats.polls++;
      backoff = 1000; // healthy idle
      continue;
    }
    if (!res.ok) {
      tunnel.stats.errors++;
      log(`tunnel poll HTTP ${res.status}; backing off ${Math.round(backoff / 1000)}s`);
      await tunnelSleep(backoff);
      backoff = Math.min(backoff * 2, TUNNEL_BACKOFF_MAX_MS);
      continue;
    }
    tunnel.stats.polls++;
    backoff = 1000;
    let cmd = null;
    try {
      cmd = await res.json();
    } catch (e) {
      tunnel.stats.errors++;
      log(`tunnel poll body was not JSON (${String(e?.message || e)})`);
      continue;
    }
    const out = await tunnelRunCommand(String(cmd?.tool || '').replace(/^tool\./, ''), cmd?.params && typeof cmd.params === 'object' && !Array.isArray(cmd.params) ? cmd.params : {});
    tunnel.stats.commands++;
    // a failed response POST is an error counter, never a loop stopper — the
    // command already ran, so losing the reply must not spin the relay dry
    try {
      const body = { id: cmd?.id ?? null };
      if (out.error) body.error = out.error;
      else body.result = out.result ?? null;
      const r2 = await fetch(`${tunnel.base}/tunnels/${encodeURIComponent(tunnel.id)}/response`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${tunnel.token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(15_000),
      });
      if (!r2.ok) {
        tunnel.stats.errors++;
        log(`tunnel response POST failed (HTTP ${r2.status})`);
      }
    } catch (e) {
      tunnel.stats.errors++;
      log(`tunnel response POST failed (${String(e?.message || e)})`);
    }
  }
}

// ------------------------------------------------------------- session.transfer

/**
 * Move one origin's cookies from slot A to slot B (e.g. the user's real browser
 * → an isolated workspace). The VALUES cross the two daemon⇄extension links
 * only: this reply, the audit and every model context carry counts and names,
 * never a value. confirm:true is mandatory — copying a session IS copying a
 * credential. Cookie-scoped, never "everything".
 */
async function sessionTransfer(agent, msg) {
  const p = msg.params || {};
  if (p.confirm !== true) {
    agent.send({
      id: msg.id,
      error: { code: 'NEEDS_CONFIRM', message: 'session.transfer copies session cookies between browser slots — that is copying a credential. Retry with confirm:true only for an origin you intended, never with "trust me"' },
    });
    return;
  }
  let origin;
  try {
    const u = new URL(String(p.origin || ''));
    if (u.protocol !== 'https:' && u.protocol !== 'http:') throw new Error('scheme');
    origin = u.origin;
  } catch {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'origin must be an absolute http(s) URL like "https://example.com"' } });
    return;
  }
  const cfg = agentSlotConfig();
  const from = p.from ? String(p.from) : cfg.userSlot;
  const to = String(p.to || '');
  if (!BROWSER_NAME_RE.test(to)) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'to must be a slot name matching [a-z0-9_-]{1,32} (see ubb browsers)' } });
    return;
  }
  if (!extensions.has(to)) {
    agent.send({ id: msg.id, error: { code: 'BROWSER_NOT_FOUND', message: `target slot ${JSON.stringify(to)} is not connected; connected: ${[...extensions.keys()].join(', ') || '(none)'}` } });
    return;
  }
  if (from === to) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'source and target are the same slot — nothing to transfer' } });
    return;
  }
  const exp = await callExtension(from, 'cookie.export', { origin, ...(p.from_tab_id != null ? { tab_id: p.from_tab_id } : {}) }, 20_000);
  if (exp.error) {
    agent.send({ id: msg.id, error: { code: exp.error.code || 'TRANSFER_SOURCE_FAILED', message: `source slot ${JSON.stringify(from)}: ${exp.error.message}` } });
    return;
  }
  const cookies = Array.isArray(exp.result?.cookies) ? exp.result.cookies : [];
  if (!cookies.length) {
    agent.send({ id: msg.id, result: { transferred: 0, total: 0, from, to, origin: new URL(origin).host, note: `no cookies for this origin in slot ${JSON.stringify(from)} — nothing to move` } });
    return;
  }
  const imp = await callExtension(to, 'cookie.import', { origin, cookies }, 30_000);
  if (imp.error) {
    agent.send({ id: msg.id, error: { code: imp.error.code || 'TRANSFER_TARGET_FAILED', message: `target slot ${JSON.stringify(to)}: ${imp.error.message}` } });
    return;
  }
  const host = new URL(origin).host;
  log(`session.transfer ${host} ${JSON.stringify(from)}→${JSON.stringify(to)}: ${imp.result?.written}/${cookies.length} cookies (values not logged)`);
  agent.send({
    id: msg.id,
    result: {
      transferred: imp.result?.written ?? 0,
      failed_names: imp.result?.failed_names ?? [],
      total: cookies.length,
      from,
      to,
      origin: host,
      note: 'cookie VALUES crossed the two daemon⇄extension links only — never this reply, the audit or a model context; the source keeps its cookies (copy, not move)',
    },
  });
}

// ------------------------------------------------------------- browser.launch

function readConfig() {
  // UBB_CONFIG_FILE overrides the path (same rule as cli/secret.mjs; tests
  // must never read the user's real config)
  const p = process.env.UBB_CONFIG_FILE || path.join(os.homedir(), '.ubb', 'config.json');
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return {};
  }
}

/** Routing target for nameless agent calls.
 *
 *  Default behaviour (auto_open_user_browser !== false): prefer the user's real
 *  logged-in browser — the configured `user_browser` slot, else the single
 *  connected non-workspace slot. When none is connected the daemon launches
 *  the browser's NORMAL profile (no --user-data-dir / --load-extension) and
 *  waits for it. `auto_workspace:true` opts back into the old workspace-first
 *  routing; `auto_open_user_browser:false` + `auto_workspace:false` restores
 *  the legacy single-slot / BROWSER_REQUIRED rules. */
function agentSlotConfig() {
  const cfg = readConfig();
  const name = typeof cfg.agent_browser === 'string' && BROWSER_NAME_RE.test(cfg.agent_browser) ? cfg.agent_browser : 'agent';
  const auto = cfg.auto_workspace === true;
  const waitMs = Math.max(500, Math.min(120000, Number(cfg.workspace_wait_ms) || 20000));
  const userSlot = typeof cfg.user_browser === 'string' && BROWSER_NAME_RE.test(cfg.user_browser) ? cfg.user_browser : 'default';
  const autoOpenUser = cfg.auto_open_user_browser !== false;
  const userWaitMs = Math.max(500, Math.min(120000, Number(cfg.user_browser_wait_ms) || 20000));
  const userExe = typeof cfg.user_browser_exe === 'string' ? cfg.user_browser_exe : '';
  const fallbackExe = typeof cfg.browser_exe === 'string' ? cfg.browser_exe : '';
  return { name, auto, waitMs, userSlot, autoOpenUser, userWaitMs, userExe, fallbackExe };
}

/** Chrome-family unpacked extension id: sha256(abs path)[:32 hex] mapped to a-p.
 *  Chromium hashes the RAW bytes of base::FilePath::value — wchar_t (UTF-16LE)
 *  on Windows, char (UTF-8) elsewhere. Hashing UTF-8 on Windows produced an id
 *  the real extension never had (measured on Edge 156: sha256(utf16le) matches,
 *  sha256(utf8) does not), so the pair page messaged a ghost and browser.launch
 *  could never pair. */
function unpackedExtId(p) {
  const bytes = process.platform === 'win32' ? Buffer.from(p, 'utf16le') : Buffer.from(p, 'utf8');
  const hex = crypto.createHash('sha256').update(bytes).digest('hex');
  return [...hex.slice(0, 32)].map((c) => String.fromCharCode(parseInt(c, 16) + 97)).join('');
}

function pairPage(token, name, nonce, extId) {
  // the object literal itself (valid JSON = valid JS), NOT a JSON string —
  // chrome.runtime.sendMessage needs the message object, and a string payload
  // would reach the extension as msg = "..." with no .type (refused, honestly,
  // but the launch would never pair)
  const payload = JSON.stringify({ type: 'UBB_PAIR_LAUNCH', nonce, token, port: PORT, name });
  const html = `<!doctype html><html><head><meta charset="utf-8"><title>UBB pairing</title>
<style>body{font:14px/1.6 system-ui;background:#1b1a20;color:#eee;padding:2rem}</style></head>
<body><h2>UBB pairing…</h2><pre id="log" style="white-space:pre-wrap"></pre>
<script>
const EXT_ID = ${JSON.stringify(extId)};
const PAYLOAD = ${payload};
const log = document.getElementById('log');
let done = false;
function attempt() {
  if (done) return;
  try {
    chrome.runtime.sendMessage(EXT_ID, PAYLOAD, (r) => {
      if (chrome.runtime.lastError) { log.textContent += 'retrying…\\n'; return; }
      done = true;
      if (r && r.ok) log.textContent = '已配对为浏览器槽位 ' + r.paired_as + ' — 这个窗口可以关掉，也可以留给 agent 使用。';
      else log.textContent = '扩展拒绝配对: ' + JSON.stringify(r);
    });
  } catch (e) { log.textContent += 'retrying…\\n'; }
}
attempt(); setTimeout(attempt, 1500); setTimeout(attempt, 4000); setTimeout(attempt, 8000);
</script></body></html>`;
  return html;
}

/** Spawn an isolated-profile browser instance that self-pairs as slot `name`.
 *  Shared by the explicit `browser.launch` method and the nameless-call
 *  workspace auto-launch. Returns the result object ({error} on failure). */
function launchWorkspace(name, params = {}) {
  const cfg = readConfig();
  let exe = String(params.exe || cfg.browser_exe || '');
  if (!exe) {
    const candidates = [
      'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
      'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
      'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
      'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
    ];
    exe = candidates.find((p) => { try { return fs.statSync(p).isFile(); } catch { return false; } }) || '';
  }
  if (!exe || !fs.existsSync(exe)) {
    return { error: { code: 'BROWSER_EXE_NOT_FOUND', message: 'could not locate msedge.exe/chrome.exe — pass params.exe ("edge", "chrome" or an absolute path), or set browser_exe in ~/.ubb/config.json' } };
  }
  let extDir = String(params.extension_dir || cfg.extension_dir || '');
  if (!extDir) {
    // the daemon lives in <repo>/daemon, the manifest in <repo>
    const guess = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..');
    if (fs.existsSync(path.join(guess, 'manifest.json'))) extDir = guess;
  }
  if (!extDir || !fs.existsSync(path.join(extDir, 'manifest.json'))) {
    return { error: { code: 'BAD_PARAMS', message: 'extension_dir unknown — pass it, or set extension_dir in ~/.ubb/config.json' } };
  }
  const profileDir = path.join(os.homedir(), '.ubb', 'profiles', name);
  fs.mkdirSync(profileDir, { recursive: true });
  const nonce = crypto.randomBytes(32).toString('hex');
  pairNonces.set(nonce, { name, minted: Date.now() });
  const url = `http://127.0.0.1:${PORT}/__ubb_pair?nonce=${nonce}`;
  const child = spawn(exe, [
    `--user-data-dir=${profileDir}`,
    `--load-extension=${extDir}`,
    // background control: the workspace must not steal the user's focus.
    // Honoured by Chromium on Windows (first window starts minimized without
    // activation); where a build ignores it, the extension's own one-shot
    // minimize-on-start (ubb_workspace flag) is the fallback.
    // params.start_minimized:false opts OUT — trusted input never drains into a
    // window Windows still considers occluded/minimized (visibilityState stays
    // hidden and every real pointer/key action is refused pre-dispatch), so a
    // challenge/typing test needs a window the OS will actually paint on top.
    ...(params.start_minimized === false
      ? ['--window-position=80,60', '--window-size=1280,900']
      : ['--start-minimized']),
    // caller-supplied Chromium switches (tests: third-party cookie policy)
    ...(Array.isArray(params.chromium_args) ? params.chromium_args.map(String) : []),
    '--no-first-run',
    '--no-default-browser-check',
    url,
  ], { detached: true, stdio: 'ignore' });
  child.unref();
  return {
    launched: true,
    name,
    exe,
    profile_dir: profileDir,
    extension_id: unpackedExtId(extDir),
  };
}

function browserLaunch(agent, msg) {
  const name = String(msg.params?.name || '');
  if (!BROWSER_NAME_RE.test(name)) {
    agent.send({ id: msg.id, error: { code: 'BAD_PARAMS', message: 'browser.launch needs name matching [a-z0-9_-]{1,32} (the slot the new instance registers under)' } });
    return;
  }
  const out = launchWorkspace(name, msg.params || {});
  if (out.error) {
    agent.send({ id: msg.id, error: out.error });
    return;
  }
  agent.send({
    id: msg.id,
    result: {
      ...out,
      note: 'the fresh profile pairs itself through the daemon one-shot page (started minimized — it does not steal focus); wait for the name to appear (ubb browsers), then target it with browser:"<name>" / ubb --browser <name>',
    },
  });
}

// ------------------------------------------------------- agent workspace slot
// Nameless agent calls must never land in the user's own browser: they route
// to a dedicated agent-workspace instance (an isolated-profile browser the
// daemon spawns on demand). The user's real browser stays untouched unless an
// agent names its slot explicitly (`ubb --browser default`).

// Tools that must never CAUSE a browser launch just to be routed.
// browser.close is the obvious one: "close the browser" when none is open is a
// no-op/refusal, not a reason to start a browser.
const NO_AUTO_OPEN = new Set(['browser.close', 'session.transfer', 'repl.run', 'repl.reset']);
// Methods that move credential values between slots. An agent may NEVER call
// them directly — the values must cross the daemon⇄extension links only, and
// the daemon-side wrapper demands confirm:true.
const INTERNAL_ONLY = new Set(['cookie.export', 'cookie.import']);

function isWorkspaceExt(e) {
  return e?.info?.workspace === true;
}

/** Pick the user's real browser slot: the configured `user_browser` name when
 *  connected, else the only connected non-workspace slot. Multiple real slots
 *  without the configured one → BROWSER_REQUIRED (never guess). */
function realSlot(cfg) {
  const named = extensions.get(cfg.userSlot);
  if (named && !isWorkspaceExt(named)) return { name: cfg.userSlot };
  const reals = [...extensions.entries()]
    .filter(([, e]) => !isWorkspaceExt(e))
    .map(([n]) => n);
  if (reals.length === 1) return { name: reals[0] };
  if (reals.length > 1) {
    return { error: { code: 'BROWSER_REQUIRED', message: `${reals.join(', ')} are connected real browser slots and none matches user_browser ${JSON.stringify(cfg.userSlot)} — pass the slot name (ubb --browser <name> / params.browser)` } };
  }
  return {};
}

let workspaceLaunch = null; // single-flight: concurrent nameless calls share one launch+wait
let userBrowserLaunch = null; // single-flight: one normal-profile launch at a time

function resolveUserBrowserExe(cfg) {
  const edge = [
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  ];
  const chrome = [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
  ];
  const raw = String(cfg.userExe || cfg.fallbackExe || '');
  if (raw) {
    if (raw === 'edge' || raw === 'chrome') {
      const hit = (raw === 'edge' ? edge : chrome).find((p) => { try { return fs.statSync(p).isFile(); } catch { return false; } });
      if (hit) return { exe: hit };
      return { error: { code: 'BROWSER_EXE_NOT_FOUND', message: `could not locate ${raw} — set user_browser_exe in ~/.ubb/config.json` } };
    }
    if (fs.existsSync(raw)) return { exe: raw };
    return { error: { code: 'BROWSER_EXE_NOT_FOUND', message: `user_browser_exe does not exist: ${raw}` } };
  }
  const hit = [...edge, ...chrome].find((p) => { try { return fs.statSync(p).isFile(); } catch { return false; } });
  if (hit) return { exe: hit };
  return { error: { code: 'BROWSER_EXE_NOT_FOUND', message: 'could not locate msedge.exe/chrome.exe — set user_browser_exe (or browser_exe) in ~/.ubb/config.json' } };
}

/** Start the browser's NORMAL profile, not an isolated one: no --user-data-dir,
 *  no --load-extension. --start-minimized keeps the cold start out of the
 *  user's face; the extension is already installed in the real profile. */
function launchUserBrowser(cfg) {
  const resolved = resolveUserBrowserExe(cfg);
  if (resolved.error) return resolved;
  const args = [];
  if (process.platform === 'win32') args.push('--start-minimized');
  args.push('about:blank');
  const child = spawn(resolved.exe, args, { detached: true, stdio: 'ignore' });
  child.unref();
  return { launched: true, exe: resolved.exe, pid: child.pid, profile: 'default' };
}

/** Wait for the user's real browser slot, launching the normal profile once if
 *  it is missing. Never falls back to an isolated workspace. */
async function ensureUserBrowser(cfg) {
  const first = realSlot(cfg);
  if (first.name || first.error) return first;
  if (!cfg.autoOpenUser) return {};
  if (!userBrowserLaunch) {
    userBrowserLaunch = (async () => {
      const again = realSlot(cfg);
      if (again.name || again.error) return again;
      const launch = launchUserBrowser(cfg);
      if (launch.error) return { error: launch.error };
      log(`user browser ${JSON.stringify(cfg.userSlot)} spawning (${launch.exe}); waiting up to ${cfg.userWaitMs}ms for its extension`);
      const deadline = Date.now() + cfg.userWaitMs;
      while (Date.now() < deadline) {
        const picked = realSlot(cfg);
        if (picked.name) return { ...picked, launched: true, exe: launch.exe };
        if (picked.error) return picked;
        await new Promise((r) => setTimeout(r, 150));
      }
      return { error: { code: 'USER_BROWSER_LAUNCH_TIMEOUT', message: `the user browser was started with its normal profile (${launch.exe}) but no real UBB slot paired within ${cfg.userWaitMs}ms — check that the extension is installed and enabled in that profile, or set auto_open_user_browser:false / use browser.launch for an isolated instance` } };
    })().finally(() => { userBrowserLaunch = null; });
  }
  return userBrowserLaunch;
}

/**
 * Opt-in workspace-first routing (`auto_workspace:true` only). When the
 * workspace slot is connected this returns immediately; when it is missing the
 * daemon spawns the workspace (isolated profile, one-shot pair page) and waits
 * for it to self-pair. `null` means the caller should fall through to the
 * real-browser / legacy rules.
 */
async function ensureAgentSlot() {
  const { name, auto, waitMs } = agentSlotConfig();
  if (extensions.has(name)) return { name };
  if (!auto) return null;
  if (!workspaceLaunch) {
    workspaceLaunch = (async () => {
      const launch = launchWorkspace(name);
      if (launch.error) return { error: launch.error };
      log(`agent workspace ${JSON.stringify(name)} spawning (${launch.exe}); waiting up to ${waitMs}ms for it to self-pair`);
      const deadline = Date.now() + waitMs;
      while (Date.now() < deadline) {
        if (extensions.has(name)) return { name, launched: true };
        await new Promise((r) => setTimeout(r, 150));
      }
      return { error: { code: 'AGENT_LAUNCH_TIMEOUT', message: `the agent workspace ${JSON.stringify(name)} was spawned (profile ${launch.profile_dir}) but did not pair within ${waitMs}ms — check that ${launch.exe} actually started and loaded the extension (--load-extension is dropped when another instance of the SAME profile is already running). In auto_workspace mode a nameless call stays on this workspace target; name another slot explicitly with ubb --browser <name> / params.browser if you meant to.` } };
    })().finally(() => { workspaceLaunch = null; });
  }
  return workspaceLaunch;
}

async function handleAgentMessage(agent, msg) {
  if (!msg || typeof msg !== 'object' || Array.isArray(msg)) return;
  if (msg.method === 'cancel') {
    if (msg.params?.id != null) {
      for (const [intId, r] of routes) {
        // identity, not just the caller-chosen id: two agents both using id=1
        // must not be able to cancel each other
        if (r.agent === agent && r.agentId === msg.params.id) {
          const ext = extensions.get(r.browser);
          if (ext) ext.ws.send(JSON.stringify({ method: 'cancel', params: { id: intId } }));
          return;
        }
      }
    }
    return;
  }
  if (msg.method == null || msg.id == null) {
    agent.send({ id: msg.id ?? null, error: { code: 'BAD_REQUEST', message: 'need id and method' } });
    return;
  }
  if (daemonMethod(agent, msg)) return;
  if (msg.method === 'browser.launch') {
    browserLaunch(agent, msg);
    return;
  }
  // routing hint: envelope `browser` wins, then params.browser (stripped before
  // the tool ever sees it — unknown-param handling stays the extension's job)
  let want = typeof msg.browser === 'string' && msg.browser ? msg.browser : null;
  let params = msg.params;
  if (!want && params && typeof params.browser === 'string' && params.browser) {
    want = params.browser;
    params = { ...params };
    delete params.browser;
    msg = { ...msg, params };
  }
  const routeCfg = agentSlotConfig();
  const autoOpenAllowed = !NO_AUTO_OPEN.has(String(msg.method));
  if (want) {
    // An explicit `--browser default` may wake the user's normal profile; other
    // slot names never do (they are workspaces or genuinely unknown).
    if (!extensions.has(want) && want === routeCfg.userSlot && routeCfg.autoOpenUser && autoOpenAllowed) {
      try {
        const ensured = await ensureUserBrowser(routeCfg);
        if (ensured?.error) {
          agent.send({ id: msg.id, error: ensured.error });
          return;
        }
        if (ensured?.name) want = ensured.name;
      } catch (e) {
        agent.send({ id: msg.id, error: { code: 'INTERNAL', message: `user browser routing failed: ${String(e?.message || e)}` } });
        return;
      }
    }
  } else if (routeCfg.auto) {
    // explicit opt-in: the old workspace-first behaviour
    try {
      const ensured = await ensureAgentSlot();
      if (ensured && ensured.error) {
        agent.send({ id: msg.id, error: ensured.error });
        return;
      }
      if (ensured) want = ensured.name;
    } catch (e) {
      agent.send({ id: msg.id, error: { code: 'INTERNAL', message: `workspace routing failed: ${String(e?.message || e)}` } });
      return;
    }
  } else if (routeCfg.autoOpenUser) {
    // default: the user's real browser, opened on demand; a running isolated
    // workspace is not a reason to skip it
    try {
      const picked = realSlot(routeCfg);
      if (picked.error) {
        agent.send({ id: msg.id, error: picked.error });
        return;
      }
      if (picked.name) want = picked.name;
      else if (autoOpenAllowed) {
        const ensured = await ensureUserBrowser(routeCfg);
        if (ensured?.error) {
          agent.send({ id: msg.id, error: ensured.error });
          return;
        }
        if (ensured?.name) want = ensured.name;
      }
    } catch (e) {
      agent.send({ id: msg.id, error: { code: 'INTERNAL', message: `user browser routing failed: ${String(e?.message || e)}` } });
      return;
    }
  }
  // want === null → legacy selectExtension rules (single slot, else refusal)
  try {
    dispatchToExtension(agent, msg, want);
  } catch (e) {
    agent.send({ id: msg.id, error: { code: 'INTERNAL', message: `dispatch failed: ${String(e?.message || e)}` } });
  }
}

// ---------------------------------------------------------------- http + ws

const server = http.createServer((req, res) => {
  if (req.url === '/health') {
    const route = agentSlotConfig();
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      ok: true,
      extension: extensions.size > 0,
      browsers: [...extensions.keys()].sort(),
      extension_build: [...extensions.values()][0]?.info?.build ?? null,
      agents: wsAgents.size + 1,
      user_browser: route.userSlot,
      auto_open_user_browser: route.autoOpenUser,
      agent_slot: route.name,
      auto_workspace: route.auto,
    }));
    return;
  }
  if (req.url.startsWith('/__ubb_pair?')) {
    // one-shot pairing page for browser.launch instances: nonce single-use, 120s TTL
    const nonce = new URL(req.url, 'http://127.0.0.1').searchParams.get('nonce') || '';
    const rec = pairNonces.get(nonce);
    if (rec && Date.now() - rec.minted < PAIR_NONCE_TTL_MS) {
      pairNonces.delete(nonce);
      const extDir = String(readConfig().extension_dir || path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..'));
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      res.end(pairPage(TOKEN, rec.name, nonce, unpackedExtId(extDir)));
    } else {
      res.writeHead(404, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ ok: false, error: 'unknown or expired pairing nonce (single-use, 120s TTL)' }));
    }
    return;
  }
  if (req.url.startsWith('/shutdown')) {
    const t = new URL(req.url, 'http://127.0.0.1').searchParams.get('token');
    if (t !== TOKEN) {
      res.writeHead(403).end();
      return;
    }
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ ok: true, shutting_down: true }));
    log('shutdown requested');
    setTimeout(() => process.exit(0), 150);
    return;
  }
  res.writeHead(404).end();
});

function isLoopback(req) {
  const ra = req.socket.remoteAddress || '';
  return ra === '127.0.0.1' || ra === '::1' || ra === '::ffff:127.0.0.1';
}

attachWs(server, {
  onConnection(ws, req, protocol, meta) {
    if (!isLoopback(req)) {
      log('rejected non-loopback connection from', req.socket.remoteAddress);
      ws.close(1008, 'loopback only');
      return;
    }

    let role = null; // 'extension' | 'agent'
    let authenticated = false;
    let authTimer = null;

    const preAuthFrames = [];
    let authDecided = false;

    const finishAuth = (r) => {
      clearTimeout(authTimer);
      authDecided = true;
      authenticated = true;
      role = r;
      if (r === 'extension') {
        // registration waits for hello — the hello's browser_name decides which
        // slot this socket owns; until then it owns nothing
        log('extension connected (awaiting hello)');
      } else {
        wsAgents.add({ ws, send: (o) => ws.send(JSON.stringify(o)) });
        log('agent connected (ws)');
      }
      // replay frames that arrived while auth was being decided (e.g. hello)
      for (const m of preAuthFrames.splice(0)) handleWsFrame(m, ws);
    };

    if (protocol === `ubb-auth.${TOKEN}`) finishAuth('extension');
    else if (protocol === `ubb-agent.${TOKEN}`) finishAuth('agent');
    else if (meta?.offeredAny) {
      // offered a subprotocol but neither matched → reject immediately
      log('rejected bad subprotocol');
      ws.close(1008, 'authentication failed');
      return;
    } else {
      // no subprotocol: first message must be {"method":"auth","params":{"token","role"}}
      authTimer = setTimeout(() => ws.close(1008, 'auth timeout'), 5000);
    }

    ws.onclose = () => {
      clearTimeout(authTimer);
      if (role === 'extension') {
        // whichever slot this socket owned (if hello landed) dies with it
        for (const [name, e] of [...extensions]) {
          if (e.ws === ws) dropExtension(name, 'gone');
        }
      } else if (role === 'agent') {
      for (const a of [...wsAgents]) {
        if (a.ws === ws) {
          wsAgents.delete(a);
          // fail this agent's in-flight calls so it doesn't hang, and ask the
          // owning extension to stop work the agent can no longer receive
          for (const [intId, r] of routes) {
            if (r.agent === a) {
              clearTimeout(r.timer);
              routes.delete(intId);
              const ext = extensions.get(r.browser);
              if (ext) ext.ws.send(JSON.stringify({ method: 'cancel', params: { id: intId } }));
              r.agent.send = () => {};
            }
          }
        }
      }
        log('agent disconnected (ws)');
      }
    };

    ws.onmessage = (text) => {
      let msg;
      try {
        msg = JSON.parse(text);
      } catch {
        return;
      }
      if (!authenticated) {
        if (msg?.method === 'auth' && msg?.params?.token === TOKEN) {
          finishAuth(msg.params.role === 'agent' ? 'agent' : 'extension');
        } else if (authDecided) {
          ws.close(1008, 'authentication failed');
        } else {
          // arrived during the incumbent probe — bounded, never an unbounded
          // memory sink for a client that has not authenticated yet
          if (preAuthFrames.length >= 32) {
            ws.close(1008, 'too many pre-auth frames');
            return;
          }
          preAuthFrames.push(msg);
        }
        return;
      }
      handleWsFrame(msg, ws);
    };

    const handleWsFrame = (msg, socket) => {
      if (role === 'extension') {
        handleExtensionMessage(msg, socket);
      } else {
        const wrapper = [...wsAgents].find((a) => a.ws === ws);
        // handleAgentMessage is async (workspace auto-launch may wait for a
        // pairing slot); a failure inside it answers the caller itself —
        // swallow only the impossible-but-possible rejection here
        Promise.resolve(handleAgentMessage(wrapper || { send: (o) => socket.send(JSON.stringify(o)) }, msg)).catch(() => {});
      }
    };
  },
});

/** Register (or refuse) an extension socket under its hello slot name. A live
 *  incumbent wins the probe; a dead one is replaced — same rule as Rust. */
async function registerExtension(msg, ws) {
  const raw = String(msg.params?.browser_name || '');
  const name = BROWSER_NAME_RE.test(raw) ? raw : 'default';
  const incumbent = extensions.get(name);
  if (incumbent && incumbent.ws !== ws) {
    const alive = incumbent.ws.open ? await incumbent.ws.probe(1500) : false;
    if (alive && incumbent.ws.open) {
      log(`refused duplicate extension connection for browser ${JSON.stringify(name)} (incumbent is alive)`);
      return { name, refused: true };
    }
    log('incumbent extension connection is dead, replacing');
    dropExtension(name, 'replaced');
  }
  const gen = ++extGen;
  extensions.set(name, { ws, info: msg.params || {}, lastFrame: Date.now(), gen });
  log(`extension registered: browser ${JSON.stringify(name)}`);
  broadcast({ method: 'event.extension.connected', params: { browser: name, info: msg.params || {} } });
  return { name, refused: false };
}

async function handleExtensionMessage(msg, ws) {
  if (!msg || typeof msg !== 'object' || Array.isArray(msg)) return;
  const owner = [...extensions.entries()].find(([, e]) => e.ws === ws);
  if (owner) owner[1].lastFrame = Date.now();
  if (msg.method === 'ping') {
    ws.send(JSON.stringify({ method: 'pong' }));
    return;
  }
  if (msg.method === 'hello') {
    const pv = msg.params?.protocol_version || '1.0';
    const min = msg.params?.min_compatible || '1.0';
    if (major(min) > 1) {
      ws.send(JSON.stringify({ id: msg.id, error: { code: 'version_skew', message: `daemon supports 1.x, extension wants ${pv}+` } }));
      return;
    }
    const reg = await registerExtension(msg, ws);
    if (reg.refused) {
      ws.send(JSON.stringify({ id: msg.id, error: { code: 'duplicate_browser_connection', message: `another live extension already holds the browser slot ${JSON.stringify(reg.name)}` } }));
      ws.close(1008, 'duplicate browser connection');
      return;
    }
    // refresh stored info (a renamed profile reconnecting keeps its slot current)
    const e = extensions.get(reg.name);
    if (e) e.info = msg.params || {};
    ws.send(JSON.stringify({ id: msg.id, result: { protocol_version: '1.0', daemon: 'ubb-bridge 1.1.0' } }));
    return;
  }
  if (!owner) return; // frames before a successful hello have no slot to act on
  if (msg.id != null && typeof msg.method === 'string' && EXT_REQ_ID.test(String(msg.id))) {
    answerExtensionRequest(ws, msg);
    return;
  }
  if (msg.id != null && (msg.result !== undefined || msg.error !== undefined)) {
    const route = routes.get(msg.id);
    if (route) {
      routes.delete(msg.id);
      clearTimeout(route.timer);
      const { id: _drop, ...rest } = msg;
      route.agent.send({ id: route.agentId, ...rest });
    }
    return;
  }
  if (msg.method) broadcast(msg); // events → all agents
}

/**
 * Extension→daemon secret RPC. The reply goes back on the extension socket
 * ONLY — it must never reach broadcast()/routes, or a plaintext value would be
 * copied to every connected agent.
 */
function answerExtensionRequest(ws, msg) {
  const reply = (payload) => ws.send(JSON.stringify({ id: msg.id, ...payload }));
  if (msg.method === 'diag.browsers') {
    // extension reverse channel: tool.browser.diagnostics asks the daemon to
    // scan the local machine (ABSORB-CODEX-PLAN §9). Read-only, no vault
    // values — answered on this extension socket ONLY, like every reverse RPC.
    void diagBrowsersResult()
      .then((r) => reply({ result: r }))
      .catch((e) => reply({ error: { code: 'DIAG_FAILED', message: `browser scan failed: ${String(e?.message || e)}` } }));
    return;
  }
  if (!SECRET_RPC.has(msg.method)) {
    log('extension requested a non-secret rpc', msg.method);
    reply({ error: { code: 'RPC_METHOD_NOT_ALLOWED', message: `the daemon answers only diag.browsers and ${[...SECRET_RPC].join(', ')} for extension requests` } });
    return;
  }
  const name = msg.params?.name ?? (Array.isArray(msg.params?.names) ? msg.params.names.join(',') : undefined);
  // traceability without a value: method + name(s) only, never params.value
  log('reverse rpc', msg.method, typeof name === 'string' ? name : '(no name)');
  try {
    if (msg.method === 'secret.resolve') {
      const value = resolveSecret(name);
      if (value == null) {
        throw Object.assign(new Error(`no secret named "${name}" — add it with: ubb secret add ${name}`), { code: 'SECRET_UNKNOWN' });
      }
      reply({ result: { value } });
    } else if (msg.method === 'secret.bindings') {
      // names + origin bindings + mode only — the reply never carries a value
      reply({ result: secretBindings(msg.params?.names) });
    } else if (msg.method === 'secret.list') {
      const info = vaultInfo();
      reply({ result: { names: listNames(), vault_path: info.path, vault_updated_at: info.updated_at } });
    } else if (msg.method === 'secret.set') {
      const r = setSecret(name, msg.params?.value);
      reply({ result: { stored: r.name, bytes: r.bytes } });
    } else {
      const r = deleteSecret(name);
      reply({ result: { deleted: r.name, existed: r.existed } });
    }
  } catch (e) {
    // vault messages name the secret, never its value; log() stays value-free too
    log('secret rpc failed', msg.method, e?.code || 'SECRET_STORE_ERROR', typeof name === 'string' ? name : '');
    reply({ error: { code: e?.code || 'SECRET_STORE_ERROR', message: String(e?.message || e) } });
  }
}

// half-open watchdog: if any slot went silent, probe and drop if dead,
// so agent requests fail fast instead of timing out into a corpse socket
setInterval(async () => {
  for (const [name, e] of [...extensions]) {
    if (!e.ws.open) continue;
    if (Date.now() - (e.lastFrame || 0) < 45000) continue;
    const alive = await e.ws.probe(1200);
    if (alive) {
      e.lastFrame = Date.now();
    } else {
      log('extension idle watchdog: dropping dead connection');
      dropExtension(name, 'dead');
    }
  }
}, 15000);

server.listen(PORT, '127.0.0.1', () => {
  log(`listening on ws://127.0.0.1:${PORT}/ (token ${TOKEN.slice(0, 4)}…)`);
  stdioAgent.send({ method: 'event.daemon.ready', params: { port: PORT } });
});
server.on('error', (e) => {
  log('server error', e.message);
  process.exit(1);
});

// ---------------------------------------------------------------- stdio agent

let stdinBuf = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  stdinBuf += chunk;
  let idx;
  while ((idx = stdinBuf.indexOf('\n')) >= 0) {
    const line = stdinBuf.slice(0, idx).trim();
    stdinBuf = stdinBuf.slice(idx + 1);
      if (line) {
        if (line === '{') continue; // tolerate pretty-printed first brace
        let msg;
        try {
          msg = JSON.parse(line);
        } catch {
          stdioAgent.send({ id: null, error: { code: 'BAD_JSON', message: 'stdin lines must be one JSON object per line' } });
          continue;
        }
        Promise.resolve(handleAgentMessage(stdioAgent, msg)).catch(() => {});
      }
  }
});
process.stdin.on('end', () => {
  log('stdin closed; daemon stays alive for WS agents (use `ubb daemon stop` to stop)');
});

function major(v) {
  return Number(String(v).split('.')[0]) || 0;
}
