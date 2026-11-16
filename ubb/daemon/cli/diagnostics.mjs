// `ubb diagnostics` — read-only multi-browser install scan + daemon health
// (ABSORB-CODEX-PLAN §9). Runs BEFORE the token gate (see daemon/ubb.mjs): a
// fresh machine has no token yet, and this is exactly the command you run there
// to find out why pairing never happens. Nothing here needs a token; the
// daemon's /health is probed opportunistically and its absence never blocks
// the scan.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { scanBrowsers } from '../diag.mjs';

const CONFIG_PATH = path.join(os.homedir(), '.ubb', 'config.json');

// daemon/ubb.mjs splices --token/--port out of process.argv before a
// contributed run() is reached (the audit.mjs rawFlag problem), so the untouched
// argv has to be captured at module-load time — imports evaluate first, before
// ubb.mjs's body runs its flag() splices.
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

function daemonPort() {
  return Number(rawArg('daemon-port') || rawArg('port') || process.env.UBB_PORT || readConfig().port || 8765);
}

async function fetchHealth(port, ms = 900) {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/health`, { signal: AbortSignal.timeout(ms) });
    if (!r.ok) return null;
    return await r.json();
  } catch {
    return null;
  }
}

const mark = (v) => (v === true ? 'yes' : v === false ? 'no' : 'unknown');

function printFamily(b) {
  const ext = b.extension;
  const states = ext.profiles.length
    ? ext.profiles.map((p) => `${p.name}=${p.ubb}${p.parse !== 'ok' ? `(${p.parse})` : ''}`).join(', ')
    : ext.note || 'no profiles scanned';
  console.log(`[${b.family}] ${b.display}`);
  console.log(`  installed=${mark(b.installed)}  running=${mark(b.running)}  manage=${b.manage_url}`);
  if (b.exe_path) console.log(`  exe: ${b.exe_path}`);
  console.log(`  extension: installed=${mark(ext.installed)} enabled=${mark(ext.enabled)}${ext.profiles.length ? ` (${ext.profiles.length} profile(s))` : ''}`);
  console.log(`  profiles: ${states}`);
  const nm = b.nm_manifest;
  console.log(`  nm manifest: present=${mark(nm.present)} valid=${mark(nm.valid)}${nm.via ? ` via ${nm.via}` : ''}${nm.manifest_path_field ? ` exe=${nm.manifest_path_field}(${nm.exe_ok ? 'ok' : 'MISSING'})` : ''}`);
}

async function runDiagnostics(argv) {
  const json = argv.includes('--json');
  const port = daemonPort();
  const [scan, health] = await Promise.all([scanBrowsers(), fetchHealth(port)]);
  const out = {
    ...scan,
    daemon: health
      ? { reachable: true, port, ...health }
      : { reachable: false, port, note: `no daemon on 127.0.0.1:${port} — start it with: ubb daemon start` },
  };
  if (json) {
    console.log(JSON.stringify(out, null, 2));
    return;
  }
  console.log(`UBB diagnostics — ${out.scanned_at}`);
  console.log(`unpacked UBB extension dir: ${out.repo_root}`);
  console.log('');
  for (const b of out.browsers) {
    printFamily(b);
    console.log('');
  }
  const d = out.daemon;
  console.log(`[daemon] port=${d.port} reachable=${mark(d.reachable)}`);
  if (d.reachable) {
    console.log(`  extension_connected=${mark(d.extension)}  slots=${Array.isArray(d.browsers) && d.browsers.length ? d.browsers.join(', ') : '(none)'}`);
    console.log(`  agent_workspace=${d.agent_slot || '(default)'}  user_browser=${d.user_browser || '(default)'}`);
  } else {
    console.log(`  ${d.note}`);
  }
}

export const DIAGNOSTICS_COMMANDS = [
  {
    name: 'diagnostics',
    describe: 'diagnostics [--json] — read-only multi-browser UBB install scan (installed/running/extension/nm-manifest) + daemon health; no token needed',
    async run(argv) {
      try {
        await runDiagnostics(argv);
      } catch (e) {
        console.error(`ubb diagnostics failed: ${String(e?.message || e)}`);
        process.exitCode = 1;
      }
    },
  },
];
