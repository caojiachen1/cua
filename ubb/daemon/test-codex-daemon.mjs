// DAEMON wave offline suite (ABSORB-CODEX-PLAN §1/§9/§12) — no browser, no real
// relay, nothing under the user's ~/.ubb is read or written:
//
//   node daemon/test-codex-daemon.mjs
//
// [A] diag.browsers family matrix against throwaway User Data/Preferences
//     fixtures in os.tmpdir() (installed/running/enabled/disabled/parse-fail
//     branches + NM manifest priority registry → appdata → home).
// [B] the tunnel state machine against a local http stub relay + a FAKE
//     extension: start → allow=read denial → allow=all forward → 401 stop.
// [C] static wiring + CLI flag parsing for the two new contributed commands.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { scanBrowsers, UBB_ROOT } from './diag.mjs';
import { CLI_COMMANDS } from './cli/index.mjs';
import { TOOLS as MCP_TOOLS } from './mcp-tools/index.mjs';

const ROOT = path.resolve(fileURLToPath(import.meta.url), '..', '..');

let pass = 0;
let fail = 0;
const ok = (name, cond, detail = '') => {
  if (cond) { pass++; console.log(`  ✓ ${name}`); }
  else { fail++; console.log(`  ✗ ${name}${detail ? ` — ${detail}` : ''}`); }
};

const waitFor = async (fn, ms = 8000, step = 50) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try { if (await fn()) return true; } catch { /* keep waiting */ }
    await new Promise((r) => setTimeout(r, step));
  }
  return false;
};

// ---------------------------------------------------------------- [A] diag

// throwaway roots: every matrix path the scanner builds resolves inside TMP,
// so the user's real browser profiles are never touched.
const TMP = fs.mkdtempSync(path.join(os.tmpdir(), 'ubb-diag-'));
const roots = {
  pf: path.join(TMP, 'pf'),
  pf86: path.join(TMP, 'pf86'),
  local: path.join(TMP, 'local'),
  appdata: path.join(TMP, 'appdata'),
  home: path.join(TMP, 'home'),
};
const dir = (...segs) => { const p = path.join(...segs); fs.mkdirSync(p, { recursive: true }); return p; };
const writeJson = (p, obj) => fs.writeFileSync(p, JSON.stringify(obj));
const pref = (...segs) => {
  const p = path.join(...segs, 'Preferences');
  fs.mkdirSync(path.dirname(p), { recursive: true }); // a profile dir comes into existence with its Preferences
  return p;
};

// the fixture's "unpacked extension path" — deliberately UPPERCASED to prove
// the Preferences `path` comparison is case/slash-insensitive (Windows)
const UPPERCASE_ROOT = UBB_ROOT.toUpperCase();

// chrome: exe + 4 profiles (enabled / disabled / unparsable / absent) + home NM manifest
dir(roots.pf, 'Google', 'Chrome', 'Application');
fs.writeFileSync(path.join(roots.pf, 'Google', 'Chrome', 'Application', 'chrome.exe'), 'exe');
const chromeUd = dir(roots.local, 'Google', 'Chrome', 'User Data');
writeJson(pref(chromeUd, 'Default'), { extensions: { settings: { abcdefghijklmnopqrstuvwxyzabcdef: { path: UPPERCASE_ROOT, state: 1 } } } });
writeJson(pref(chromeUd, 'Disabled Profile'), { extensions: { settings: { abcdefghijklmnopqrstuvwxyzabcdef: { path: UBB_ROOT, state: 0 } } } });
fs.writeFileSync(pref(chromeUd, 'Broken'), '{"extensions":{"settings":'); // unparsable
writeJson(pref(chromeUd, 'Plain'), {}); // a real profile with no UBB entry
dir(roots.home, '.ubb');
writeJson(path.join(roots.home, '.ubb', 'com.ubb.bridge.json'), {
  name: 'com.ubb.bridge', type: 'stdio', path: path.join(roots.pf, 'Google', 'Chrome', 'Application', 'chrome.exe'),
});

// brave: no exe, Preferences "too large" (injected reader), appdata NM manifest
// pointing at a MISSING exe → present but invalid
const braveUd = dir(roots.local, 'BraveSoftware', 'Brave-Browser', 'User Data');
writeJson(pref(braveUd, 'Default'), {});
dir(roots.appdata, 'BraveSoftware', 'Brave-Browser', 'NativeMessagingHosts');
writeJson(path.join(roots.appdata, 'BraveSoftware', 'Brave-Browser', 'NativeMessagingHosts', 'com.ubb.bridge.json'), {
  name: 'com.ubb.bridge', type: 'stdio', path: path.join(TMP, 'missing', 'ubb.exe'),
});

// opera: per-user exe + one profile with a UBB entry that has NO state field
dir(roots.local, 'Programs', 'Opera');
fs.writeFileSync(path.join(roots.local, 'Programs', 'Opera', 'opera.exe'), 'exe');
writeJson(pref(dir(roots.appdata, 'Opera Software', 'Opera Stable')), { extensions: { settings: { abcdefghijklmnopqrstuvwxyzabcdef: { path: UBB_ROOT } } } });

// the injected reader: only brave's Preferences lie about their size, every
// other file (Preferences and NM manifests) is read honestly
const inject = {
  roots,
  readJson: (p) => {
    if (p.includes('BraveSoftware') && p.endsWith('Preferences')) return { error: 'too_large' };
    try { return { json: JSON.parse(fs.readFileSync(p, 'utf8')) }; }
    catch { return { error: 'unparsable' }; }
  },
  runningNames: () => new Set(['chrome.exe', 'msedge.exe']),
  // one reg query per family, stubbed: only the Chrome hive answers, pointing
  // at the valid home manifest (registry beats the appdata convention)
  regQuery: (key) => (key.includes('Google\\Chrome') ? path.join(roots.home, '.ubb', 'com.ubb.bridge.json') : null),
  daemonState: { port: 8765, extension: true },
};

const scan = await scanBrowsers(inject);
const fam = Object.fromEntries(scan.browsers.map((b) => [b.family, b]));
const prof = (b, name) => b.extension.profiles.find((p) => p.name === name);

ok('diag: five families, fixed order, injected daemon state passes through',
  scan.browsers.map((b) => b.family).join(',') === 'edge,chrome,brave,opera,vivaldi' && scan.daemon?.extension === true,
  JSON.stringify(scan.daemon));
ok('diag: scanned_at/repo_root present', typeof scan.scanned_at === 'string' && scan.repo_root === UBB_ROOT, scan.repo_root);

const chrome = fam.chrome;
ok('diag chrome: installed via pf exe, running via injected tasklist',
  chrome.installed === true && chrome.exe_path === path.join(roots.pf, 'Google', 'Chrome', 'Application', 'chrome.exe') && chrome.running === true,
  JSON.stringify({ i: chrome.installed, r: chrome.running, exe: chrome.exe_path }));
ok('diag chrome: uppercase Preferences path still matches the UBB root (case-insensitive)',
  prof(chrome, 'Default')?.ubb === 'enabled' && prof(chrome, 'Default')?.parse === 'ok',
  JSON.stringify(prof(chrome, 'Default')));
ok('diag chrome: state 0 reports disabled', prof(chrome, 'Disabled Profile')?.ubb === 'disabled', JSON.stringify(prof(chrome, 'Disabled Profile')));
ok('diag chrome: unparsable Preferences degrade to unknown, no throw',
  prof(chrome, 'Broken')?.ubb === 'unknown' && prof(chrome, 'Broken')?.parse === 'unparsable', JSON.stringify(prof(chrome, 'Broken')));
ok('diag chrome: a plain profile reports absent', prof(chrome, 'Plain')?.ubb === 'absent', JSON.stringify(prof(chrome, 'Plain')));
ok('diag chrome: extension enabled (one profile) and installed',
  chrome.extension.installed === true && chrome.extension.enabled === true, JSON.stringify(chrome.extension));
ok('diag chrome: NM manifest found via registry, exe ok, valid',
  chrome.nm_manifest.present === true && chrome.nm_manifest.valid === true && chrome.nm_manifest.via === 'registry' &&
  chrome.nm_manifest.exe_ok === true && chrome.nm_manifest.registry_value?.endsWith('com.ubb.bridge.json'),
  JSON.stringify(chrome.nm_manifest));

const edge = fam.edge;
ok('diag edge: no exe → not installed, but process still detected as running',
  edge.installed === false && edge.running === true, JSON.stringify({ i: edge.installed, r: edge.running }));
ok('diag edge: no profile root → scanned false with a note, extension absent',
  edge.extension.profiles.length === 0 && /no profile root/.test(edge.extension.note) && edge.extension.installed === false,
  JSON.stringify(edge.extension));

// The ~/.ubb home fallback is BY DESIGN shared by every family, so the fixture
// home manifest above makes edge/vivaldi "present via home" too. A second scan
// with an empty home is the only honest way to assert the no-manifest shape.
const rootsNoNm = { ...roots, home: dir(TMP, 'home-empty') };
const scanNoNm = await scanBrowsers({ ...inject, roots: rootsNoNm });
const edgeNm = scanNoNm.browsers.find((b) => b.family === 'edge').nm_manifest;
ok('diag edge: no registry, no appdata manifest, no home fallback → present false',
  edgeNm.present === false && edgeNm.registry_value === null, JSON.stringify(edgeNm));

const brave = fam.brave;
ok('diag brave: too_large Preferences degrade to unknown (installed counts it, enabled does not)',
  prof(brave, 'Default')?.parse === 'too_large' && prof(brave, 'Default')?.ubb === 'unknown' &&
  brave.extension.installed === true && brave.extension.enabled === false,
  JSON.stringify(brave.extension));
ok('diag brave: appdata NM manifest found but its exe is missing → present, NOT valid',
  brave.nm_manifest.present === true && brave.nm_manifest.valid === false && brave.nm_manifest.via === 'appdata' && brave.nm_manifest.exe_ok === false,
  JSON.stringify(brave.nm_manifest));

const opera = fam.opera;
ok('diag opera: per-user exe found, not in the process set → running false',
  opera.installed === true && opera.running === false, JSON.stringify({ i: opera.installed, r: opera.running }));
ok('diag opera: UBB entry without state/disable_reasons → enabled（Edge 157 unpacked 实测形态）',
  prof(opera, 'Opera Stable')?.ubb === 'enabled', JSON.stringify(prof(opera, 'Opera Stable')));
ok('diag opera: no registry, no appdata manifest → home fallback (~/.ubb) wins',
  opera.nm_manifest.present === true && opera.nm_manifest.valid === true && opera.nm_manifest.via === 'home',
  JSON.stringify(opera.nm_manifest));

const vivaldi = fam.vivaldi;
const vivaldiNm = scanNoNm.browsers.find((b) => b.family === 'vivaldi').nm_manifest;
ok('diag vivaldi: nothing anywhere → all false, NM present false',
  vivaldi.installed === false && vivaldi.running === false && vivaldi.extension.installed === false && vivaldiNm.present === false,
  JSON.stringify({ i: vivaldi.installed, r: vivaldi.running, nm: vivaldiNm.present }));

// the production scan path (real tasklist + reg query on THIS machine): shape
// only — running may be true/false/null and everything must stay read-only
const live = await scanBrowsers();
ok('diag live: default scan returns the same five families with boolean-or-null running',
  live.browsers.length === 5 && live.browsers.every((b) => b.running === true || b.running === false || b.running === null) &&
  live.browsers.every((b) => typeof b.installed === 'boolean'),
  JSON.stringify(live.browsers.map((b) => [b.family, b.installed, b.running])));

// ---------------------------------------------------------------- [B] tunnel

const freePort = () =>
  new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => { const p = srv.address().port; srv.close(() => resolve(p)); });
  });

const DAEMON_PORT = await freePort();
const RELAY_PORT = await freePort();
const TOKEN = 'f'.repeat(32) + '-tunnel-suite';
// throwaway HOME (inside TMP): the daemon's config reads resolve to the temp
// tree — pinned routing (no auto workspace/user-browser launch) so the tunnel
// test drives the fake extension deterministically.
const FAKE_HOME = dir(TMP, 'daemon-home');
fs.mkdirSync(path.join(FAKE_HOME, '.ubb'), { recursive: true });
fs.writeFileSync(path.join(FAKE_HOME, '.ubb', 'config.json'), JSON.stringify({ auto_open_user_browser: false }));
const CHILD_ENV = { ...process.env, HOME: FAKE_HOME, USERPROFILE: FAKE_HOME };
delete CHILD_ENV.UBB_TOKEN;
delete CHILD_ENV.UBB_PORT;

// stub relay: one command handed out per poll, then 204; POSTs are recorded
let queued = []; // commands to hand out once each
let failStatus = null; // when set, polls answer this status
const polls = [];
const responses = [];
const relay = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    if (req.method === 'POST' && /\/response$/.test(req.url)) {
      try { responses.push(JSON.parse(body || '{}')); } catch { responses.push({ raw: body }); }
      res.writeHead(204);
      res.end();
      return;
    }
    if (req.method === 'GET' && req.url.includes('/poll')) {
      polls.push({ url: req.url, auth: req.headers.authorization || null });
      if (failStatus) { res.writeHead(failStatus); res.end(); return; }
      const cmd = queued.shift();
      if (!cmd) { res.writeHead(204); res.end(); return; }
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(cmd));
      return;
    }
    res.writeHead(404);
    res.end();
  });
});
await new Promise((r) => relay.listen(RELAY_PORT, '127.0.0.1', r));

const daemon = spawn(process.execPath, [path.join(ROOT, 'daemon', 'ubb-bridge.mjs'), '--port', String(DAEMON_PORT), '--token', TOKEN], {
  env: CHILD_ENV,
  stdio: ['pipe', 'pipe', 'pipe'],
  windowsHide: true,
});
let daemonErr = '';
daemon.stderr.on('data', (d) => { daemonErr += d; });
daemon.stdout.setEncoding('utf8');
let outBuf = '';
const frames = [];
daemon.stdout.on('data', (d) => {
  outBuf += d;
  let i;
  while ((i = outBuf.indexOf('\n')) >= 0) {
    const line = outBuf.slice(0, i).trim();
    outBuf = outBuf.slice(i + 1);
    if (line) { try { frames.push(JSON.parse(line)); } catch { /* ignore */ } }
  }
});

let rpcSeq = 100;
const pendingRpc = new Map();
const rpc = (method, params = {}, timeoutMs = 12_000) =>
  new Promise((resolve, reject) => {
    const id = ++rpcSeq;
    const t = setTimeout(() => { pendingRpc.delete(id); reject(new Error(`${method} timed out`)); }, timeoutMs);
    pendingRpc.set(id, (m) => { clearTimeout(t); resolve(m); });
    daemon.stdin.write(JSON.stringify({ id, method, params }) + '\n');
  });
// feed replies from the stdout frame pump to their waiters
const pump = setInterval(() => {
  for (const f of frames.splice(0)) {
    if (f?.id != null && pendingRpc.has(f.id)) {
      const w = pendingRpc.get(f.id);
      pendingRpc.delete(f.id);
      w(f);
    }
  }
}, 25);

const up = await waitFor(async () => (await fetch(`http://127.0.0.1:${DAEMON_PORT}/health`).then((r) => r.ok).catch(() => false)), 10_000);
ok('tunnel: one-shot test daemon is up', up, `daemon stderr: ${daemonErr.slice(0, 200)}`);

// fake extension (slot 'default') — answers exactly one shape: tool.snapshot
let extWs = null;
const extHits = [];
const openExt = new Promise((resolve, reject) => {
  extWs = new WebSocket(`ws://127.0.0.1:${DAEMON_PORT}/`, [`ubb-auth.${TOKEN}`]);
  extWs.addEventListener('open', () => {
    extWs.send(JSON.stringify({ id: 'hello', method: 'hello', params: { min_compatible: '1.0', build: 'test-codex-daemon-fake', browser_name: 'default' } }));
    resolve();
  }, { once: true });
  extWs.addEventListener('error', () => reject(new Error('fake extension could not connect')), { once: true });
  extWs.addEventListener('message', (ev) => {
    const m = JSON.parse(typeof ev.data === 'string' ? ev.data : '');
    if (!m || !m.method) return; // hello reply / pong
    extHits.push(m.method);
    if (m.method === 'tool.snapshot') extWs.send(JSON.stringify({ id: m.id, result: { fake: 'snapshot-result' } }));
    else extWs.send(JSON.stringify({ id: m.id, error: { code: 'FAKE_REFUSED', message: 'the fake extension only answers tool.snapshot' } }));
  });
  setTimeout(() => reject(new Error('fake extension open timed out')), 6000);
});
await openExt;

let st = await rpc('tunnel.status');
ok('tunnel: fresh daemon reports status off (default off, per the plan)',
  st.result?.running === false && st.result?.base === null, JSON.stringify(st.result || st.error));

st = await rpc('tunnel.start', { base_url: 'not a url', token: 'x' });
ok('tunnel: a non-URL base is BAD_PARAMS before anything starts',
  st.error?.code === 'BAD_PARAMS', JSON.stringify(st.error || st.result));
st = await rpc('tunnel.start', { base_url: `http://127.0.0.1:${RELAY_PORT}` });
ok('tunnel: start without a relay token is BAD_PARAMS', st.error?.code === 'BAD_PARAMS', JSON.stringify(st.error || st.result));

st = await rpc('tunnel.start', { base_url: `http://127.0.0.1:${RELAY_PORT}`, token: 'relay-secret-1234', allow: 'read' });
ok('tunnel: start(allow=read) answers running:true with a minted id and a redacted token',
  st.result?.running === true && st.result?.allow === 'read' && /^[a-z0-9-]{5,64}$/.test(st.result?.id || '') &&
  /^.{4}…$/.test(st.result?.token || '') && !(st.result?.token || '').includes('secret'),
  JSON.stringify(st.result || st.error));

// phase 1 — allow=read refuses a write tool BEFORE any slot is touched
queued = [{ id: 'c1', tool: 'fill', params: { ref: '@e1', value: 'nope' } }];
const deniedIn = await waitFor(() => responses.some((r) => r.id === 'c1' && r.error?.code === 'TUNNEL_TOOL_DENIED'), 8000);
ok('tunnel: allow=read forwards nothing — the fill command is refused TUNNEL_TOOL_DENIED at the daemon',
  deniedIn, JSON.stringify(responses.find((r) => r.id === 'c1') || polls));
ok('tunnel: the denied command never reached the extension slot', extHits.length === 0, JSON.stringify(extHits));
ok('tunnel: the poll carries the relay token as Authorization: Bearer',
  polls[0]?.auth === 'Bearer relay-secret-1234' && /\/tunnels\/[^/]+\/poll/.test(polls[0]?.url || ''), JSON.stringify(polls[0]));
ok('tunnel: denial is still one counted command (stats.commands) and does not stop the loop',
  (await rpc('tunnel.status')).result?.stats?.commands === 1, JSON.stringify((await rpc('tunnel.status')).result));

await rpc('tunnel.stop');
st = await rpc('tunnel.status');
ok('tunnel: stop → running:false, stop_reason "stopped by request"',
  st.result?.running === false && st.result?.stop_reason === 'stopped by request', JSON.stringify(st.result));

// phase 2 — allow=all forwards the command through the normal agent routing
queued = [{ id: 'c2', tool: 'tool.snapshot', params: {} }]; // the "tool." prefix is normalized away
st = await rpc('tunnel.start', { base_url: `http://127.0.0.1:${RELAY_PORT}`, token: 'relay-secret-1234', allow: 'all', id: 'suite-tunnel' });
ok('tunnel: restart with allow=all and a caller-chosen id',
  st.result?.running === true && st.result?.allow === 'all' && st.result?.id === 'suite-tunnel', JSON.stringify(st.result || st.error));
const forwarded = await waitFor(() => extHits.includes('tool.snapshot'), 8000);
ok('tunnel: allow=all forwards tool.snapshot to the connected slot through the normal routing',
  forwarded, JSON.stringify(extHits));
const resultIn = await waitFor(() => responses.some((r) => r.id === 'c2' && r.result?.fake === 'snapshot-result'), 8000);
ok('tunnel: the extension result is POSTed back as {id, result} to /tunnels/<id>/response',
  resultIn, JSON.stringify(responses.find((r) => r.id === 'c2') || {}));

// phase 3 — auth failure stops the loop
failStatus = 401;
const stopped = await waitFor(async () => {
  const s = await rpc('tunnel.status');
  return s.result?.running === false && /401/.test(s.result?.stop_reason || '');
}, 10_000);
ok('tunnel: a 401 poll stops the loop with the reason recorded', stopped,
  JSON.stringify((await rpc('tunnel.status')).result));
failStatus = null;

st = await rpc('tunnel.start', { base_url: `http://127.0.0.1:${RELAY_PORT}`, token: 'second', allow: 'read' });
const dup = await rpc('tunnel.start', { base_url: `http://127.0.0.1:${RELAY_PORT}`, token: 'third', allow: 'read' });
ok('tunnel: a second start while running is TUNNEL_ALREADY_RUNNING naming the live tunnel',
  st.result?.running === true && dup.error?.code === 'TUNNEL_ALREADY_RUNNING' && (dup.error?.message || '').includes('tunnel.stop'),
  JSON.stringify(dup.error || dup.result));
await rpc('tunnel.stop');
ok('tunnel: stop after the 401-stopped state still answers honestly (running:false)',
  (await rpc('tunnel.status')).result?.running === false, JSON.stringify((await rpc('tunnel.status')).result));

// diag.browsers through the real agent socket (daemonMethod side of §9)
st = await rpc('diag.browsers');
ok('diag.browsers via the agent channel: daemon state + five families + slots',
  st.result?.daemon?.port_ok === true && Array.isArray(st.result?.slots) &&
  st.result?.browsers?.map((b) => b.family).join(',') === 'edge,chrome,brave,opera,vivaldi',
  JSON.stringify(st.error || { d: st.result?.daemon, n: st.result?.browsers?.length }));

clearInterval(pump);
relay.close();
daemon.kill();
extWs?.close?.();

// ---------------------------------------------------------------- [C] wiring

ok('wiring: CLI_COMMANDS gained diagnostics + tunnel with describe text',
  CLI_COMMANDS.some((c) => c.name === 'diagnostics' && /no token/.test(c.describe)) &&
  CLI_COMMANDS.some((c) => c.name === 'tunnel' && /default off/.test(c.describe)),
  JSON.stringify(CLI_COMMANDS.map((c) => c.name)));
ok('wiring: MCP catalog gained ubb_diagnostics mapping to browser.diagnostics',
  MCP_TOOLS.some((t) => t.name === 'ubb_diagnostics' && t.ubb === 'browser.diagnostics'),
  JSON.stringify(MCP_TOOLS.filter((t) => t.name.startsWith('ubb_diag'))));

const runCli = (args) =>
  new Promise((resolve) => {
    const c = spawn(process.execPath, [path.join(ROOT, 'daemon', 'ubb.mjs'), ...args], {
      env: CHILD_ENV, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
    });
    let out = '';
    let err = '';
    c.stdout.on('data', (d) => { out += d; });
    c.stderr.on('data', (d) => { err += d; });
    const timer = setTimeout(() => { try { c.kill(); } catch {} resolve({ code: -1, out, err: `${err}\n(timeout)` }); }, 30_000);
    c.on('exit', (code) => { clearTimeout(timer); resolve({ code, out, err }); });
  });

const usage = await runCli(['tunnel']);
ok('cli: bare `ubb tunnel` prints usage and exits 2',
  usage.code === 2 && /usage: ubb tunnel start --base-url/.test(usage.err), JSON.stringify({ code: usage.code, err: usage.err.slice(0, 120) }));
const noToken = await runCli(['tunnel', 'status']);
ok('cli: `ubb tunnel status` without a daemon token fails honestly (relay --token is NOT the daemon token)',
  noToken.code !== 0 && /no daemon token/.test(noToken.err), JSON.stringify({ code: noToken.code, err: noToken.err.slice(0, 160) }));
const noStart = await runCli(['tunnel', 'start', '--base-url', 'http://x.example', '--token', 'relaytok']);
ok('cli: `tunnel start` keeps --token for the relay and still demands daemon creds from env/config',
  noStart.code !== 0 && /no daemon token/.test(noStart.err), JSON.stringify({ code: noStart.code, err: noStart.err.slice(0, 160) }));

const diagJson = await runCli(['diagnostics', '--json']);
let parsed = null;
try { parsed = JSON.parse(diagJson.out); } catch { /* reported below */ }
ok('cli: `ubb diagnostics --json` emits the full scan shape (5 families + daemon section), exit 0',
  diagJson.code === 0 && parsed?.browsers?.length === 5 && parsed?.browsers?.[0]?.family === 'edge' &&
  typeof parsed?.daemon === 'object' && typeof parsed?.daemon?.port === 'number',
  JSON.stringify({ code: diagJson.code, keys: parsed ? Object.keys(parsed) : String(diagJson.out).slice(0, 120), err: diagJson.err.slice(0, 120) }));
const diagHuman = await runCli(['diagnostics']);
ok('cli: `ubb diagnostics` human output names every family and the daemon block',
  diagHuman.code === 0 && /\[edge\] Microsoft Edge/.test(diagHuman.out) && /\[vivaldi\] Vivaldi/.test(diagHuman.out) && /\[daemon\] port=/.test(diagHuman.out),
  diagHuman.out.slice(0, 200));

fs.rmSync(TMP, { recursive: true, force: true });

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
