// locator.* offline suite: the locator-core matcher is pure DOM logic, so it
// runs in Node against synthetic elements — no jsdom, no browser. The live
// cross-frame resolution is exercised by e2e/run.mjs against a real page.
// Run: node daemon/test-locator.mjs

import assert from 'node:assert/strict';
import vm from 'node:vm';
import fs from 'node:fs';

// locator-core is a classic-script IIFE for content-script injection with a
// CJS escape hatch; the package is type:module, so run the file bytes through
// a CJS wrapper ourselves — the test exercises exactly what the browser loads
const src = fs.readFileSync(new URL('../content/locator-core.js', import.meta.url), 'utf8');
const sandbox = { module: { exports: {} }, globalThis: {} };
vm.runInNewContext(src, sandbox);
const L = sandbox.module.exports;
assert.equal(typeof L.query, 'function', 'locator-core did not export its API');

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

// ---------------------------------------------------------------- fake DOM
// locator-core touches: tagName, getAttribute, attributes, childNodes/nodeValue,
// textContent, labels, disabled/checked/placeholder/value, getBoundingClientRect,
// ownerDocument.defaultView.getComputedStyle. Just enough DOM to be honest.

function makeView() {
  const view = { getComputedStyle: () => ({ visibility: 'visible', display: 'block' }) };
  view.Element = function Element() {};
  return view;
}

let idSeq = 0;
function makeEl(spec = {}) {
  const attrs = new Map(Object.entries(spec.attrs || {}));
  const el = {
    tagName: (spec.tag || 'div').toUpperCase(),
    nodeType: 1,
    children: spec.children || [],
    childNodes: spec.childNodes || (spec.children || []).flatMap((c) => (c.nodeType === 3 ? [c] : [c, { nodeType: 3, nodeValue: '' }])),
    parentElement: null,
    labels: spec.labels || [],
    ownerDocument: { defaultView: null, getElementById: (id) => (attrs.get('aria-labelledby') || '').split(/\s+/).includes(id) ? doc.byId[id] : null },
    getAttribute: (n) => (attrs.has(n) ? attrs.get(n) : null),
    hasAttribute: (n) => attrs.has(n),
    getBoundingClientRect: () => spec.rect || { x: 10, y: 10, width: 100, height: 24 },
    disabled: spec.disabled,
    checked: spec.checked,
    value: spec.value,
    placeholder: spec.placeholder,
    multiple: spec.multiple,
    size: spec.size,
    type: spec.type,
    contains: (other) => {
      for (const c of el.children) if (c === other || c.contains(other)) return true;
      return false;
    },
    querySelectorAll: () => [], // baseSet uses doc-level; el-level only scoped css
    closest: () => null,
  };
  if (spec.type) attrs.set('type', spec.type);
  if (spec.placeholder) attrs.set('placeholder', spec.placeholder);
  const view = spec.view || makeView();
  el.ownerDocument.defaultView = view;
  Object.setPrototypeOf(el, view.Element.prototype); // visible() instanceof check
  Object.defineProperty(el, 'textContent', {
    get() {
      return (spec.text || '') + el.children.map((c) => (c.nodeType === 3 ? c.nodeValue : c.textContent)).join(' ');
    },
  });
  if (!el.childNodes.length && spec.text) el.childNodes = [{ nodeType: 3, nodeValue: spec.text }];
  return el;
}

function makeDoc(byId = {}) {
  const doc = {
    nodeType: 9,
    byId,
    getElementById: (id) => byId[id] || null,
    createElement: () => makeEl(),
    defaultView: null,
  };
  doc.defaultView = makeView();
  for (const k of Object.keys(byId)) {
    byId[k].ownerDocument = doc;
    byId[k].ownerDocument.defaultView = doc.defaultView;
    Object.setPrototypeOf(byId[k], doc.defaultView.Element.prototype);
  }
  return doc;
}

function baseEl(doc) {
  return doc; // query(spec, root) with root.nodeType === 9 uses root.querySelectorAll — stubbed below
}

// query() needs doc.querySelectorAll for the base set; give the fake doc a
// registry-driven one
function docWith(elements) {
  const doc = makeDoc();
  doc.querySelectorAll = (sel) => {
    // a real DOM throws on malformed selectors — locator-core relies on that
    if (typeof sel !== 'string') throw new Error('not a selector');
    const opens = (sel.match(/\[/g) || []).length;
    const closes = (sel.match(/\]/g) || []).length;
    if (opens !== closes || /\[\s*\]/.test(sel)) throw new Error(`invalid selector: ${sel}`);
    return elements;
  };
  doc.body = makeEl({ tag: 'body' });
  doc.body.ownerDocument = doc;
  doc.documentElement = doc.body;
  for (const e of elements) e.ownerDocument = doc;
  return doc;
}

console.log('\n[1] role computation');
ok('explicit role wins over the implicit table', L.roleOf(makeEl({ tag: 'button', attrs: { role: 'tab' } })) === 'tab');
ok('button → button', L.roleOf(makeEl({ tag: 'button' })) === 'button');
ok('a[href] → link, a without href → null', L.roleOf(makeEl({ tag: 'a', attrs: { href: '/x' } })) === 'link' && L.roleOf(makeEl({ tag: 'a' })) === null);
ok('input type=checkbox → checkbox', L.roleOf(makeEl({ tag: 'input', type: 'checkbox' })) === 'checkbox');
ok('input type=submit → button', L.roleOf(makeEl({ tag: 'input', type: 'submit' })) === 'button');
ok('select single → combobox, multiple → listbox', L.roleOf(makeEl({ tag: 'select' })) === 'combobox' && L.roleOf(makeEl({ tag: 'select', multiple: true })) === 'listbox');
ok('textarea → textbox, h2 → heading', L.roleOf(makeEl({ tag: 'textarea' })) === 'textbox' && L.roleOf(makeEl({ tag: 'h2' })) === 'heading');

console.log('\n[2] matching grammar');
const btn = makeEl({ tag: 'button', text: '提交表单', attrs: { 'aria-label': '提交' } });
ok('role + text substring', L.matches(btn, { role: 'button', text: '提交' }) === true);
ok('text is case-insensitive', L.matches(makeEl({ tag: 'button', text: 'SAVE ALL' }), { text: 'save all' }) === true);
ok('text_exact refuses substrings', L.matches(btn, { text_exact: '提交' }) === false && L.matches(btn, { text_exact: '提交表单' }) === true);
ok('role mismatch refuses', L.matches(btn, { role: 'link', text: '提交' }) === false);
ok('aria-label feeds label matching', L.matches(btn, { label: '提交' }) === true);
ok('label[for]-less .labels association works', L.matches(makeEl({ tag: 'input', type: 'text', labels: [makeEl({ tag: 'label', text: '用户名' })] }), { label: '用户名' }) === true);
ok('placeholder matches', L.matches(makeEl({ tag: 'input', type: 'text', placeholder: '请输入邮箱' }), { placeholder: '邮箱' }) === true);
ok('test_id hits any supported attr', L.matches(makeEl({ tag: 'button', attrs: { 'data-testid': 'checkout' } }), { test_id: 'checkout' }) === true);
ok('has_text looks at descendants', (() => {
  const inner = makeEl({ tag: 'span', text: '限时优惠' });
  const outer = makeEl({ tag: 'div', children: [inner] });
  return L.matches(outer, { has_text: '优惠' }) === true && L.matches(inner, { has_not_text: '优惠' }) === false;
})());
ok('checked/disabled filters are strict booleans', (() => {
  const on = makeEl({ tag: 'input', type: 'checkbox', checked: true });
  return L.matches(on, { checked: true }) === true && L.matches(on, { checked: false }) === false
    && L.matches(makeEl({ tag: 'button', disabled: true }), { disabled: true }) === true
    && L.matches(makeEl({ tag: 'button', disabled: false }), { disabled: true }) === false;
})());

console.log('\n[3] query + nth over a document');
const doc = docWith([
  makeEl({ tag: 'button', text: '删除', attrs: { class: 'danger' } }),
  makeEl({ tag: 'button', text: '保存' }),
  makeEl({ tag: 'button', text: '取消' }),
  makeEl({ tag: 'a', attrs: { href: '/x' }, text: '帮助' }),
]);
ok('role query returns all buttons in DOM order', L.query({ role: 'button' }, doc).length === 3);
ok('text narrows to one', L.query({ role: 'button', text: '保存' }, doc).length === 1);
ok('nth picks by index', L.query({ role: 'button', nth: 2 }, doc)[0].textContent === '取消');
ok('nth out of range → empty, never a wraparound', L.query({ role: 'button', nth: 9 }, doc).length === 0);
ok('css base set works', L.query({ css: 'button', text: '删除' }, doc).length === 1);
ok('an invalid css selector yields zero candidates (never a throw into the page)', L.query({ css: 'button[[' }, doc).length === 0);
ok('an empty spec matches every base candidate (the grammar guard lives in normalizeLocator, which refuses it upstream)', L.query({}, doc).length === 4);

console.log('\n[4] visibility + describe shape');
const hidden = makeEl({ tag: 'button', text: '隐身', rect: { x: 0, y: 0, width: 0, height: 0 } });
const shown = makeEl({ tag: 'button', text: '在的' });
ok('zero-size box is invisible', L.visible(hidden) === false && L.visible(shown) === true);
ok('visible:false filter drops it from query', L.query({ text: '隐身', visible: true }, docWith([hidden])).length === 0);
const d = L.describe(shown);
ok('describe exposes tag/role/name/box/visible', d.tag === 'button' && d.role === 'button' && d.box.width === 100 && d.visible === true);
ok('describe centers cx/cy for the coordinate path', d.cx === 60 && d.cy === 22);

console.log('\n[5] locator grammar guard (lib/locator-resolve.js)');
// locator-resolve imports the extension stack; load it with the harness stub
const { installChromeStub } = await import('../lib/testharness.mjs');
installChromeStub();
const { normalizeLocator } = await import('../lib/locator-resolve.js');
// err() throws plain {code, message} objects, not Errors — verify the code
assert.throws(() => normalizeLocator(null), (e) => e.code === 'BAD_PARAMS' && /must be an object/.test(e.message));
assert.throws(() => normalizeLocator({}), (e) => e.code === 'BAD_PARAMS' && /at least one/.test(e.message));
assert.throws(() => normalizeLocator({ role: 'button', wat: 1 }), (e) => e.code === 'BAD_PARAMS' && /unknown locator key/.test(e.message));
assert.throws(() => normalizeLocator({ role: 'button', nth: -1 }), (e) => e.code === 'BAD_PARAMS' && /non-negative integer/.test(e.message));
ok('normalizeLocator refuses empty/unknown/negative specs honestly (BAD_PARAMS + reason)', true);
ok('text_exact alone is a valid spec', normalizeLocator({ text_exact: '保存' }).text_exact === '保存');
ok('normalizeLocator passes a valid spec through untouched', normalizeLocator({ role: 'button', text: 'x', nth: '2' }).nth === 2);

console.log(`\n=== test-locator: ${passed} passed, ${failed} failed ===`);
if (failed) {
  console.error('FAILED:', failures.join(' · '));
  process.exit(1);
}
