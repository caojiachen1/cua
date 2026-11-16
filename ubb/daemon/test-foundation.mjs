// Wave 0 offline registry self-check (contract §6.2) — no browser, no daemon:
//   node daemon/test-foundation.mjs
// Wave 1+ copy this file for their own module; lib/testharness.mjs does the stubbing.

import { collectRegistry } from '../lib/testharness.mjs';

const MIN_TOOLS = 83;

let pass = 0, fail = 0;
const ok = (name, cond, detail = '') => {
  if (cond) { pass++; console.log(`  ✓ ${name}`); }
  else { fail++; console.log(`  ✗ ${name}${detail ? ` — ${detail}` : ''}`); }
};

const { names, metas, call } = await collectRegistry();

ok(`registry has >= ${MIN_TOOLS} tools`, names.length >= MIN_TOOLS, `got ${names.length}`);

const noCategory = names.filter((n) => !metas[n]?.category);
ok('every tool declares a category', noCategory.length === 0, noCategory.join(', '));

// a tab_id outside any session must stop in targetTab(), not crash on chrome
let code = null;
let thrown = null;
try {
  await call('snapshot', { tab_id: 99999 });
} catch (e) {
  thrown = e;
  code = e?.code ?? null;
}
ok('snapshot without a session throws NO_SESSION', code === 'NO_SESSION',
  thrown ? `got ${code || 'plain throw'}: ${thrown?.message || thrown}` : 'it returned instead of throwing');

// the chrome stub is load-bearing: a chrome-backed tool must run through it
const listed = await call('tab.list', { scope: 'session' });
ok('tab.list runs against the chrome stub', Array.isArray(listed?.tabs), JSON.stringify(listed));

// browser.close: destructive, workspace-only by default, reply-before-close
const dry = await call('browser.close', { dry_run: true });
ok('browser.close dry_run lists windows and names the user-browser guard',
  dry?.dry_run === true && dry?.window_count === 1 && dry?.requires_allow_user_browser === true,
  JSON.stringify(dry));

let closeCode = null;
try { await call('browser.close', { confirm: true }); } catch (e) { closeCode = e?.code ?? 'plain'; }
ok('browser.close refuses the user browser without allow_user_browser', closeCode === 'USER_BROWSER_REFUSED', String(closeCode));

const origStorageGet = globalThis.chrome.storage.local.get;
const origWindowsRemove = globalThis.chrome.windows.remove;
try {
  globalThis.chrome.storage.local.get = async () => ({ ubb_workspace: true });
  const removed = [];
  globalThis.chrome.windows.remove = async (id) => { removed.push(id); };
  const out = await call('browser.close', { confirm: true, delay_ms: 0 });
  await new Promise((r) => setTimeout(r, 20));
  ok('browser.close on a workspace replies before closing and then requests every window',
    out?.closing === true && out?.workspace === true && out?.window_count === 1 && removed.join(',') === '1',
    JSON.stringify({ out, removed }));
} finally {
  globalThis.chrome.storage.local.get = origStorageGet;
  globalThis.chrome.windows.remove = origWindowsRemove;
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
