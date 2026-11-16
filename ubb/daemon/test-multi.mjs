// multi-browser routing suite: two fake extensions (distinct hello browser
// names) + an agent against a REAL daemon process. This is the wire-level
// contract the Rust daemon mirrors (its own tests cover the same semantics
// in Rust). Run: node daemon/test-multi.mjs

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..');
const PORT = 18800 + Math.floor(Math.random() * 400);
const TOKEN = 'a'.repeat(16) + Math.random().toString(16).slice(2, 10);
const URL_WS = `ws://127.0.0.1:${PORT}/`;

let passed = 0;
let failed = 0;
const failures = [];
function ok(name, cond, extra) {
  if (cond) {
    passed += 1;
    console.log(`  ✓ ${name}`);
  } else {
    failed += 1;
    failures.push(name);
    console.log(`  ✗ ${name}${extra ? ` — ${extra}` : ''}`);
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function wsConnect(url, protocols) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, protocols);
    ws.addEventListener('open', () => resolve(ws), { once: true });
    ws.addEventListener('error', () => reject(new Error('connect failed')), { once: true });
    setTimeout(() => reject(new Error('connect timeout')), 4000);
  });
}

function rpc(ws, method, params, { browser = undefined, timeoutMs = 5000, id = Math.floor(Math.random() * 1e6) } = {}) {
  return new Promise((resolve, reject) => {
    const envelope = { id, method, params };
    if (browser !== undefined) envelope.browser = browser;
    const timer = setTimeout(() => reject(new Error(`rpc timeout: ${method}`)), timeoutMs);
    const onMsg = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id === id) {
        clearTimeout(timer);
        ws.removeEventListener('message', onMsg);
        resolve(m);
      }
    };
    ws.addEventListener('message', onMsg);
    ws.send(JSON.stringify(envelope));
  });
}

function makeFakeExtension(answers) {
  // answers: Map method → (params) => result
  const hitLog = [];
  return {
    hitLog,
    async run() {
      const ws = await wsConnect(URL_WS, [`ubb-auth.${TOKEN}`]);
      this.ws = ws;
      ws.addEventListener('message', (ev) => {
        const m = JSON.parse(ev.data);
        if (!m.method || m.id == null) return;
        hitLog.push({ method: m.method, params: m.params });
        const fn = answers.get(m.method);
        ws.send(JSON.stringify({ id: m.id, ...(fn ? { result: fn(m.params) } : { result: { ok: true, slot: this.slotName } }) }));
      });
      // register under our slot name, exactly like the real ws-client does
      ws.send(JSON.stringify({
        id: 'hello',
        method: 'hello',
        params: {
          min_compatible: '1.0',
          build: 'test-multi-fake',
          browser_name: this.slotName,
          instance_id: this.slotName + '-' + Math.random().toString(16).slice(2),
          workspace: this.workspace === true,
        },
      }));
      await sleep(300); // let the daemon process the hello
      return ws;
    },
    slotName: null,
    workspace: false,
  };
}

// The daemon reads ~/.ubb/config.json on every routing decision, so the suite
// points its HOME at a temp dir and rewrites the file between sections.
const tmpHome = fs.mkdtempSync(path.join(os.tmpdir(), 'ubb-multi-'));
const configPath = path.join(tmpHome, '.ubb', 'config.json');
function writeConfig(obj) {
  fs.mkdirSync(path.dirname(configPath), { recursive: true });
  fs.writeFileSync(configPath, JSON.stringify(obj));
}
// sections [1]-[5] pin the LEGACY routing rules; workspace mode gets [6]+
writeConfig({ auto_workspace: false, auto_open_user_browser: false });

const daemon = spawn(process.execPath, [
  path.join(ROOT, 'daemon', 'ubb-bridge.mjs'),
  '--port', String(PORT),
  '--token', TOKEN,
], {
  stdio: ['ignore', 'ignore', 'pipe'],
  env: { ...process.env, USERPROFILE: tmpHome, HOME: tmpHome },
});
let daemonErr = '';
daemon.stderr.on('data', (d) => { daemonErr += String(d); });

const agents = [];
const cleanup = () => {
  for (const ws of agents.splice(0)) {
    try { ws.close(); } catch {}
  }
  try { daemon.kill(); } catch {}
  try { fs.rmSync(tmpHome, { recursive: true, force: true }); } catch {}
};

try {
  let up = false;
  for (let i = 0; i < 60 && !(up = await fetch(`http://127.0.0.1:${PORT}/health`).then((r) => r.json()).then((h) => h.ok).catch(() => false)); i++) await sleep(100);
  ok('daemon is up', up, daemonErr.slice(0, 200));
  if (!up) throw new Error('daemon never came up');

  console.log('\n[1] slots register from hello');
  const work = makeFakeExtension(new Map([
    ['tool.echo', (p) => ({ echoed: p, slot: 'work' })],
  ]));
  work.slotName = 'work';
  const wsWork = await work.run();

  let r = await rpc(await agent(), 'meta.browsers', {});
  ok('one slot listed after hello', r.result?.browsers?.length === 1 && r.result.browsers[0].name === 'work', JSON.stringify(r.result));

  const home = makeFakeExtension(new Map());
  home.slotName = 'default';
  await home.run();
  r = await rpc(await agent(), 'meta.browsers', {});
  ok('two slots listed', r.result?.browsers?.length === 2 && r.result.browsers.map((b) => b.name).sort().join(',') === 'default,work', JSON.stringify(r.result));

  r = await fetch(`http://127.0.0.1:${PORT}/health`).then((x) => x.json());
  ok('/health names both browsers', Array.isArray(r.browsers) && r.browsers.join(',') === 'default,work', JSON.stringify(r));

  console.log('\n[2] routing');
  r = await rpc(await agent(), 'tool.echo', { ping: 1 }, { browser: 'work' });
  ok('named routing reaches the named slot', r.result?.slot === 'work' && r.result?.echoed?.ping === 1, JSON.stringify(r));
  ok('the other slot stayed silent', !home.hitLog.some((h) => h.method === 'tool.echo'));

  r = await rpc(await agent(), 'tool.echo', { ping: 2 }, { browser: 'nope' });
  ok('unknown slot → BROWSER_NOT_FOUND listing what exists', r.error?.code === 'BROWSER_NOT_FOUND' && /work/.test(r.error.message), JSON.stringify(r));

  r = await rpc(await agent(), 'tool.echo', { ping: 3 }, { browser: undefined });
  ok('no browser with two slots → BROWSER_REQUIRED', r.error?.code === 'BROWSER_REQUIRED', JSON.stringify(r));

  r = await rpc(await agent(), 'tool.echo', { browser: 'work', ping: 4 });
  ok('params.browser routes too (and is stripped before the tool sees it)', r.result?.slot === 'work' && r.result?.echoed && r.result.echoed.browser === undefined && r.result.echoed.ping === 4, JSON.stringify(r.result));

  console.log('\n[3] teardown of one slot leaves the other intact');
  wsWork.close();
  await sleep(400);
  r = await rpc(await agent(), 'tool.echo', { ping: 5 }, { browser: 'work' });
  ok('a dead slot refuses with BROWSER_NOT_FOUND (no corpse routing)', r.error?.code === 'BROWSER_NOT_FOUND', JSON.stringify(r));
  r = await rpc(await agent(), 'tool.echo', { ping: 6 }, { browser: 'default' });
  ok('the surviving slot still answers', r.result?.slot === 'default', JSON.stringify(r));

  console.log('\n[4] single-slot back-compat');
  // close the default fake extension, reconnect a single one without routing hints
  r = await rpc(await agent(), 'meta.browsers', {});
  ok('meta.browsers reflects exactly one slot', r.result?.browsers?.length === 1, JSON.stringify(r.result));

  console.log('\n[5] pair.register + one-shot pair page');
  r = await rpc(await agent(), 'pair.register', { name: 'scratch' });
  const nonce = r.result?.nonce;
  ok('pair.register mints a 64-hex nonce', /^[0-9a-f]{64}$/.test(nonce || ''), JSON.stringify(r));
  const page = await fetch(`http://127.0.0.1:${PORT}/__ubb_pair?nonce=${nonce}`).then((x) => x.text());
  ok('the pair page embeds the credentials for the launched profile', page.includes('UBB_PAIR_LAUNCH') && page.includes(TOKEN) && page.includes('"scratch"'), page.slice(0, 120));
  const replay = await fetch(`http://127.0.0.1:${PORT}/__ubb_pair?nonce=${nonce}`);
  ok('the nonce is single-use (replay → 404)', replay.status === 404, `status ${replay.status}`);
  const bogus = await fetch(`http://127.0.0.1:${PORT}/__ubb_pair?nonce=${'b'.repeat(64)}`);
  ok('an unknown nonce is a 404 (no credential leak)', bogus.status === 404, `status ${bogus.status}`);

  console.log('\n[6] workspace mode: nameless calls target the agent slot, never the user\'s browser');
  writeConfig({ auto_workspace: true, agent_browser: 'agent' });
  const workspace = makeFakeExtension(new Map([
    ['tool.echo', (p) => ({ echoed: p, slot: 'agent' })],
  ]));
  workspace.slotName = 'agent';
  const wsWorkspace = await workspace.run();
  const homeHitsBefore = home.hitLog.length;
  r = await rpc(await agent(), 'tool.echo', { ping: 10 });
  ok('nameless call lands in the workspace slot (no BROWSER_REQUIRED)', r.result?.slot === 'agent', JSON.stringify(r));
  ok('the user\'s slot stayed silent', home.hitLog.length === homeHitsBefore, JSON.stringify(home.hitLog.slice(-3)));
  r = await rpc(await agent(), 'tool.echo', { ping: 11 }, { browser: 'default' });
  ok('an explicit hint still wins over the workspace default', r.result?.slot === 'default', JSON.stringify(r));

  console.log('\n[7] missing workspace: daemon spawns + waits, user slots untouched');
  const hitsBefore = home.hitLog.length;
  wsWorkspace.close();
  await sleep(400);
  // browser_exe = the node binary: `node --user-data-dir=…` exits instantly on
  // an unknown option — the launch really spawns, nothing pairs, no window
  writeConfig({ auto_workspace: true, agent_browser: 'agent', browser_exe: process.execPath, workspace_wait_ms: 800 });
  r = await rpc(await agent(), 'tool.echo', { ping: 12 }, { timeoutMs: 8000 });
  ok('spawn+wait ends in an honest AGENT_LAUNCH_TIMEOUT', r.error?.code === 'AGENT_LAUNCH_TIMEOUT', JSON.stringify(r));
  ok('the user\'s slot never received the nameless call', home.hitLog.length === hitsBefore, JSON.stringify(home.hitLog.slice(-3)));

  console.log('\n[8] launch machinery failure is honest — never a silent grab of the user\'s browser');
  writeConfig({ auto_workspace: true, agent_browser: 'agent', browser_exe: 'Z:/definitely/not/here.exe' });
  r = await rpc(await agent(), 'tool.echo', { ping: 13 });
  ok('no exe → BROWSER_EXE_NOT_FOUND (surfaced, not papered over)', r.error?.code === 'BROWSER_EXE_NOT_FOUND', JSON.stringify(r));
  ok('the user\'s slot stayed untouched', home.hitLog.length === hitsBefore, JSON.stringify(home.hitLog.slice(-3)));

  console.log('\n[9] both automations off restores the legacy rules; status names the routing target');
  writeConfig({ auto_workspace: false, auto_open_user_browser: false });
  const work2 = makeFakeExtension(new Map());
  work2.slotName = 'work';
  const wsWork2 = await work2.run();
  r = await rpc(await agent(), 'tool.echo', { ping: 14 });
  ok('two user slots + no name → BROWSER_REQUIRED again', r.error?.code === 'BROWSER_REQUIRED', JSON.stringify(r));
  r = await rpc(await agent(), 'status', {});
  ok('status names the routing target and the mode', r.result?.agent_slot === 'agent' && r.result?.auto_workspace === false, JSON.stringify(r.result));

  console.log('\n[10] default routing prefers the real browser and auto-opens its normal profile');
  try { home.ws.close(); } catch {}
  try { wsWork2.close(); } catch {}
  await sleep(400);
  // no slots: an exe that exits immediately proves the daemon really tries to
  // start the NORMAL profile and reports USER_BROWSER_LAUNCH_TIMEOUT instead of
  // silently spawning an isolated workspace
  writeConfig({
    auto_workspace: false, auto_open_user_browser: true, user_browser: 'default',
    user_browser_exe: process.execPath, user_browser_wait_ms: 700,
  });
  r = await rpc(await agent(), 'tool.echo', { ping: 20 }, { timeoutMs: 5000 });
  ok('missing user browser → USER_BROWSER_LAUNCH_TIMEOUT (no workspace fallback)',
    r.error?.code === 'USER_BROWSER_LAUNCH_TIMEOUT', JSON.stringify(r));
  r = await rpc(await agent(), 'meta.browsers', {});
  ok('the failed user-browser launch left no slot behind', r.result?.browsers?.length === 0, JSON.stringify(r.result));

  const real = makeFakeExtension(new Map([['tool.echo', (p) => ({ echoed: p, slot: 'default' })]]));
  real.slotName = 'default';
  const wsReal = await real.run();
  const isolated = makeFakeExtension(new Map([['tool.echo', (p) => ({ echoed: p, slot: 'agentx' })]]));
  isolated.slotName = 'agentx';
  isolated.workspace = true;
  const wsIsolated = await isolated.run();
  r = await rpc(await agent(), 'tool.echo', { ping: 21 });
  ok('nameless call prefers the real default slot over an isolated workspace', r.result?.slot === 'default', JSON.stringify(r));
  ok('the isolated workspace received nothing', !isolated.hitLog.some((h) => h.method === 'tool.echo'), JSON.stringify(isolated.hitLog.slice(-3)));
  r = await rpc(await agent(), 'status', {});
  ok('status names the real-browser target and auto-open mode',
    r.result?.user_browser === 'default' && r.result?.auto_open_user_browser === true, JSON.stringify(r.result));
  try { wsReal.close(); } catch {}
  try { wsIsolated.close(); } catch {}

  async function agent() {
    // one WS agent per call keeps message routing dead simple in this suite
    const ws = await wsConnect(URL_WS, [`ubb-agent.${TOKEN}`]);
    agents.push(ws);
    return ws;
  }
} catch (e) {
  failed += 1;
  failures.push('suite error: ' + e.message);
  console.error('SUITE ERROR:', e);
} finally {
  cleanup();
}

console.log(`\n=== test-multi: ${passed} passed, ${failed} failed ===`);
if (failed) {
  console.error('FAILED:', failures.join(' · '));
  console.error('daemon stderr tail:', daemonErr.slice(-600));
  process.exit(1);
}
