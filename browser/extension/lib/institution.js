// Institution access: CARSI (Shibboleth DS) and WebVPN — both driven by
// DATA profiles, not by a hardcoded school. A profile describes the school's
// SSO entry, WebVPN home, library navigation labels and proxy URL shape; the
// engine below only knows the flow. Add profiles with institution.profile.set
// (stored in chrome.storage.local) or in content/institution-profiles.json.
//
// No tool here reads, types or stores a password. Login pages are handed to
// the user (institution.sso_handoff) or use the browser's own session; the
// CARSI school chooser and the library search are normal page input.

import { err } from './config.js';
import { cdp } from './cdp.js';
import { sessions } from './tabs.js';
import { targetTab, evalJs, armTab, waitForLoad, runHelp } from './actions.js';
import { sleep, withPass } from './shield-ctl.js';
import { audit } from './audit.js';
import { redactUrl, truncate } from './redact.js';

const PROFILE_KEY = 'cua_institution_profiles';
const SEED_FILE = 'content/institution-profiles.json';
let seedCache = null;

async function loadSeed() {
  if (seedCache) return seedCache;
  try {
    const r = await fetch(chrome.runtime.getURL(SEED_FILE));
    seedCache = r.ok ? await r.json() : { template: null, profiles: [] };
  } catch {
    seedCache = { template: null, profiles: [] };
  }
  return seedCache;
}

async function loadProfiles() {
  const seed = await loadSeed();
  const stored = (await chrome.storage.local.get(PROFILE_KEY).catch(() => ({})))[PROFILE_KEY] || [];
  const map = new Map();
  for (const p of seed.profiles || []) if (p?.name) map.set(p.name, p);
  for (const p of stored) if (p?.name) map.set(p.name, p);
  return { map, template: seed.template || null };
}

async function getProfile(name) {
  if (!name) return null;
  const { map } = await loadProfiles();
  return map.get(String(name)) || null;
}

function webvpnConfig(profile, inline) {
  return { ...(profile?.webvpn || {}), ...(inline || {}) };
}

function missingWebvpn(cfg) {
  const need = ['cas_login', 'home_pattern', 'library_link_text', 'library_query_button_text', 'library_entry_prefix'];
  return need.filter((k) => !cfg[k]);
}


/** Real trusted click at a page coordinate (the WebVPN portal cards are SPA
 *  handlers that ignore a synthetic .click()). The tab must be visible. */
async function realClick(tabId, x, y) {
  const st = await evalJs(tabId, 'document.visibilityState').catch(() => null);
  if (st && st !== 'visible') {
    const t = await chrome.tabs.get(tabId).catch(() => null);
    if (t?.windowId != null) {
      await chrome.windows.update(t.windowId, { state: 'normal', focused: true }).catch(() => {});
      await sleep(300);
    }
  }
  const st2 = await evalJs(tabId, 'document.visibilityState').catch(() => null);
  if (st2 && st2 !== 'visible') throw err('INPUT_NOT_DELIVERED', `document.visibilityState=${JSON.stringify(st2)} — bring the window forward (window.raise) and retry; nothing was clicked`);
  await withPass(tabId, async () => {
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
    await sleep(40 + Math.random() * 60);
    await cdp.send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 1, clickCount: 1 });
  }, { at: [x, y] });
}

/** Resolve an element with a JS expression returning that element, then click
 *  its box with real input. Returns false when the expression finds no visible box. */
async function realClickByJs(tabId, locateExpr) {
  const r = await evalJs(tabId, `(() => {
    const a = (${locateExpr});
    if (!a) return null;
    // a row below the fold has a box outside the viewport: a click at that y
    // reaches nothing. Center it first, then measure.
    try { a.scrollIntoView({ block: 'center', inline: 'center' }); } catch (e) {}
    const b = a.getBoundingClientRect();
    if (b.width < 2 || b.height < 2) return null;
    return { x: b.x + b.width / 2, y: b.y + b.height / 2, w: b.width, h: b.height };
  })()`).catch(() => null);
  if (!r) return false;
  await realClick(tabId, r.x, r.y);
  return true;
}

function isProxiedUrl(u) {
  try {
    return /^\/(?:https?|https-\d+)\/[^/]+\//.test(new URL(u).pathname);
  } catch { return false; }
}

async function waitUrlRegex(tabId, re, ms = 15000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const t = await chrome.tabs.get(tabId).catch(() => null);
    if (t && re.test(t.url || '')) return t;
    await sleep(300);
  }
  return null;
}

async function findProxiedTab(preferPattern, excludeIds = [], ms = 12000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const tabs = await chrome.tabs.query({}).catch(() => []);
    const proxied = tabs.filter((t) => !excludeIds.includes(t.id) && isProxiedUrl(t.url || ''));
    const hit = preferPattern ? proxied.find((t) => preferPattern.test(t.url || '')) : proxied[0];
    if (hit) return hit;
    await sleep(300);
  }
  return null;
}

function registerInstitution(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('institution.profile.list', {
    category: 'session',
    description: 'List institution profiles (seed content/institution-profiles.json + institution.profile.set overrides) and the field template. A profile is data: CARSI school, WebVPN SSO entry, library navigation labels, proxy shape. No passwords.',
  }, async () => {
    const { map, template } = await loadProfiles();
    return {
      profiles: [...map.values()].map((p) => ({
        name: p.name,
        carsi_school: p.carsi_school || null,
        webvpn: p.webvpn ? {
          cas_host: (() => { try { return new URL(String(p.webvpn.cas_login || '').replace('{service}', 'x')).hostname; } catch { return null; } })(),
          library_link_text: p.webvpn.library_link_text || null,
          library_url_pattern: p.webvpn.library_url_pattern || null,
          home_pattern: p.webvpn.home_pattern || null,
        } : null,
      })),
      template,
      note: 'add a profile with institution.profile.set {name, profile:{carsi_school, webvpn:{cas_login, service, home_pattern, library_link_text, library_link_class, library_url_pattern, library_search_input, library_query_button_text, library_entry_prefix, row_name_cell}}}',
    };
  });

  R('institution.profile.set', {
    category: 'session',
    description: 'Create or replace an institution profile in chrome.storage.local. The profile is plain data (URLs/labels); it never contains a password. Validate required WebVPN fields before saving.',
  }, async (p = {}) => {
    const name = String(p.name || p.profile?.name || '').trim();
    if (!/^[a-zA-Z0-9_-]{1,40}$/.test(name)) throw err('BAD_PARAMS', 'name must be 1-40 chars of [a-zA-Z0-9_-]');
    if (!p.profile || typeof p.profile !== 'object' || Array.isArray(p.profile)) throw err('BAD_PARAMS', 'profile object required');
    const profile = { ...p.profile, name };
    if (profile.webvpn) {
      const miss = missingWebvpn(profile.webvpn);
      if (miss.length) throw err('BAD_PARAMS', `webvpn profile is missing: ${miss.join(', ')}`);
    }
    if (!profile.webvpn && !profile.carsi_school) throw err('BAD_PARAMS', 'a profile needs at least webvpn or carsi_school');
    const cur = (await chrome.storage.local.get(PROFILE_KEY).catch(() => ({})))[PROFILE_KEY] || [];
    const next = cur.filter((x) => x?.name !== name);
    next.push(profile);
    await chrome.storage.local.set({ [PROFILE_KEY]: next });
    audit.log({ action: 'institution.profile.set', name });
    return { saved: true, name, has_webvpn: !!profile.webvpn, carsi_school: profile.carsi_school || null };
  });

  R('institution.profile.remove', {
    category: 'session',
    description: 'Remove a stored institution profile override. A seed profile (content/institution-profiles.json) cannot be removed this way.',
  }, async (p = {}) => {
    const name = String(p.name || '');
    const cur = (await chrome.storage.local.get(PROFILE_KEY).catch(() => ({})))[PROFILE_KEY] || [];
    const next = cur.filter((x) => x?.name !== name);
    await chrome.storage.local.set({ [PROFILE_KEY]: next });
    return { removed: next.length !== cur.length, name, note: next.length === cur.length ? 'not a stored override (a seed profile can only be edited in the JSON file)' : undefined };
  });

  R('institution.carsi', {
    category: 'session',
    timeoutMs: 120000,
    description: 'Log in to a CARSI school and (optionally) open a named resource. Pass school, or a profile with carsi_school. Opens ds.carsi.edu.cn, selects the school, completes the login if the IdP session is valid, searches the resource list and follows the gotoResource entry. Returns {tab_id,url,stage}; a waiting login form returns stage:"needs_sso" for institution.sso_handoff. No password is read or typed.',
  }, async (p = {}) => {
    const profile = p.profile ? await getProfile(p.profile) : null;
    if (p.profile && !profile) throw err('NOT_FOUND', `unknown institution profile "${p.profile}" — see institution.profile.list`);
    const school = String(p.school || profile?.carsi_school || '');
    if (!school) throw err('BAD_PARAMS', 'pass school:"<name>" or use a profile with carsi_school');
    if (!sessions.active()) await sessions.start();
    const t = await sessions.createTab('https://ds.carsi.edu.cn/login/index.html', { active: p.active !== false });
    await armTab(t.tab_id);
    await waitForLoad(t.tab_id, 20000).catch(() => false);
    await sleep(600);
    const step = await evalJs(t.tab_id, `(() => {
      const input = [...document.querySelectorAll('input')].find((i) => /高校|机构|institution/i.test(i.placeholder || ''));
      if (!input) return { stage: 'no_form' };
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      setter.call(input, ${JSON.stringify(school)});
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
      return { stage: 'typed' };
    })()`).catch(() => null);
    await sleep(700);
    const clicked = await evalJs(t.tab_id, `(() => {
      const sug = [...document.querySelectorAll('a')].find((a) => (a.textContent || '').includes(${JSON.stringify(school)}));
      if (sug) sug.click();
      const btn = [...document.querySelectorAll('button,a')].find((b) => /^\\s*登录\\s*$/.test(b.textContent || ''));
      if (btn) btn.click();
      return { sug: !!sug, btn: !!btn };
    })()`).catch(() => null);
    const resTab = await waitUrlRegex(t.tab_id, /resource\.php/i, 12000);
    const cur = await chrome.tabs.get(t.tab_id).catch(() => null);
    if (!resTab && cur && !/resource\.php/i.test(cur.url || '')) {
      return {
        stage: 'needs_sso', tab_id: t.tab_id, url: redactUrl(cur.url || ''), typed: step, clicked,
        note: 'complete the school login (Edge autofill / institution.sso_handoff), then call institution.carsi again with the same session',
      };
    }
    if (!p.resource) return { stage: 'resources', tab_id: t.tab_id, url: redactUrl(cur?.url || '') };
    // Search with REAL keystrokes: the CARSI page is a Vue app and a native
    // value setter does not update its model.
    await evalJs(t.tab_id, `(() => {
      const i = [...document.querySelectorAll('input')].find((x) => /资源名称|搜索|resource/i.test(x.placeholder || ''));
      if (i) i.focus();
      return !!i;
    })()`).catch(() => false);
    await cdp.send(t.tab_id, 'Input.insertText', { text: String(p.resource) });
    await sleep(250);
    await evalJs(t.tab_id, `(() => {
      const b = [...document.querySelectorAll('button,input[type=button],input[type=submit]')]
        .find((x) => ((x.textContent || '').trim() || x.value || '').trim() === '搜索');
      if (b) b.click();
      return !!b;
    })()`).catch(() => false);
    await sleep(2200);
    const href = await evalJs(t.tab_id, `(() => {
      const a = [...document.querySelectorAll('a[href*="resourceDetail.php"]')].find((x) => (x.textContent || '').includes(${JSON.stringify(String(p.resource))}));
      return a ? a.href : null;
    })()`).catch(() => null);
    if (!href) return { stage: 'resource_not_found', tab_id: t.tab_id, url: redactUrl(cur?.url || ''), resource: p.resource };
    const goto = String(href).replace(/resourceDetail\.php\?id=/, 'gotoResource.php?id=');
    await chrome.tabs.update(t.tab_id, { url: goto });
    await sleep(2500);
    const final = await chrome.tabs.get(t.tab_id).catch(() => null);
    return { stage: 'opened', tab_id: t.tab_id, url: redactUrl(final?.url || ''), title: truncate(final?.title || '', 160), resource_detail: redactUrl(href) };
  });

  R('institution.sso_handoff', {
    category: 'session',
    description: 'Ask the user to complete a login with Edge\'s own password manager (the password never touches this tool), while the tab is brought to the front. Waits for success_url_contains / success_url_matches or a manual 完成. Use it for any SSO / CARSI IdP / WebVPN page, then continue with institution.*.',
  }, async (p = {}, ctx) => {
    const tabId = targetTab(p);
    const conds = [];
    if (p.success_url_contains) conds.push({ kind: 'url_contains', value: String(p.success_url_contains) });
    if (p.success_url_matches) conds.push({ kind: 'url_matches', value: String(p.success_url_matches) });
    const r = await runHelp(
      tabId,
      String(p.prompt || '请用 Edge 已保存的账号密码完成登录（密码由浏览器自动填入，不经过本工具），完成后点“完成”'),
      conds,
      Math.min(Math.max(Number(p.timeout_ms) || 120000, 5000), 300000),
      ctx,
      true,
    );
    audit.log({ action: 'institution.sso_handoff', tabId, outcome: r.outcome });
    return r;
  });

  R('institution.webvpn', {
    category: 'session',
    timeoutMs: 180000,
    description: 'Profile-driven WebVPN flow: open the school\'s CAS entry, follow the token login into the WebVPN home, open the library database navigation, search a database name, click its entry and return a session tab on the proxied publisher. Requires a profile (institution.profile.set) or inline fields: cas_login (with {service} optional), service, home_pattern, library_link_text, library_link_class, library_url_pattern, library_search_input, library_query_button_text, library_entry_prefix, row_name_cell. A waiting login form returns stage:"needs_sso". No password is read or typed.',
  }, async (p = {}) => {
    const profile = p.profile ? await getProfile(p.profile) : null;
    if (p.profile && !profile) throw err('NOT_FOUND', `unknown institution profile "${p.profile}" — see institution.profile.list`);
    const cfg = webvpnConfig(profile, p.webvpn || {});
    const miss = missingWebvpn(cfg);
    if (miss.length) {
      throw err('PROFILE_REQUIRED', `no usable WebVPN profile: missing ${miss.join(', ')}. Add one with institution.profile.set {name, profile:{webvpn:{...}}} (see institution.profile.list's template) or pass the fields inline.`, {
        missing: miss, template: (await loadProfiles()).template,
      });
    }
    const database = String(p.database || '');
    if (!database) throw err('BAD_PARAMS', 'database name required (e.g. a publisher/database label as shown in the library list)');
    const login = String(cfg.cas_login).includes('{service}')
      ? String(cfg.cas_login).replace('{service}', encodeURIComponent(String(cfg.service || '')))
      : String(cfg.cas_login);
    if (!sessions.active()) await sessions.start();
    let tabId = p.resume_tab_id;
    if (tabId == null) {
      const t = await sessions.createTab(login, { active: p.active !== false });
      tabId = t.tab_id;
      await armTab(tabId);
      await waitForLoad(tabId, 20000).catch(() => false);
    }
    let cur = await chrome.tabs.get(tabId).catch(() => null);
    if (cur && /\/cas\/login/i.test(cur.url || '')) {
      const hasForm = await evalJs(tabId, `!!document.querySelector('#username, input[type=password]')`).catch(() => false);
      if (hasForm) return { stage: 'needs_sso', tab_id: tabId, url: redactUrl(cur.url || ''), note: 'complete the login (Edge autofill / institution.sso_handoff) and call institution.webvpn again with resume_tab_id' };
    }
    const home = await waitUrlRegex(tabId, new RegExp(String(cfg.home_pattern)), 15000);
    if (!home) {
      cur = await chrome.tabs.get(tabId).catch(() => null);
      return { stage: 'webvpn_unexpected', tab_id: tabId, url: redactUrl(cur?.url || ''), home_pattern: cfg.home_pattern };
    }
    // the WebVPN home is an SPA: wait for the link to render, then click it
    // with REAL pointer input — its card handler ignores synthetic .click().
    await sleep(1200);
    const locateLibrary = `(() => {
      const text = ${JSON.stringify(String(cfg.library_link_text))};
      const cls = ${JSON.stringify(String(cfg.library_link_class || ''))};
      const links = [...document.querySelectorAll('a')].filter((x) => (x.textContent || '').includes(text));
      return (cls ? links.find((x) => String(x.className || '').includes(cls)) : null) || links[links.length - 1] || links[0] || null;
    })()`;
    let clicked = false;
    for (let i = 0; i < 20 && !clicked; i++) {
      clicked = await realClickByJs(tabId, locateLibrary).catch(() => false);
      if (!clicked) await sleep(400);
    }
    if (!clicked) return { stage: 'library_link_missing', tab_id: tabId, link_text: cfg.library_link_text };
    const libPattern = cfg.library_url_pattern ? new RegExp(String(cfg.library_url_pattern)) : null;
    let libTab = await findProxiedTab(libPattern, [tabId], 7000);
    if (!libTab) {
      // one retry: the first click may have landed before the SPA wired it
      await realClickByJs(tabId, locateLibrary).catch(() => false);
      libTab = await findProxiedTab(libPattern, [tabId], 8000);
    }
    if (!libTab) {
      const same = await chrome.tabs.get(tabId).catch(() => null);
      if (same && isProxiedUrl(same.url || '')) libTab = same;
    }
    if (!libTab) return { stage: 'library_tab_missing', tab_id: tabId };
    await waitForLoad(libTab.id, 20000).catch(() => false);
    await sleep(800);
    // The library is usually a plain JSP form: native setter + input event,
    // then the query button. Retry the search because a late proxy redirect can
    // replace the document and silently clear the field.
    await chrome.tabs.update(libTab.id, { active: true }).catch(() => {});
    await sleep(400);
    const rowCell = Number.isInteger(Number(cfg.row_name_cell)) ? Number(cfg.row_name_cell) : 1;
    const setSearch = `(() => {
      const sel = ${JSON.stringify(String(cfg.library_search_input || 'input[type=text]'))};
      const i = document.querySelector(sel) || [...document.querySelectorAll('input')].find((x) => x.type === 'text' || !x.type);
      if (!i) return false;
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      setter.call(i, ${JSON.stringify(database)});
      i.dispatchEvent(new Event('input', { bubbles: true }));
      i.dispatchEvent(new Event('change', { bubbles: true }));
      return i.value === ${JSON.stringify(database)};
    })()`;
    const clickQuery = `(() => {
      const want = ${JSON.stringify(String(cfg.library_query_button_text))};
      const b = [...document.querySelectorAll('button,input[type=button],input[type=submit]')]
        .find((x) => ((x.textContent || '').trim() || x.value || '').trim() === want);
      if (b) b.click();
      return !!b;
    })()`;
    const rowExists = `(() => {
      const db = ${JSON.stringify(database)};
      const rowCell = ${JSON.stringify(rowCell)};
      return [...document.querySelectorAll('tr')].some((tr) => {
        const cells = [...tr.querySelectorAll('td')].map((td) => (td.innerText || '').trim());
        return cells[rowCell] && cells[rowCell].indexOf(db) >= 0;
      });
    })()`;
    let rowReady = false;
    for (let attempt = 0; attempt < 3 && !rowReady; attempt++) {
      const setOk = await evalJs(libTab.id, setSearch).catch(() => false);
      await sleep(250);
      await evalJs(libTab.id, clickQuery).catch(() => false);
      await sleep(2300);
      rowReady = await evalJs(libTab.id, rowExists).catch(() => false);
      if (!rowReady) await sleep(600);
      audit.log({ action: 'institution.webvpn.search', attempt, setOk, rowReady });
    }
    if (!rowReady) return { stage: 'database_row_missing', tab_id: libTab.id, url: redactUrl(libTab.url || ''), database };
    const before = new Set((await chrome.tabs.query({}).catch(() => [])).map((t) => t.id));
    const locateEntry = `(() => {
      const db = ${JSON.stringify(database)};
      const rowCell = ${JSON.stringify(rowCell)};
      const prefix = ${JSON.stringify(String(cfg.library_entry_prefix))};
      const rows = [...document.querySelectorAll('tr')]
        .map((tr) => ({ tr, cells: [...tr.querySelectorAll('td')].map((td) => (td.innerText || '').trim()) }))
        .filter((r) => r.cells.length >= 4);
      const row = rows.find((r) => r.cells[rowCell] && r.cells[rowCell].indexOf(db) >= 0)
        || rows.find((r) => (r.tr.textContent || '').indexOf(db) >= 0);
      const scope = row ? row.tr : null;
      if (!scope) return null;
      return [...scope.querySelectorAll('a')].find((x) => (x.textContent || '').trim().startsWith(prefix)) || null;
    })()`;
    const entry = await realClickByJs(libTab.id, locateEntry).catch(() => false);
    if (!entry) return { stage: 'entry_missing', tab_id: libTab.id, url: redactUrl(libTab.url || ''), database };
    let source = null;
    for (let i = 0; i < 40 && !source; i++) {
      await sleep(300);
      const tabs = await chrome.tabs.query({}).catch(() => []);
      source = tabs.find((t) => !before.has(t.id) && isProxiedUrl(t.url || '')) || null;
      if (source) break;
      const libNow = await chrome.tabs.get(libTab.id).catch(() => null);
      if (libNow && isProxiedUrl(libNow.url || '') && !(libPattern && libPattern.test(libNow.url || ''))) source = libNow;
    }
    if (!source) source = await chrome.tabs.get(libTab.id).catch(() => null);
    const targetUrl = source?.url || '';
    let adopted = null;
    if (isProxiedUrl(targetUrl)) {
      try {
        const nt = await sessions.createTab(targetUrl, { active: true });
        await armTab(nt.tab_id);
        adopted = nt.tab_id;
      } catch { /* the spawned tab is still evidence */ }
      if (source?.id && source.id !== adopted && source.id !== tabId && source.id !== libTab.id) chrome.tabs.remove(source.id).catch(() => {});
      if (libTab.id !== tabId && libTab.id !== adopted) chrome.tabs.remove(libTab.id).catch(() => {});
    }
    audit.log({ action: 'institution.webvpn', profile: p.profile || null, database, adopted: adopted != null });
    return {
      stage: adopted ? 'opened' : 'proxied_tab',
      tab_id: adopted || source?.id || libTab.id,
      url: redactUrl(targetUrl),
      title: truncate(source?.title || '', 160),
      database,
      profile: p.profile || null,
    };
  });
}

export function registerInstitutionTools(dispatcher) {
  registerInstitution(dispatcher);
}
