// daemon/diag.mjs — read-only multi-browser diagnostics scan (ABSORB-CODEX-PLAN §9).
//
// Answers "why is the extension not connected" without touching anything: which
// Chromium-family browsers are installed and running, whether the unpacked UBB
// extension is present+enabled in their profiles (Preferences / Secure Preferences JSON), and whether
// the native-messaging manifest (com.ubb.bridge) resolves to a live bin/ubb.exe.
//
// Everything here is READ-ONLY: process listing (one tasklist call, filtered
// locally), directory listings, JSON reads, one `reg query` per family. A broken
// profile reports parse:'unparsable'/'too_large'/'unreadable' instead of failing
// the scan, and an unreadable piece degrades to null/'unknown', never a throw.
//
// scanBrowsers() takes no arguments in production; tests inject fakes through
// the same `inject` object (roots, isFile, readPrefs, runningNames, regQuery).

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const execFileP = promisify(execFile);

// the unpacked extension lives at <repo>/ (this daemon in <repo>/daemon):
// Preferences `extensions.settings` entries whose `path` points here ARE UBB
export const UBB_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// Preferences can be tens of MB and the extension block sits deep inside.
// Reading it whole is simple; the cap keeps a pathological file from eating
// memory — bigger files are reported too_large instead of parsed.
const MAX_PREFS_BYTES = 32 * 1024 * 1024;
const MAX_PROFILES_PER_FAMILY = 20;

// ------------------------------------------------- family matrix (Windows only)
// Per family: install-path candidates (first segment = root key: pf/pf86/local),
// process image name, profile root (Chromium "User Data" layout; Opera keeps its
// profiles directly under %APPDATA%\Opera Software), the %APPDATA% NM manifest
// dir, the HKCU NativeMessagingHosts registry key and the extension manager URL.
// Matrix data mirrors the Codex extension-ids.json approach: hard-coded here.
const FAMILIES = [
  {
    family: 'edge',
    display: 'Microsoft Edge',
    process: 'msedge.exe',
    manage_url: 'edge://extensions',
    exe: [['pf86', 'Microsoft', 'Edge', 'Application', 'msedge.exe'], ['pf', 'Microsoft', 'Edge', 'Application', 'msedge.exe']],
    user_data: ['local', 'Microsoft', 'Edge', 'User Data'],
    nm_dir: ['appdata', 'Microsoft', 'Edge', 'NativeMessagingHosts'],
    reg_key: 'HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.ubb.bridge',
  },
  {
    family: 'chrome',
    display: 'Google Chrome',
    process: 'chrome.exe',
    manage_url: 'chrome://extensions',
    exe: [['pf', 'Google', 'Chrome', 'Application', 'chrome.exe'], ['pf86', 'Google', 'Chrome', 'Application', 'chrome.exe'], ['local', 'Google', 'Chrome', 'Application', 'chrome.exe']],
    user_data: ['local', 'Google', 'Chrome', 'User Data'],
    nm_dir: ['appdata', 'Google', 'Chrome', 'NativeMessagingHosts'],
    reg_key: 'HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.ubb.bridge',
  },
  {
    family: 'brave',
    display: 'Brave',
    process: 'brave.exe',
    manage_url: 'brave://extensions',
    exe: [['pf', 'BraveSoftware', 'Brave-Browser', 'Application', 'brave.exe'], ['pf86', 'BraveSoftware', 'Brave-Browser', 'Application', 'brave.exe'], ['local', 'BraveSoftware', 'Brave-Browser', 'Application', 'brave.exe']],
    user_data: ['local', 'BraveSoftware', 'Brave-Browser', 'User Data'],
    nm_dir: ['appdata', 'BraveSoftware', 'Brave-Browser', 'NativeMessagingHosts'],
    reg_key: 'HKCU\\Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts\\com.ubb.bridge',
  },
  {
    family: 'opera',
    display: 'Opera',
    process: 'opera.exe',
    manage_url: 'opera://extensions',
    exe: [['local', 'Programs', 'Opera', 'opera.exe'], ['local', 'Programs', 'Opera GX', 'opera.exe'], ['pf', 'Opera', 'opera.exe']],
    user_data: ['appdata', 'Opera Software'],
    nm_dir: ['appdata', 'Opera Software', 'NativeMessagingHosts'],
    reg_key: 'HKCU\\Software\\Opera Software\\NativeMessagingHosts\\com.ubb.bridge',
  },
  {
    family: 'vivaldi',
    display: 'Vivaldi',
    process: 'vivaldi.exe',
    manage_url: 'vivaldi://extensions',
    exe: [['local', 'Vivaldi', 'Application', 'vivaldi.exe'], ['pf', 'Vivaldi', 'Application', 'vivaldi.exe']],
    user_data: ['local', 'Vivaldi', 'User Data'],
    nm_dir: ['appdata', 'Vivaldi', 'NativeMessagingHosts'],
    reg_key: 'HKCU\\Software\\Vivaldi\\NativeMessagingHosts\\com.ubb.bridge',
  },
];

// ---------------------------------------------------------------- default IO

function defaultRoots() {
  const home = os.homedir();
  return {
    pf: process.env.ProgramFiles || 'C:\\Program Files',
    pf86: process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)',
    local: process.env.LOCALAPPDATA || path.join(home, 'AppData', 'Local'),
    appdata: process.env.APPDATA || path.join(home, 'AppData', 'Roaming'),
    home,
  };
}

const joinSegs = (roots, segs) => path.join(String(roots[segs[0]] ?? ''), ...segs.slice(1));

/** Path compare tolerant to the slash direction / casing / trailing separator
 *  Chromium writes into Preferences `path` fields. */
function normPath(p) {
  return String(p || '').replace(/[/\\]+/g, '\\').replace(/\\+$/, '').toLowerCase();
}

function isUbbUnpacked(p) {
  return !!p && normPath(p) === normPath(UBB_ROOT);
}

function defaultIsFile(p) {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

function defaultListDirs(dir) {
  try {
    return fs.readdirSync(dir, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name);
  } catch {
    return null; // no profile root at all (browser family not used)
  }
}

/** One capped JSON reader for Preferences AND NM manifests: never throws. */
function defaultReadJson(p) {
  let st;
  try {
    st = fs.statSync(p);
    if (!st.isFile()) return { error: 'unreadable' };
    if (st.size > MAX_PREFS_BYTES) return { error: 'too_large', bytes: st.size };
  } catch {
    return { error: 'unreadable' };
  }
  try {
    return { json: JSON.parse(fs.readFileSync(p, 'utf8')) };
  } catch {
    return { error: 'unparsable' };
  }
}

/** ONE tasklist call, filtered locally to the matrix process names. `null`
 *  means detection itself failed — per-family `running` becomes null, never
 *  a false "not running". */
async function defaultRunningNames() {
  if (process.platform !== 'win32') return new Set();
  try {
    const { stdout } = await execFileP('tasklist', ['/FO', 'CSV', '/NH'], { timeout: 8000, windowsHide: true, maxBuffer: 8 * 1024 * 1024 });
    const names = new Set();
    for (const line of String(stdout).split(/\r?\n/)) {
      const m = line.match(/^"([^"]+)"/);
      if (m) names.add(m[1].toLowerCase());
    }
    return names;
  } catch {
    return null;
  }
}

/** `reg query <key> /ve` → the default value (the NM manifest path), or null
 *  when the key is missing / reg is unavailable. */
async function defaultRegQuery(key) {
  if (process.platform !== 'win32') return null;
  try {
    const { stdout } = await execFileP('reg', ['query', key, '/ve'], { timeout: 5000, windowsHide: true });
    for (const line of String(stdout).split(/\r?\n/)) {
      const i = line.indexOf('REG_SZ');
      if (i >= 0) {
        const v = line.slice(i + 6).trim();
        return v || null;
      }
    }
    return null;
  } catch {
    return null;
  }
}

// ------------------------------------------------------------------ scanning

/** Find the UBB unpacked-extension entry among `extensions.settings`. */
function findUbbSetting(settings) {
  if (!settings || typeof settings !== 'object') return null;
  for (const [id, v] of Object.entries(settings)) {
    if (v && typeof v === 'object' && isUbbUnpacked(v.path)) {
      // Chromium: state 1 = enabled, 0 = disabled. Edge 的 unpacked 条目实测
      // （Edge 157）根本不带 state —— 用 disable_reasons 兜底：无该字段或为空
      // 数组视为 enabled（与 chrome://extensions 的「已启用」一致），有内容视为
      // disabled；两者都没有才报 unknown。
      let state;
      if (v.state === 1) state = 'enabled';
      else if (v.state === 0) state = 'disabled';
      else if (Array.isArray(v.disable_reasons)) state = v.disable_reasons.length ? 'disabled' : 'enabled';
      else state = 'enabled'; // unpacked（location 4）无 state 无 disable_reasons = 已启用（Edge 157 实测形态）
      return { id, state };
    }
  }
  return null;
}

/** Walk one family's profile root and classify each profile's UBB state.
 *  Profiles without a Preferences file (cache/system dirs) are skipped. */
function scanProfiles({ family, roots, listDirs, readJson }) {
  const root = joinSegs(roots, family.user_data);
  const dirs = listDirs(root);
  if (dirs == null) {
    return { scanned: false, note: `no profile root at ${root}`, profiles: [] };
  }
  const profiles = [];
  let truncated = false;
  for (const name of [...dirs].sort()) {
    // Windows 上 Chromium 把 extensions.settings 挪进 Secure Preferences（防篡改），
    // 两个文件都读并合并（Secure 优先）；两个都读不了才算非 profile 目录。
    const r = readJson(path.join(root, name, 'Preferences'));
    const rs = readJson(path.join(root, name, 'Secure Preferences'));
    if (r.error === 'unreadable' && rs.error === 'unreadable') continue; // not a profile dir
    const entry = { name };
    if (r.error && rs.error) {
      entry.parse = r.error === 'unreadable' ? rs.error : r.error; // too_large | unparsable
      entry.ubb = 'unknown';
    } else {
      entry.parse = 'ok';
      const merged = { ...(r.json?.extensions?.settings || {}), ...(rs.json?.extensions?.settings || {}) };
      const hit = findUbbSetting(merged);
      entry.ubb = hit ? hit.state : 'absent';
      if (hit) entry.ext_id = hit.id;
    }
    profiles.push(entry);
    if (profiles.length >= MAX_PROFILES_PER_FAMILY) {
      truncated = true;
      break;
    }
  }
  const out = { scanned: true, profiles };
  if (truncated) out.note = `stopped at ${MAX_PROFILES_PER_FAMILY} profiles`;
  return out;
}

/** NM manifest for one family. Candidates in priority order: the path the HKCU
 *  registry value points at (what Chromium actually reads on Windows), then the
 *  %APPDATA% per-family convention, then ~/.ubb/com.ubb.bridge.json (what
 *  `ubb install` writes). valid = name com.ubb.bridge AND its `path` exe exists. */
async function scanNmManifest({ family, roots, isFile, readJson, regQuery }) {
  let regValue = null;
  try {
    regValue = await regQuery(family.reg_key);
  } catch {
    regValue = null;
  }
  const candidates = [
    regValue ? { via: 'registry', p: regValue } : null,
    { via: 'appdata', p: path.join(joinSegs(roots, family.nm_dir), 'com.ubb.bridge.json') },
    { via: 'home', p: path.join(roots.home, '.ubb', 'com.ubb.bridge.json') },
  ].filter(Boolean);
  for (const c of candidates) {
    if (!isFile(c.p)) continue;
    const r = readJson(c.p);
    if (r.error) {
      return { present: true, valid: false, path: c.p, via: c.via, registry_value: regValue, registry_key: family.reg_key, note: `manifest ${r.error}` };
    }
    const exePath = typeof r.json?.path === 'string' ? r.json.path : '';
    const exeOk = !!exePath && isFile(exePath);
    return {
      present: true,
      valid: r.json?.name === 'com.ubb.bridge' && exeOk,
      path: c.p,
      manifest_path_field: exePath || null,
      exe_ok: exeOk,
      via: c.via,
      registry_value: regValue,
      registry_key: family.reg_key,
    };
  }
  return { present: false, valid: false, path: null, via: null, registry_value: regValue, registry_key: family.reg_key };
}

/**
 * Scan every family. Production callers pass nothing; tests pass `inject` with
 * any of { roots, isFile, listDirs, readJson, runningNames, regQuery, daemonState }.
 * Returns { scanned_at, platform, repo_root, browsers[], daemon }.
 */
export async function scanBrowsers(inject = null) {
  const inj = inject || {};
  const roots = inj.roots || defaultRoots();
  const isFile = inj.isFile || defaultIsFile;
  const listDirs = inj.listDirs || defaultListDirs;
  const readJson = inj.readJson || defaultReadJson;
  const runningNames = inj.runningNames || defaultRunningNames;
  const regQuery = inj.regQuery || defaultRegQuery;

  let running = null;
  try {
    running = await runningNames();
  } catch {
    running = null;
  }

  const browsers = [];
  for (const family of FAMILIES) {
    const exePath = family.exe.map((segs) => joinSegs(roots, segs)).find(isFile) || null;
    const prof = scanProfiles({ family, roots, listDirs, readJson });
    const ubbProfiles = prof.profiles.filter((p) => p.ubb === 'enabled' || p.ubb === 'disabled' || p.ubb === 'unknown');
    const nm = await scanNmManifest({ family, roots, isFile, readJson, regQuery });
    browsers.push({
      family: family.family,
      display: family.display,
      manage_url: family.manage_url,
      installed: !!exePath,
      exe_path: exePath,
      // null = process detection failed; true/false only when tasklist answered
      running: running ? running.has(family.process) : null,
      extension: {
        installed: ubbProfiles.length > 0,
        enabled: prof.profiles.some((p) => p.ubb === 'enabled'),
        profiles: prof.profiles,
        ...(prof.note ? { note: prof.note } : {}),
      },
      nm_manifest: nm,
    });
  }

  return {
    scanned_at: new Date().toISOString(),
    platform: process.platform,
    repo_root: UBB_ROOT,
    browsers,
    // /health-shaped daemon state, filled in by the caller (CLI fetch, or the
    // daemon itself for daemonMethod); scanBrowsers stays process-scoped
    daemon: inj.daemonState ?? null,
  };
}
