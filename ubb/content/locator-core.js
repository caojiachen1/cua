// Playwright-style locator matching — the pure, DOM-only core.
// Loaded as a content script BEFORE bridge.js in every frame (isolated world)
// and imported by daemon/test-locator.mjs in Node: one implementation, two
// consumers. No chrome.* access here; nothing async; never mutates the page.
//
// Grammar (subset of Playwright's, deliberately):
//   { css }                    selector base
//   { role }                   ARIA role (explicit wins, implicit table fallback)
//   { text }                   substring, case-insensitive, on collapsed text
//   { text_exact }             exact (still whitespace-collapsed)
//   { label }                  <label>-associated accessible name substring
//   { label_exact }
//   { placeholder }            input/textarea placeholder substring
//   { test_id }                matches the first TESTID_ATTRS attribute present
//   { alt }                    img alt substring
//   { value }                  input value substring
//   { has_text }               descendant text substring filter
//   { has_not_text }           descendant text must NOT contain
//   { checked }  { disabled }  { visible }   boolean state filters
//   { nth }                    0-based index into the remaining candidates
// Compose freely: { role: 'button', text: '提交', visible: true } — every given
// key must match. DOM order decides candidate order; nth picks from that order.

(function (root, factory) {
  const api = factory();
  const KEY = '__ubbLocator';
  if (root && typeof root === 'object') root[KEY] = api;
  if (typeof self !== 'undefined') self[KEY] = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  const TESTID_ATTRS = ['data-testid', 'data-test', 'data-test-id', 'data-qa', 'data-cy'];

  const IMPLICIT_ROLE = {
    a: (el) => (el.hasAttribute('href') ? 'link' : null),
    area: (el) => (el.hasAttribute('href') ? 'link' : null),
    button: () => 'button',
    datalist: () => 'listbox',
    dd: () => 'definition',
    details: () => 'group',
    dialog: () => 'dialog',
    dt: () => 'term',
    fieldset: () => 'group',
    figure: () => 'figure',
    form: () => 'form',
    h1: () => 'heading',
    h2: () => 'heading',
    h3: () => 'heading',
    h4: () => 'heading',
    h5: () => 'heading',
    h6: () => 'heading',
    hr: () => 'separator',
    img: (el) => (el.getAttribute('alt') !== '' ? 'img' : 'presentation'),
    input: (el) => {
      const t = (el.getAttribute('type') || 'text').toLowerCase();
      if (t === 'button' || t === 'submit' || t === 'reset') return 'button';
      if (t === 'checkbox') return 'checkbox';
      if (t === 'radio') return 'radio';
      if (t === 'image') return 'button';
      if (t === 'range') return 'slider';
      if (t === 'number') return 'spinbutton';
      if (t === 'search') return 'searchbox';
      if (t === 'email' || t === 'tel' || t === 'url' || t === 'text' || t === 'password' || t === 'date' || t === 'time' || t === '') return 'textbox';
      return 'textbox';
    },
    li: () => 'listitem',
    link: () => 'link',
    main: () => 'main',
    menu: () => 'list',
    menuitem: () => 'menuitem',
    meter: () => 'meter',
    nav: () => 'navigation',
    ol: () => 'list',
    option: () => 'option',
    output: () => 'status',
    progress: () => 'progressbar',
    section: () => 'region',
    select: (el) => (el.multiple || el.size > 1 ? 'listbox' : 'combobox'),
    summary: () => 'button',
    table: () => 'table',
    tbody: () => 'rowgroup',
    td: () => 'cell',
    textarea: () => 'textbox',
    tfoot: () => 'rowgroup',
    th: () => 'columnheader',
    thead: () => 'rowgroup',
    tr: () => 'row',
    ul: () => 'list',
  };

  function collapse(s) {
    return String(s || '').replace(/\s+/g, ' ').trim();
  }

  function roleOf(el) {
    const explicit = el.getAttribute && el.getAttribute('role');
    if (explicit) return collapse(explicit).split(' ')[0];
    const fn = IMPLICIT_ROLE[el.tagName && el.tagName.toLowerCase()];
    return fn ? fn(el) : null;
  }

  function ownText(el) {
    let out = '';
    for (const n of el.childNodes) {
      if (n.nodeType === 3) out += n.nodeValue;
    }
    return collapse(out);
  }

  function textOf(el) {
    return collapse(el.textContent);
  }

  function accessibleName(el) {
    const lbl = el.getAttribute && el.getAttribute('aria-labelledby');
    if (lbl) {
      const parts = lbl
        .split(/\s+/)
        .map((id) => el.ownerDocument && el.ownerDocument.getElementById(id))
        .filter(Boolean)
        .map((n) => textOf(n))
        .filter(Boolean);
      if (parts.length) return parts.join(' ');
    }
    return (
      el.getAttribute('aria-label') ||
      el.getAttribute('alt') ||
      el.getAttribute('title') ||
      (el.tagName === 'INPUT' && (el.type === 'button' || el.type === 'submit' || el.type === 'reset') ? el.value : '') ||
      (el.labels && el.labels.length ? collapse(el.labels[0].textContent) : '') ||
      ownText(el) ||
      textOf(el)
    );
  }

  function visible(el) {
    if (!(el instanceof el.ownerDocument.defaultView.Element)) return false;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return false;
    const view = el.ownerDocument.defaultView;
    const st = view.getComputedStyle ? view.getComputedStyle(el) : null;
    if (st && (st.visibility === 'hidden' || st.display === 'none')) return false;
    return true;
  }

  /** Base candidate set: the smallest enumeration that can still satisfy the spec. */
  function baseSet(doc, spec) {
    if (spec.css) {
      try {
        return Array.from(doc.querySelectorAll(spec.css));
      } catch {
        return []; // invalid selector → zero candidates, never a throw into the page
      }
    }
    const interactive =
      'a,button,input,select,textarea,option,label,summary,dialog,form,h1,h2,h3,h4,h5,h6,img,li,td,th,tr,nav,main,table,ul,ol,menu,[role],[contenteditable],[onclick],[tabindex]';
    return Array.from(doc.querySelectorAll(interactive));
  }

  function matches(el, spec) {
    if (spec.role != null && roleOf(el) !== String(spec.role).toLowerCase()) return false;
    if (spec.test_id != null) {
      const hit = TESTID_ATTRS.some((a) => (el.getAttribute(a) || '') === String(spec.test_id));
      if (!hit) return false;
    }
    // _exact variants are standalone keys: {text_exact:"保存"} without {text}
    // must still constrain, not silently match everything
    if (spec.text != null || spec.text_exact != null) {
      const hay = textOf(el);
      const exact = spec.text_exact != null;
      const needle = collapse(exact ? spec.text_exact : spec.text);
      if (!needle) return false;
      if (exact ? hay !== needle : !hay.toLowerCase().includes(needle.toLowerCase())) return false;
    }
    if (spec.label != null || spec.label_exact != null) {
      const hay = collapse(el.getAttribute('aria-label') || '') || (el.labels && el.labels.length ? collapse(el.labels[0].textContent) : '');
      const exact = spec.label_exact != null;
      const needle = collapse(exact ? spec.label_exact : spec.label);
      if (!needle) return false;
      if (exact ? hay !== needle : !hay.toLowerCase().includes(needle.toLowerCase())) return false;
    }
    if (spec.placeholder != null || spec.placeholder_exact != null) {
      const hay = collapse(el.placeholder != null ? el.placeholder : el.getAttribute && el.getAttribute('placeholder'));
      const exact = spec.placeholder_exact != null;
      const needle = collapse(exact ? spec.placeholder_exact : spec.placeholder);
      if (!hay) return false;
      if (exact ? hay !== needle : !hay.toLowerCase().includes(needle.toLowerCase())) return false;
    }
    if (spec.alt != null) {
      const hay = collapse(el.getAttribute && el.getAttribute('alt'));
      if (!hay || !hay.toLowerCase().includes(collapse(spec.alt).toLowerCase())) return false;
    }
    if (spec.value != null) {
      const hay = collapse(el.value != null ? el.value : '');
      if (!hay || !hay.toLowerCase().includes(collapse(spec.value).toLowerCase())) return false;
    }
    if (spec.has_text != null) {
      const hay = textOf(el);
      if (!hay.toLowerCase().includes(collapse(spec.has_text).toLowerCase())) return false;
    }
    if (spec.has_not_text != null) {
      if (textOf(el).toLowerCase().includes(collapse(spec.has_not_text).toLowerCase())) return false;
    }
    if (spec.checked != null) {
      if (typeof el.checked !== 'boolean' || el.checked !== !!spec.checked) return false;
    }
    if (spec.disabled != null) {
      if (!!el.disabled !== !!spec.disabled) return false;
    }
    if (spec.visible != null && visible(el) !== !!spec.visible) return false;
    return true;
  }

  /**
   * Query one document. Returns elements in DOM order; nth slices the final list.
   * @param {object} spec  see the grammar above
   * @param {Document|Element} root  search root (Element scopes css to descendants)
   */
  function query(spec, root) {
    if (!spec || typeof spec !== 'object') return [];
    const rootEl = root && root.nodeType === 1 ? root : null;
    const doc = rootEl ? rootEl.ownerDocument : (root && root.querySelectorAll ? root : document);
    let list = baseSet(doc, rootEl ? { css: spec.css } : spec);
    if (rootEl && spec.css) {
      list = list.filter((el) => rootEl.contains(el));
    }
    const out = [];
    for (const el of list) {
      if (matches(el, spec)) out.push(el);
    }
    if (spec.nth != null) {
      const i = Number(spec.nth);
      return i >= 0 && i < out.length ? [out[i]] : [];
    }
    return out;
  }

  /** Serializable description of one match (boxes in THIS frame's viewport). */
  function describe(el) {
    const r = el.getBoundingClientRect();
    const view = el.ownerDocument.defaultView;
    const st = view.getComputedStyle ? view.getComputedStyle(el) : null;
    const vis = visible(el) && (!st || (st.visibility !== 'hidden' && st.display !== 'none'));
    return {
      tag: el.tagName.toLowerCase(),
      role: roleOf(el),
      name: collapse(accessibleName(el)).slice(0, 120),
      text: textOf(el).slice(0, 200),
      placeholder: el.placeholder || undefined,
      value: typeof el.value === 'string' ? el.value.slice(0, 120) : undefined,
      attributes: (() => {
        const out = {};
        for (const a of ['id', 'name', 'class', 'href', 'src', 'type', 'alt', 'title', 'aria-label', 'data-testid']) {
          const v = el.getAttribute && el.getAttribute(a);
          if (v != null) out[a] = String(v).slice(0, 120);
        }
        return out;
      })(),
      box: { x: r.x, y: r.y, width: r.width, height: r.height },
      cx: r.x + r.width / 2,
      cy: r.y + r.height / 2,
      visible: vis,
      disabled: !!el.disabled,
      checked: typeof el.checked === 'boolean' ? el.checked : undefined,
    };
  }

  return { query, describe, matches, visible, roleOf, accessibleName, TESTID_ATTRS };
});
