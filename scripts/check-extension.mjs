#!/usr/bin/env node
// check-extension.mjs — fail loudly when an extension script does not parse.
//
// The shield stylesheet lives inside a JS template literal, so a stray
// backtick anywhere in the CSS (a perfectly normal thing to type in a comment
// while describing `code`) closes the literal early and breaks the WHOLE
// content script. The symptom is silent: the veil simply never appears, and
// nothing in the page or the console says why.
//
// Run from package.ps1 before packing, or by hand:
//     node scripts/check-extension.mjs

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { execFileSync } from 'node:child_process';

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const extDir = join(root, 'browser', 'extension');

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (name.endsWith('.js') || name.endsWith('.mjs')) out.push(p);
  }
  return out;
}

let failed = 0;
for (const file of walk(extDir)) {
  try {
    execFileSync(process.execPath, ['--check', file], { stdio: 'pipe' });
  } catch (e) {
    failed++;
    const msg = (e.stderr || '').toString().split('\n').slice(0, 4).join('\n');
    console.error(`FAIL ${relative(root, file)}\n${msg}\n`);
  }
}

if (failed) {
  console.error(`${failed} extension script(s) do not parse — fix before shipping.`);
  process.exit(1);
}
console.log('extension scripts: all parse');
