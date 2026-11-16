// `ubb tunnel` — opt-in Codex-style long-polling MCP relay client, default OFF
// (ABSORB-CODEX-PLAN §0.2/§9). SECURITY: starting a tunnel exposes the daemon's
// tool surface to the REMOTE base-url named here. Nothing starts it for you,
// nothing persists it: the state lives in the running daemon's memory only and
// dies with the daemon (`ubb tunnel status` on a fresh daemon reports off).
// Default allow=read forwards only the read-only perception whitelist;
// --allow all forwards the full surface (writes included — the extension's own
// confirm/policy gates still apply per command) and prints a hard warning.
//
// NOTE: --token here is the RELAY token, NOT the daemon token. daemon/ubb.mjs
// splices --token out of argv before this run(), so the relay token is read
// from the untouched argv captured at module load, and the daemon side never
// borrows it (daemon creds come from env/config only).

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { AgentClient } from './audit.mjs';

const CONFIG_PATH = path.join(os.homedir(), '.ubb', 'config.json');

const RAW_ARGV = process.argv.slice(2);
const rawArg = (name) => {
  const i = RAW_ARGV.indexOf(`--${name}`);
  return i >= 0 && RAW_ARGV[i + 1] !== undefined ? RAW_ARGV[i + 1] : undefined;
};

function readConfig() {
  try {
    return JSON.parse(fs.readFileSync(CONFIG_PATH, 'utf8'));
  } catch {
    return {};
  }
}

function daemonCreds() {
  const cfg = readConfig();
  const token = process.env.UBB_TOKEN || cfg.token;
  const port = Number(rawArg('daemon-port') || rawArg('port') || process.env.UBB_PORT || cfg.port || 8765);
  if (!token) throw new Error('no daemon token — set UBB_TOKEN or run `ubb config token <token>` first (--token on `tunnel start` is the RELAY token, a different thing)');
  return { token, port };
}

const USAGE = 'usage: ubb tunnel start --base-url URL --token TOKEN [--allow read|all] [--id <tunnel-id>]\n       ubb tunnel status\n       ubb tunnel stop';

async function withClient(fn) {
  const c = new AgentClient(daemonCreds());
  await c.connect();
  try {
    return await fn(c);
  } finally {
    c.close();
  }
}

async function runStart(argv) {
  const base = rawArg('base-url');
  const relayToken = rawArg('token');
  const allow = rawArg('allow') || 'read';
  if (!base || !relayToken) {
    console.error(USAGE);
    process.exit(2);
  }
  if (!['read', 'all'].includes(allow)) {
    console.error(`ubb tunnel: --allow must be "read" or "all", got "${allow}"`);
    process.exit(2);
  }
  if (allow === 'all') {
    // the plan's mandated warning — printed regardless of what the daemon logs
    console.error('WARNING: --allow all forwards the FULL tool surface (writes, secret workflow, cdp) to the remote relay. Every command still passes the extension confirm/policy gates. Stop with: ubb tunnel stop');
  }
  const params = { base_url: base, token: relayToken, allow };
  const id = rawArg('id');
  if (id) params.id = id;
  const r = await withClient((c) => c.call('tunnel.start', params, 20_000));
  if (r.error) {
    console.error(JSON.stringify(r.error));
    process.exitCode = 2;
    return;
  }
  console.log(JSON.stringify(r.result, null, 2));
}

async function runStatus() {
  const r = await withClient((c) => c.call('tunnel.status', {}, 15_000));
  if (r.error) {
    console.error(JSON.stringify(r.error));
    process.exitCode = 2;
    return;
  }
  console.log(JSON.stringify(r.result, null, 2));
}

async function runStop() {
  const r = await withClient((c) => c.call('tunnel.stop', {}, 15_000));
  if (r.error) {
    console.error(JSON.stringify(r.error));
    process.exitCode = 2;
    return;
  }
  console.log(JSON.stringify(r.result, null, 2));
}

export const TUNNEL_COMMANDS = [
  {
    name: 'tunnel',
    describe: 'tunnel start --base-url URL --token TOKEN [--allow read|all] [--id <tunnel-id>] | status | stop — opt-in long-polling MCP relay to the daemon (default off; --token is the RELAY token)',
    async run(argv) {
      const sub = argv[0];
      try {
        if (sub === 'start') await runStart(argv);
        else if (sub === 'status') await runStatus();
        else if (sub === 'stop') await runStop();
        else {
          console.error(USAGE);
          process.exit(2);
        }
      } catch (e) {
        console.error(`ubb tunnel: ${String(e?.message || e)}`);
        process.exitCode = 1;
      }
    },
  },
];
