// dev.* — the two levers a developer needs while iterating on THIS extension
// without opening edge://extensions by hand. Both require confirm:true and are
// audited; neither is used by an agent task.

import { err, BUILD } from './config.js';
import { cdp } from './cdp.js';
import { targetTab } from './actions.js';
import { audit } from './audit.js';

export function register(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('dev.reload', {
    category: 'diagnostics',
    sensitive: true,
    description:
      'Reload this unpacked extension from disk (chrome.runtime.reload) so edited background/lib code takes effect. confirm:true required. The daemon connection drops for 1-3s and self-heals; already-injected content scripts keep the OLD code until their page reloads, so reload the tab too when content/*.js changed.',
  }, async (p = {}) => {
    if (p.confirm !== true) throw err('NEEDS_CONFIRM', 'reloading the extension drops every in-flight tool call — retry with confirm:true');
    audit.log({ action: 'dev.reload', build: BUILD });
    // answer first, then reload: a synchronous reload would kill the response
    setTimeout(() => { chrome.runtime.reload(); }, 200);
    return { reloading: true, build: BUILD, note: 'connection drops ~1-3s; reload the page to re-inject edited content scripts' };
  });


  R('dev.focus', {
    category: 'diagnostics',
    sensitive: true,
    description:
      'Emulate a focused/visible page (Emulation.setFocusEmulationEnabled) for a window Windows reports as occluded. Needed only when Chromium native occlusion marks a real (not minimized) window hidden: trusted input is refused pre-dispatch in that state. This is DevTools emulation, not a physical foreground — results carry focus_emulated:true so a caller never mistakes it for a visible window. By DEFAULT this never touches the physical window; pass restore_window:true only when you deliberately want to un-minimize and resize a test window (that steals foreground and shows up on the desktop). confirm:true required.',
  }, async (p = {}) => {
    if (p.confirm !== true) throw err('NEEDS_CONFIRM', 'focus emulation changes what the page believes about visibility — retry with confirm:true');
    const tabId = targetTab(p);
    await cdp.ensure(tabId);
    const enabled = p.enabled !== false;
    // A window launched minimized reports innerWidth/innerHeight = 0 and every
    // box collapses to a point. Un-minimizing it is a PHYSICAL, foreground-
    // stealing action: keep it behind an explicit opt-in so routine diagnostics
    // never hijack the desktop or make windows flash/resize.
    let restored = false;
    let before = null;
    const dims = async () => {
      const r = await cdp.send(tabId, 'Runtime.evaluate', {
        expression: '[innerWidth, innerHeight, document.visibilityState]',
        returnByValue: true,
      }).catch(() => null);
      return r?.result?.value || null;
    };
    before = await dims();
    if (p.restore_window === true && before && before[0] < 100) {
      const tab = await chrome.tabs.get(tabId).catch(() => null);
      if (tab?.windowId != null) {
        await chrome.windows.update(tab.windowId, { state: 'maximized', focused: true }).catch(() => {});
        await new Promise((r) => setTimeout(r, 350));
        restored = true;
      }
    }
    await cdp.send(tabId, 'Emulation.setFocusEmulationEnabled', { enabled });
    const after = await dims();
    return {
      focus_emulated: enabled,
      visibility_state: after?.[2] ?? null,
      viewport: after ? { w: after[0], h: after[1] } : null,
      restored_minimized: restored,
      restore_window_opted_in: p.restore_window === true,
      note: after && after[0] < 100
        ? 'viewport is STILL 0x0 — the window is genuinely minimized/off-screen on the OS; bring it to the foreground (or use window.restore) before any geometry-based action'
        : 'geometry is usable',
      tab_id: tabId,
    };
  });

  // KEY NAME → { code, windowsVirtualKeyCode } for the accelerator subset that
  // matters to browser-level shortcuts. Chromium routes these through the same
  // pre-handler as physical keys, so browser accelerators (ctrl+s on a PDF
  // viewer, ctrl+p) fire even when the OS window is occluded — the one honest
  // way to drive a covered window's Save dialog without stealing foreground.
  const KEY_CODES = {
    s: { code: 'KeyS', vk: 83 }, p: { code: 'KeyP', vk: 80 },
    a: { code: 'KeyA', vk: 65 }, c: { code: 'KeyC', vk: 67 }, v: { code: 'KeyV', vk: 86 },
    w: { code: 'KeyW', vk: 87 }, t: { code: 'KeyT', vk: 84 }, r: { code: 'KeyR', vk: 82 },
    f: { code: 'KeyF', vk: 70 }, j: { code: 'KeyJ', vk: 74 }, o: { code: 'KeyO', vk: 79 },
    enter: { code: 'Enter', vk: 13 }, escape: { code: 'Escape', vk: 27 },
    tab: { code: 'Tab', vk: 9 }, space: { code: 'Space', vk: 32 },
    arrowdown: { code: 'ArrowDown', vk: 40 }, arrowup: { code: 'ArrowUp', vk: 38 },
    arrowleft: { code: 'ArrowLeft', vk: 37 }, arrowright: { code: 'ArrowRight', vk: 39 },
    home: { code: 'Home', vk: 36 }, end: { code: 'End', vk: 35 },
    pageup: { code: 'PageUp', vk: 33 }, pagedown: { code: 'PageDown', vk: 34 },
  };
  const MOD_BITS = { ctrl: 2, alt: 1, shift: 8, meta: 4 };

  R('dev.key', {
    category: 'diagnostics',
    description:
      'Dispatch a keyboard event INTO a tab via CDP Input.dispatchKeyEvent — works on background/occluded tabs because it rides the DevTools pipeline, not OS input. This is how to trigger a browser accelerator (ctrl+s on the built-in PDF viewer, ctrl+p) when the OS window is covered and SendInput cannot reach it: pair with dev.focus {enabled:true} if the page must believe it is focused. key: single char or named key (enter/escape/tab/space/arrows/home/end/pageup/pagedown); modifiers: array of ctrl/alt/shift/meta. Read-only diagnostics it is not — this ACTUATES the page; use with the same care as click.',
  }, async (p = {}) => {
    const tabId = targetTab(p);
    const kname = String(p.key || '').toLowerCase();
    const spec = KEY_CODES[kname];
    if (!spec) throw err('BAD_PARAMS', `unsupported key "${kname}" — supported: ${Object.keys(KEY_CODES).join(', ')} or a-z`);
    let modifiers = 0;
    for (const m of p.modifiers || []) {
      const bit = MOD_BITS[String(m).toLowerCase()];
      if (!bit) throw err('BAD_PARAMS', `unsupported modifier "${m}"`);
      modifiers |= bit;
    }
    await cdp.ensure(tabId);
    const base = {
      key: kname.length === 1 ? kname : kname,
      code: spec.code,
      windowsVirtualKeyCode: spec.vk,
      nativeVirtualKeyCode: spec.vk,
      modifiers,
      text: modifiers === 0 && kname.length === 1 ? kname : '',
    };
    const down = await cdp.send(tabId, 'Input.dispatchKeyEvent', { type: 'keyDown', ...base }).catch((e) => ({ __error: String(e?.message || e) }));
    const up = await cdp.send(tabId, 'Input.dispatchKeyEvent', { type: 'keyUp', ...base }).catch((e) => ({ __error: String(e?.message || e) }));
    audit.log({ action: 'dev.key', tabId, key: kname, modifiers });
    return {
      dispatched: !down?.__error && !up?.__error,
      key: kname, modifiers,
      errors: [down?.__error, up?.__error].filter(Boolean),
      note: 'sent keyDown+keyUp through CDP — background tab OK; pair with dev.focus if the page gates on focus',
    };
  });

  R('dev.clear_cookies', {
    category: 'diagnostics',
    sensitive: true,
    description: 'Delete cookies for one domain (default x-mol.com) in this browser profile. confirm:true required. Used to reproduce a first-visit challenge in a disposable test profile; it never touches another profile.',
  }, async (p = {}) => {
    if (p.confirm !== true) throw err('NEEDS_CONFIRM', 'deleting cookies is destructive — retry with confirm:true');
    const domain = String(p.domain || 'x-mol.com');
    const list = await chrome.cookies.getAll({ domain });
    let cleared = 0;
    for (const c of list) {
      const host = c.domain.startsWith('.') ? c.domain.slice(1) : c.domain;
      const url = `${c.secure ? 'https' : 'http'}://${host}${c.path || '/'}`;
      const r = await chrome.cookies.remove({ url, name: c.name, storeId: c.storeId }).catch(() => null);
      if (r) cleared += 1;
    }
    return { domain, found: list.length, cleared };
  });

  R('dev.build', { category: 'diagnostics', description: 'Report the running extension build stamp and manifest version.' }, async () => {
    let manifest = null;
    try { manifest = chrome.runtime.getManifest(); } catch { /* ignore */ }
    return { build: BUILD, version: manifest?.version || null, name: manifest?.name || null };
  });
}
