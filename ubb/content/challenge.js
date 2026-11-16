// UBB challenge sensor: runs in EVERY frame (all_frames), including the
// cross-origin iframes the vendors put their widgets in. It answers one
// question per frame — "is a human-verification widget here, what kind, and
// where is its clickable point in THIS frame's viewport" — and hands the
// geometry to lib/challenge.js, which composes it into top-viewport
// coordinates and drives real CDP input there.
//
// Rules are data-shaped on purpose: a vendor is a set of URL/DOM/text
// signatures plus the selectors that make a target clickable. Adding a vendor
// must not touch the solver.
//
// Privacy: this file never reads input values. Token fields are reported as
// {present, length} only — the token itself stays in the page.

(() => {
  if (window.__ubbChallenge) return;
  const DOC_EPOCH = Math.random().toString(36).slice(2, 10);
  const IS_TOP = (() => {
    try { return window.top === window; } catch { return false; }
  })();

  // ---------------------------------------------------------------- helpers

  function visible(el) {
    if (!el || !el.getBoundingClientRect) return false;
    const r = el.getBoundingClientRect();
    if (r.width <= 1 || r.height <= 1) return false;
    const s = getComputedStyle(el);
    if (s.visibility === 'hidden' || s.display === 'none') return false;
    if (Number(s.opacity) === 0) return false;
    return true;
  }

  function boxOf(el) {
    const r = el.getBoundingClientRect();
    return {
      x: r.left + (window.scrollX || 0),
      y: r.top + (window.scrollY || 0),
      width: r.width,
      height: r.height,
      cx: r.left + r.width / 2,
      cy: r.top + r.height / 2,
      viewport_x: r.left,
      viewport_y: r.top,
    };
  }

  // boxes for click targets are VIEWPORT-relative (lib composes them through
  // the frame chain, which is expressed in the top viewport, not scroll space)
  function vbox(el) {
    const r = el.getBoundingClientRect();
    return {
      x: r.left, y: r.top, width: r.width, height: r.height,
      cx: r.left + r.width / 2, cy: r.top + r.height / 2,
    };
  }

  function text(el) {
    return String((el && (el.innerText || el.textContent)) || '').replace(/\s+/g, ' ').trim();
  }

  function attr(el, name) {
    try { return el.getAttribute(name) || ''; } catch { return ''; }
  }

  function sample(list, n = 12) {
    return list.slice(0, n);
  }

  // Open shadow roots only. Closed roots are unreachable by design and a
  // challenge living there is indistinguishable from one we cannot see — the
  // probe says so instead of guessing.
  function qsa(sel, { shadow = true } = {}) {
    const out = [...document.querySelectorAll(sel)];
    if (!shadow) return out;
    let roots = [document];
    for (let i = 0; i < roots.length && i < 12; i++) {
      let kids = [];
      try { kids = [...roots[i].querySelectorAll('*')]; } catch { kids = []; }
      for (const el of kids) {
        if (el.shadowRoot) {
          roots.push(el.shadowRoot);
          try { out.push(...el.shadowRoot.querySelectorAll(sel)); } catch { /* ignore */ }
        }
      }
    }
    return out;
  }

  function firstVisible(sel, { shadow = true } = {}) {
    for (const el of qsa(sel, { shadow })) if (visible(el)) return el;
    return null;
  }

  function clickable(el, fallbackSel) {
    if (!el) return null;
    if (visible(el)) return el;
    if (fallbackSel) {
      const near = el.closest(fallbackSel);
      if (near && visible(near)) return near;
    }
    return null;
  }

  // ------------------------------------------------------------- signatures

  const RX = {
    recaptcha: /(google\.com\/recaptcha|recaptcha\.net\/recaptcha|gstatic\.com\/recaptcha)/i,
    recaptchaAnchor: /recaptcha\/(api2|enterprise)\/anchor/i,
    recaptchaBframe: /recaptcha\/(api2|enterprise)\/bframe/i,
    hcaptcha: /(hcaptcha\.com|hcaptcha\.net)/i,
    hcaptchaChallenge: /hcaptcha\.com\/captcha\/v1\//i,
    turnstile: /challenges\.cloudflare\.com/i,
    geetest: /(geetest\.com|geetest\.com\.cn|geevisit\.com)/i,
    arkose: /(arkoselabs\.com|funcaptcha\.com)/i,
    awswaf: /(awswaf\.com|token\.awswaf)/i,
    datadome: /(datadome\.co|dd\.js)/i,
    px: /(perimeterx|px-cdn|pxchk)/i,
  };

  const TOKEN_FIELDS = [
    ['g-recaptcha-response', 'recaptcha'],
    ['h-captcha-response', 'hcaptcha'],
    ['cf-turnstile-response', 'turnstile'],
    ['mx_captcha', 'unknown'],
  ];

  function tokenState() {
    const out = [];
    for (const [name, vendor] of TOKEN_FIELDS) {
      const els = [...document.querySelectorAll(`textarea[name="${name}"],input[name="${name}"],textarea[name^="${name}"],input[name^="${name}"]`)];
      if (!els.length) continue;
      out.push({
        name, vendor,
        present: true,
        length: Math.max(0, ...els.map((e) => String(e.value || '').length)),
      });
    }
    return out;
  }

  /** The frame is the *challenge* frame (not the anchor): it carries tiles. */
  function gridTiles(vendor) {
    if (!vendor) {
      const here = String(location.href || '');
      if (RX.recaptcha.test(here)) vendor = 'recaptcha';
      else if (RX.hcaptcha.test(here)) vendor = 'hcaptcha';
      else vendor = frameVendor();
    }
    const tries = [];
    if (vendor === 'recaptcha') {
      tries.push('td.rc-imageselect-tile', 'div.rc-imageselect-tile');
    }
    if (vendor === 'hcaptcha') {
      tries.push('.task-image', 'div[role="button"].task', '.challenge-container div[role="button"]', '[class*="task"] img');
    }
    // Generic: ONLY inside a container that names itself a captcha/challenge.
    // A product grid or an image gallery is not a captcha, and treating one as
    // a challenge is worse than missing it (the banner would block the agent).
    const host = qsa('[class*="captcha" i],[id*="captcha" i],[class*="challenge" i],[class*="verify" i][class*="box" i]')
      .find((e) => visible(e));
    if (!vendor && host) {
      tries.push('td', '[role="button"]', 'img', 'canvas');
    }
    for (const sel of tries) {
      const root = !vendor && host ? host : document;
      let els = [];
      try { els = [...root.querySelectorAll(sel)]; } catch { els = []; }
      if (!vendor && host) {
        // shadow-DOM piercing is not needed for a vendor grid; do it only for
        // the generic path where the container is already identified
        els = els.filter(visible);
      } else {
        els = els.filter(visible);
      }
      if (els.length >= 6) {
        // de-duplicate nested matches (img inside a tile matched twice)
        const unique = [];
        for (const el of els) {
          if (!unique.some((u) => u === el || u.contains(el))) unique.push(el);
        }
        if (unique.length >= 6 && unique.length <= 25) {
          // tiles are near-square and similarly sized; this is what separates a
          // 3x3 challenge from a row of thumbnails
          const boxes = unique.map((e) => e.getBoundingClientRect());
          const sq = boxes.filter((b) => b.width > 20 && b.height > 20 && b.width / b.height > 0.6 && b.width / b.height < 1.7);
          if (sq.length >= Math.max(6, Math.floor(unique.length * 0.7))) return unique.slice(0, 20);
        }
      }
    }
    return [];
  }

  function gridPrompt() {
    const sels = [
      '.rc-imageselect-desc-no-canonical', '.rc-imageselect-desc', '.rc-imageselect-instructions',
      'strong', '[class*="prompt"]', '[class*="instruction"]', '[class*="desc"]',
    ];
    for (const sel of sels) {
      const el = qsa(sel).find((e) => visible(e) && text(e).length > 3 && text(e).length < 300);
      if (el) return text(el).slice(0, 240);
    }
    return '';
  }

  function submitButton() {
    // hCaptcha's Verify is a DIV[role=button], not a <button>; selecting only
    // native buttons silently loses the one control that finishes the round.
    const sels = ['#recaptcha-verify-button', 'button[type="submit"]', 'button', '[role="button"]'];
    for (const sel of sels) {
      const els = qsa(sel).filter(visible);
      const hit = els.find((e) => /(verify|验证|下一步|next|submit|确认|确定|continue|提交)/i.test(text(e)));
      if (hit) return { el: hit, label: text(hit).slice(0, 40), box: vbox(hit) };
    }
    return null;
  }

  function sliderParts() {
    const handles = [
      '.geetest_slider_button', '[class*="slider"][class*="btn"]', '[class*="slider-button"]',
      '#aliyunCaptcha-sliding-slider', '[class*="sliding-slider"]',
      // slidercaptcha (mouse-love library): the handle is a plain 40px div
      // inside .sliderMask/.sliderContainer. Put the specific paths first so a
      // generic [class*="slider"] elsewhere on the page cannot win.
      '.sliderContainer .slider', '.sliderMask .slider', '.slidercaptcha .slider',
      '[class*="handler"]', '[class*="handle"]', '[class*="drag"]', '#nc_1_n1z', '.btn_slide',
      '[class*="slide"] button', '[class*="verify"] [role="slider"]',
    ];
    let handle = null;
    for (const sel of handles) {
      const els = qsa(sel).filter(visible);
      const hit = els.find((e) => {
        const b = e.getBoundingClientRect();
        return b.width >= 16 && b.height >= 16 && b.width <= 200 && b.height <= 90;
      });
      if (hit) { handle = hit; break; }
    }
    if (!handle) return null;
    const track = handle.parentElement && visible(handle.parentElement) ? handle.parentElement : null;
    let bg = firstVisible('[class*="canvas_bg"], [class*="captcha-bg"], [class*="bg"][class*="puzzle"]');
    let slice = firstVisible('[class*="canvas_slice"], [class*="slice"], [class*="puzzle-piece"]');
    // slidercaptcha renders TWO same-size canvases: the background (opaque,
    // with a darkened puzzle hole) and the piece layer (mostly transparent,
    // class "block"). Its generic class names match none of the selectors
    // above, so map the pair structurally inside the widget.
    const scWrap = handle.closest && handle.closest('#captcha-id, .slidercaptcha');
    if (scWrap) {
      // the piece canvas fades in (opacity animation) — visible() rejects it
      // mid-fade, canvasGap loses the piece and falls to the unreliable
      // screenshot path. Grab both canvases regardless of opacity; readPixels
      // works on any attached canvas.
      const canvases = qsa('#captcha canvas, .slidercaptcha canvas');
      if (canvases.length >= 2) {
        const block = canvases.find((c) => c.classList.contains('block')) || canvases[canvases.length - 1];
        const back = canvases.find((c) => c !== block);
        if (back) bg = back;
        if (block) slice = block;
      }
    }
    // Aliyun 2.0's default slider has NO puzzle image: the whole proof is
    // "drag the handle to the far right". When there is no background/slice
    // pair and the copy says so, the solver must not hunt for a gap.
    const toEnd = !bg && !slice && /(最右|到右|滑到右|拖动到右|slide\s*(it\s*)?(to\s*)?(the\s*)?(far\s*)?right|drag\s*to\s*the\s*(far\s*)?right)/i.test(text(document.body).slice(0, 600));
    return {
      to_end: toEnd,
      handle: vbox(handle),
      track: track ? vbox(track) : null,
      bg: bg ? vbox(bg) : null,
      slice: slice ? vbox(slice) : null,
      handle_el: handle,
      bg_el: bg,
      slice_el: slice,
    };
  }

  function textCaptcha() {
    const inputSel = [
      'input[name*="captcha" i]', 'input[id*="captcha" i]', 'input[name*="verify" i]',
      'input[placeholder*="验证码"]', 'input[placeholder*="captcha" i]', 'input[aria-label*="验证码"]',
      'input[name*="vcode" i]', 'input[name*="checkcode" i]',
    ];
    let input = null;
    for (const sel of inputSel) {
      input = qsa(sel).find((e) => visible(e) && !/password|otp|one-time/i.test(`${attr(e, 'name')} ${attr(e, 'id')} ${attr(e, 'autocomplete')}`));
      if (input) break;
    }
    const imgSel = [
      'img[src*="captcha" i]', 'img[id*="captcha" i]', 'img[class*="captcha" i]',
      'img[src*="verify" i]', 'img[alt*="验证码"]', 'canvas[class*="captcha" i]',
      'img[src*="vcode" i]', 'img[src*="checkcode" i]',
    ];
    let img = null;
    for (const sel of imgSel) {
      img = qsa(sel).find((e) => visible(e));
      if (img) break;
    }
    if (!input || !img) return null;
    // the answer field must be near the image (same form block), otherwise a
    // search box named "verify" on an unrelated page would be reported
    const form = img.closest('form') || input.closest('form') || img.parentElement?.parentElement;
    if (form && input.closest('form') && input.closest('form') !== img.closest('form')) return null;
    return { input, img, box: vbox(img), input_box: vbox(input) };
  }

  function frameVendor() {
    // local fixtures and OEM integrations can name their vendor explicitly;
    // real vendor frames are still identified by URL below.
    return attr(document.documentElement, 'data-captcha-vendor') || attr(document.body, 'data-captcha-vendor') || '';
  }

  // ------------------------------------------------------------- detection

  function scanIframes(out) {
    for (const f of qsa('iframe')) {
      const src = attr(f, 'src');
      const marker = attr(f, 'data-captcha-vendor');
      let vendor = marker || '';
      let role = '';
      if (!vendor) {
        if (RX.recaptchaAnchor.test(src)) { vendor = 'recaptcha'; role = 'anchor'; }
        else if (RX.recaptchaBframe.test(src)) { vendor = 'recaptcha'; role = 'challenge'; }
        else if (RX.hcaptcha.test(src)) { vendor = 'hcaptcha'; role = 'widget'; }
        else if (RX.turnstile.test(src)) { vendor = 'turnstile'; role = 'widget'; }
        else if (RX.geetest.test(src)) { vendor = 'geetest'; role = 'widget'; }
        else if (RX.arkose.test(src)) { vendor = 'arkose'; role = 'widget'; }
        else if (RX.awswaf.test(src)) { vendor = 'awswaf'; role = 'widget'; }
      }
      if (!vendor) continue;
      const shown = visible(f);
      const b = vbox(f);
      const sizeKnown = b.width > 2 && b.height > 2;
      if (!shown || !sizeKnown) {
        // A hidden/zero-size vendor iframe is either a leftover after a pass
        // (its parent hides it) or an INVISIBLE widget (Turnstile can score with
        // no UI). Neither may block a page as if a visible challenge were there.
        if (vendor === 'turnstile') {
          out.push({
            vendor, kind: 'score', role: 'widget', confidence: 'low',
            evidence: [{ type: 'iframe_src', value: src.slice(0, 300) }, { type: 'hidden', value: 'true' }],
            widget_box: null, visible: false, src: src.slice(0, 300),
          });
        }
        continue;
      }
      out.push({
        vendor,
        kind: vendor === 'geetest' ? 'slider' : (vendor === 'arkose' ? 'hold' : 'checkbox'),
        role: role || 'widget',
        confidence: 'high',
        evidence: [{ type: 'iframe_src', value: src.slice(0, 300) }, ...(marker ? [{ type: 'data-attr', value: marker }] : [])],
        widget_box: b,
        visible: true,
        src: src.slice(0, 300),
      });
    }
  }

  function vendorForFrame() {
    const here = String(location.href || '');
    const fv = frameVendor();
    if (fv) return fv;
    if (RX.hcaptcha.test(here)) return 'hcaptcha';
    if (RX.arkose.test(here)) return 'arkose';
    if (RX.recaptchaBframe.test(here)) return 'recaptcha';
    if (RX.recaptchaAnchor.test(here)) return 'recaptcha';
    if (RX.geetest.test(here)) return 'geetest';
    return 'unknown';
  }

  function detectLocal({ quick = false } = {}) {
    const findings = [];
    const targets = [];
    const push = (f) => findings.push(f);

    // ---- vendor iframes (visible from this frame's DOM) --------------------
    scanIframes(findings);

    // ---- Cloudflare interstitial (top page replaced by a challenge page) --
    if (IS_TOP) {
      const title = String(document.title || '');
      const body = text(document.body).slice(0, 4000);
      const hasStage = !!document.querySelector('#challenge-stage, #cf-challenge-running, #challenge-form, #challenge-body-text, #challenge-error-text');
      const cfIframe = findings.some((f) => f.vendor === 'turnstile');
      // Cloudflare's copy changes per product/plan: "Just a moment…",
      // "Performing security verification", "Verification successful. Waiting…",
      // "Enable JavaScript and cookies to continue". Match the FAMILY, not one
      // sentence — an interstitial misread as "no challenge" is how an agent
      // ends up filling a form on a waiting-room page.
      const cfText = /(just a moment|are you a robot|confirm you are a human|complete the captcha challenge|performing security verification|verification successful|enable javascript and cookies|checking (your browser|if the site connection)|verify you are human|验证您是人类|请稍候|正在进行安全验证|验证成功|正在等待|本网站使用安全服务)/i;
      const cfTitle = /(just a moment|security verification|人机验证|请稍候|安全验证)/i;
      if ((cfTitle.test(title) && cfText.test(body)) || (hasStage && cfText.test(body)) || (hasStage && /cloudflare|challenge/i.test(title))) {
        push({
          vendor: 'cloudflare', kind: 'interstitial', role: 'interstitial',
          confidence: 'high',
          evidence: [{ type: 'title', value: title.slice(0, 120) }, { type: 'dom', value: 'challenge-stage' }],
          prompt: body.slice(0, 200),
        });
      }
      // passive score surfaces — never blocking, but worth one line
      const badge = document.querySelector('.grecaptcha-badge');
      const v3script = [...document.querySelectorAll('script[src*="recaptcha"][src*="render="],script[src*="recaptcha/api.js"]')]
        .map((s) => attr(s, 'src')).find((s) => /render=/.test(s));
      if (badge && !findings.some((f) => f.vendor === 'recaptcha')) {
        push({
          vendor: 'recaptcha', kind: 'score', role: 'badge', confidence: 'medium',
          evidence: [{ type: 'dom', value: '.grecaptcha-badge' }],
        });
      } else if (v3script && !findings.some((f) => f.vendor === 'recaptcha')) {
        push({
          vendor: 'recaptcha', kind: 'score', role: 'badge', confidence: 'low',
          evidence: [{ type: 'script', value: v3script.slice(0, 200) }],
        });
      }
    }

    // ---- vendor DOM widgets ------------------------------------------------
    const gRecaptcha = firstVisible('.g-recaptcha, [data-sitekey][class*="recaptcha"]');
    if (gRecaptcha && !findings.some((f) => f.vendor === 'recaptcha' && f.role !== 'badge')) {
      const size = attr(gRecaptcha, 'data-size') || attr(gRecaptcha.querySelector('[data-size]') || gRecaptcha, 'data-size');
      push({
        vendor: 'recaptcha', kind: size === 'invisible' ? 'score' : 'checkbox', role: 'widget',
        confidence: 'high',
        evidence: [{ type: 'dom', value: '.g-recaptcha' }],
        widget_box: vbox(gRecaptcha),
      });
    }
    const cfTurnstile = firstVisible('.cf-turnstile, [data-sitekey][class*="turnstile"]');
    if (cfTurnstile && !findings.some((f) => f.vendor === 'turnstile')) {
      const size = attr(cfTurnstile, 'data-size');
      push({
        vendor: 'turnstile', kind: size === 'invisible' ? 'score' : 'checkbox', role: 'widget',
        confidence: 'high', evidence: [{ type: 'dom', value: '.cf-turnstile' }],
        widget_box: vbox(cfTurnstile),
      });
    }
    const gt = firstVisible('.geetest_holder, .geetest_panel, [class*="geetest_holder"], [class*="yidun"], #tcaptcha_iframe');
    if (gt && !findings.some((f) => f.vendor === 'geetest')) {
      const parts = sliderParts();
      push({
        vendor: 'geetest', kind: parts ? 'slider' : 'checkbox', role: 'widget',
        confidence: parts ? 'high' : 'medium',
        evidence: [{ type: 'dom', value: 'geetest/slider widget' }],
        widget_box: vbox(gt),
        slide_to_end: !!parts?.to_end,
        slider: parts ? { handle: parts.handle, track: parts.track, bg: parts.bg, slice: parts.slice } : null,
      });
      if (parts) targets.push({ name: 'slider handle', role: 'slider', box: parts.handle, selector: '.geetest_slider_button' });
    }
    // slidercaptcha (github.com/mouse-love/slidercaptcha, common on Chinese
    // campus SSO pages): wrapper #captcha-id/.slidercaptcha, TWO canvases
    // (background + piece layer) and a plain .slider handle. The copy says
    // "Slide to the right", but the proof is a puzzle hole, not slide-to-end.
    const scWidget = firstVisible('#captcha-id, .slidercaptcha');
    if (scWidget && !findings.some((f) => f.vendor === 'slidercaptcha')) {
      const parts = sliderParts();
      if (parts) {
        push({
          vendor: 'slidercaptcha', kind: 'slider', role: 'widget', confidence: 'high',
          evidence: [{ type: 'dom', value: 'slidercaptcha' }, { type: 'dom', value: '#captcha .sliderContainer' }],
          widget_box: vbox(scWidget),
          slide_to_end: false,
          slider: parts ? { handle: parts.handle, track: parts.track, bg: parts.bg, slice: parts.slice } : null,
        });
        targets.push({ name: 'slider handle', role: 'slider', box: parts.handle, selector: '.sliderContainer .slider' });
      }
    }

    // Aliyun Captcha 2.0: the widget lives in the TOP document, ids are
    // aliyunCaptcha-* / captcha-element, and the default mode is a drag-to-end
    // handle with no puzzle image. It ships no iframe with a vendor URL, so
    // URL-signature detection alone can never see it.
    const aliyun = firstVisible('#aliyunCaptcha-sliding-wrapper, #aliyunCaptcha-window-embed, [id^="aliyunCaptcha"], .aliyun-captcha');
    if (aliyun && !findings.some((f) => f.vendor === 'aliyun')) {
      const parts = sliderParts();
      const scriptSays = [...document.querySelectorAll('script')].some((sc) => /initAliyunCaptcha|阿里云验证码/i.test(sc.textContent || '') || /aliyun|alicdn/i.test(sc.src || ''));
      push({
        vendor: 'aliyun', kind: parts ? 'slider' : 'checkbox', role: 'widget',
        confidence: parts ? 'high' : 'medium',
        evidence: [{ type: 'dom', value: 'aliyunCaptcha widget' }, ...(scriptSays ? [{ type: 'script', value: 'initAliyunCaptcha' }] : [])],
        widget_box: vbox(aliyun),
        slide_to_end: !!parts?.to_end,
        slider: parts ? { handle: parts.handle, track: parts.track, bg: parts.bg, slice: parts.slice } : null,
      });
      if (parts) targets.push({ name: 'slider handle', role: 'slider', box: parts.handle, selector: '#aliyunCaptcha-sliding-slider' });
    }

    // ---- this frame IS a vendor widget frame ------------------------------
    const localUrl = String(location.href || '');
    const fv = frameVendor();
    if (RX.recaptchaAnchor.test(localUrl) || fv === 'recaptcha') {
      const el = clickable(firstVisible('#recaptcha-anchor') || firstVisible('.recaptcha-checkbox-border') || firstVisible('[role="checkbox"]'));
      if (el) {
        push({
          vendor: 'recaptcha', kind: 'checkbox', role: 'anchor', confidence: 'high',
          evidence: [{ type: 'dom', value: '#recaptcha-anchor' }, { type: 'frame_url', value: localUrl.slice(0, 200) }],
          widget_box: vbox(el),
        });
        targets.push({ name: text(el) || "I'm not a robot", role: 'checkbox', box: vbox(el), selector: '#recaptcha-anchor' });
      }
    }
    if (RX.hcaptcha.test(localUrl) || fv === 'hcaptcha') {
      const el = clickable(firstVisible('#checkbox') || firstVisible('[role="checkbox"]'));
      if (el) {
        push({
          vendor: 'hcaptcha', kind: 'checkbox', role: 'anchor', confidence: 'high',
          evidence: [{ type: 'dom', value: '#checkbox' }, { type: 'frame_url', value: localUrl.slice(0, 200) }],
          widget_box: vbox(el),
        });
        targets.push({ name: text(el) || 'I am human', role: 'checkbox', box: vbox(el), selector: '#checkbox' });
      }
    }
    if (RX.turnstile.test(localUrl) || fv === 'turnstile') {
      const el = clickable(firstVisible('input[type="checkbox"]') || firstVisible('.cb-lb') || firstVisible('label'));
      if (el) {
        push({
          vendor: 'turnstile', kind: 'checkbox', role: 'widget', confidence: 'high',
          evidence: [{ type: 'dom', value: 'turnstile checkbox' }, { type: 'frame_url', value: localUrl.slice(0, 200) }],
          widget_box: vbox(el),
        });
        targets.push({ name: 'Verify you are human', role: 'checkbox', box: vbox(el), selector: 'input[type=checkbox]' });
      }
    }
    if (RX.arkose.test(localUrl) || fv === 'arkose') {
      const el = clickable(firstVisible('#home_children_button') || firstVisible('button'));
      if (el) {
        push({
          vendor: 'arkose', kind: 'hold', role: 'widget', confidence: 'high',
          evidence: [{ type: 'dom', value: 'arkose press-and-hold' }],
          widget_box: vbox(el),
        });
        targets.push({ name: 'Press and hold', role: 'hold', box: vbox(el), selector: 'button' });
      }
    }

    // ---- grid challenge frame ---------------------------------------------
    if (!quick) {
      // Canvas challenges (hCaptcha drag/select, Arkose): the whole puzzle is
      // ONE <canvas role=img>. There are no tiles to index, so the solver
      // returns a screenshot and takes normalized coordinates back.
      const canvas = firstVisible('canvas[role="img"], canvas[class*="challenge"], canvas[class*="captcha"]');
      const promptText = gridPrompt() || text(document.body).slice(0, 240);
      const vendorHere = vendorForFrame();
      const canvasish = canvas && (() => { const r = canvas.getBoundingClientRect(); return r.width >= 180 && r.height >= 120; })();
      const dragPrompt = /(drag|select all|click each|hold|拖动|选择所有|点击每|按住)/i.test(promptText);
      if (canvasish && (vendorHere !== 'unknown' || dragPrompt)) {
        push({
          vendor: vendorHere, kind: 'canvas', role: 'challenge', confidence: 'high',
          evidence: [{ type: 'dom', value: 'canvas[role=img]' }, ...(promptText ? [{ type: 'text', value: promptText.slice(0, 160) }] : [])],
          widget_box: vbox(canvas),
          prompt: promptText.slice(0, 240),
        });
      }
      const tiles = gridTiles('');
      if (tiles.length >= 6) {
        const b = tiles.map(vbox);
        const x1 = Math.min(...b.map((v) => v.x));
        const y1 = Math.min(...b.map((v) => v.y));
        const x2 = Math.max(...b.map((v) => v.x + v.width));
        const y2 = Math.max(...b.map((v) => v.y + v.height));
        push({
          vendor: '', kind: 'grid', role: 'challenge', confidence: 'medium',
          evidence: [{ type: 'dom', value: `${tiles.length} tiles` }],
          widget_box: { x: x1, y: y1, width: x2 - x1, height: y2 - y1, cx: (x1 + x2) / 2, cy: (y1 + y2) / 2 },
          prompt: gridPrompt(),
        });
      }
      const tc = textCaptcha();
      if (tc) {
        push({
          vendor: 'generic', kind: 'text', role: 'widget', confidence: 'high',
          evidence: [{ type: 'dom', value: 'captcha image + answer input' }],
          widget_box: tc.box,
        });
      }
    }

    // ---- token echo (page-side evidence, never the value) -----------------
    const tokens = tokenState();
    return {
      frame: {
        url: String(location.href || ''),
        title: String(document.title || ''),
        is_top: IS_TOP,
        viewport: { w: innerWidth, h: innerHeight },
        dpr: window.devicePixelRatio || 1,
        scroll: { x: scrollX || 0, y: scrollY || 0 },
      },
      findings,
      targets,
      tokens,
      epoch: DOC_EPOCH,
    };
  }

  // ----------------------------------------------------------- grid / slider

  function gridMap() {
    const tiles = gridTiles('');
    if (tiles.length < 6) return null;
    const boxes = tiles.map((el, i) => ({ i, box: vbox(el) }));
    const xs = [...new Set(boxes.map((t) => Math.round(t.box.x)))].sort((a, b) => a - b);
    const ys = [...new Set(boxes.map((t) => Math.round(t.box.y)))].sort((a, b) => a - b);
    const cols = xs.length;
    const rows = ys.length;
    const x1 = Math.min(...boxes.map((t) => t.box.x));
    const y1 = Math.min(...boxes.map((t) => t.box.y));
    const x2 = Math.max(...boxes.map((t) => t.box.x + t.box.width));
    const y2 = Math.max(...boxes.map((t) => t.box.y + t.box.height));
    return {
      tiles: boxes,
      rows, cols,
      grid_box: { x: x1, y: y1, width: x2 - x1, height: y2 - y1, cx: (x1 + x2) / 2, cy: (y1 + y2) / 2 },
      prompt: gridPrompt(),
      submit: submitButton(),
      vendor: RX.recaptchaBframe.test(location.href) ? 'recaptcha' : (RX.hcaptcha.test(location.href) ? 'hcaptcha' : ''),
    };
  }

  function sliderMap() {
    const parts = sliderParts();
    if (!parts) return null;
    return {
      handle: parts.handle,
      track: parts.track,
      bg: parts.bg,
      slice: parts.slice,
      to_end: !!parts.to_end,
      // viewport-relative; screenshot analysis needs the same origin, so the
      // caller passes the clip it used and we subtract it there
    };
  }

  // ---- pixel side: gap detection inside the page (no deps, no network) ----
  function readPixels(el) {
    try {
      if (el && el.tagName === 'CANVAS') {
        const ctx = el.getContext('2d', { willReadFrequently: true });
        return { w: el.width, h: el.height, data: ctx.getImageData(0, 0, el.width, el.height).data, el };
      }
    } catch { /* tainted canvas (cross-origin without CORS) → caller falls back */ }
    return null;
  }

  function grayAt(px, x, y) {
    if (!px || !px.data || !Number.isFinite(x) || !Number.isFinite(y)) return 0;
    const i = (y * px.w + x) * 4;
    const d = px.data;
    if (i < 0 || i + 2 >= d.length || !Number.isFinite(i)) return 0;
    return (d[i] * 0.299 + d[i + 1] * 0.587 + d[i + 2] * 0.114);
  }

  /** Alpha bounding box of the piece canvas; also returns a mask. */
  function alphaBox(px) {
    let x1 = px.w, y1 = px.h, x2 = -1, y2 = -1;
    for (let y = 0; y < px.h; y++) {
      for (let x = 0; x < px.w; x++) {
        const a = px.data[(y * px.w + x) * 4 + 3];
        if (a > 16) {
          if (x < x1) x1 = x;
          if (x > x2) x2 = x;
          if (y < y1) y1 = y;
          if (y > y2) y2 = y;
        }
      }
    }
    if (x2 < x1) return null;
    return { x: x1, y: y1, w: x2 - x1 + 1, h: y2 - y1 + 1 };
  }

  /**
   * Zero-normalized cross-correlation of the PIECE TEXTURE against the
   * background, slid horizontally. This is the primary gap finder and it is
   * fundamentally better than edge scoring, for a reason that is worth stating
   * plainly: the piece is a crop OF THE BACKGROUND ITSELF, so at the hole its
   * RGB content is nearly identical to what the background holds — modulo the
   * widget's darkening overlay, which NCC is invariant to because it subtracts
   * the mean and divides by the norm.
   *
   * Everything else in this file keys on EDGES or on a darkened run, and edges
   * are everywhere in a photo. Measured on a live sample (night camper van):
   * outlineGap answered 74 with 198.3 and its runner-up 73 scored 196.4 — a
   * 1% spread, i.e. it was picking the tent's contrast edge, not the hole.
   * NCC on the same pixels answered 106 with 0.958, and dragging to 106 passed
   * the captcha on the first try while 74 failed.
   *
   * The piece canvas is mostly transparent, so the weight is alpha * mask:
   * fully transparent pixels must not contribute (their RGB is meaningless).
   */
  function matchPieceNCC(bg, piece, ab) {
    const PX = 1.0; // sample every pixel; the canvases are small (278x155)
    // bbox from a SOLID threshold: the antialiased halo carries stale RGB from
    // a previous render and drags the correlation down
    const SOLID_A = 160;
    const WT = 0.63;
    let sx1 = 1e9, sy1 = 1e9, sx2 = -1, sy2 = -1;
    for (let y = 0; y < piece.h; y++) {
      for (let x = 0; x < piece.w; x++) {
        if (piece.data[(y * piece.w + x) * 4 + 3] > SOLID_A) {
          if (x < sx1) sx1 = x; if (x > sx2) sx2 = x;
          if (y < sy1) sy1 = y; if (y > sy2) sy2 = y;
        }
      }
    }
    if (sx2 < sx1) return null;
    const w = sx2 - sx1 + 1, h = sy2 - sy1 + 1;
    if (w < 12 || h < 12) return null;
    // A screenshot of the widget has NO alpha channel: every pixel is opaque,
    // so an alpha weight degenerates to "the whole rectangle". Detect that and
    // fall back to an unweighted rectangle mask — the correlation still works,
    // it just includes the rectangle's background fringe.
    let opaque = 0;
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        if (piece.data[((sy1 + y) * piece.w + (sx1 + x)) * 4 + 3] > 250) opaque++;
      }
    }
    const noAlpha = opaque > w * h * 0.97;
    // piece patch: RGB (mean-subtracted later) + alpha weight
    const pR = new Float64Array(w * h), pG = new Float64Array(w * h), pB = new Float64Array(w * h);
    const pW = new Float64Array(w * h);
    let wsum = 0, nUsed = 0;
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const pi = ((sy1 + y) * piece.w + (sx1 + x)) * 4;
        const a = noAlpha ? 1 : piece.data[pi + 3] / 255;
        const k = y * w + x;
        if (a < WT) { pW[k] = 0; continue; }
        pR[k] = piece.data[pi]; pG[k] = piece.data[pi + 1]; pB[k] = piece.data[pi + 2];
        pW[k] = a; wsum += a; nUsed++;
      }
    }
    if (nUsed < 80 || wsum < 60) return null;
    // weighted means per channel
    let mR = 0, mG = 0, mB = 0;
    for (let k = 0; k < w * h; k++) {
      const wt = pW[k]; if (!wt) continue;
      mR += pR[k] * wt; mG += pG[k] * wt; mB += pB[k] * wt;
    }
    mR /= wsum; mG /= wsum; mB /= wsum;
    // zero-mean, weighted piece vector + its norm
    let pnorm2 = 0;
    for (let k = 0; k < w * h; k++) {
      const wt = pW[k]; if (!wt) continue;
      const dr = pR[k] - mR, dg = pG[k] - mG, db = pB[k] - mB;
      pR[k] = dr; pG[k] = dg; pB[k] = db;
      pnorm2 += wt * (dr * dr + dg * dg + db * db);
    }
    const pnorm = Math.sqrt(pnorm2);
    if (pnorm < 1e-6) return null;
    // the piece is drawn on the bg canvas at its start position: skip past it
    const minX = Math.max(1, Math.round(w * 1.15));
    const maxX = bg.w - w;
    if (maxX <= minX) return null;
    const scores = [];
    for (let xL = minX; xL <= maxX; xL++) {
      let bR = 0, bG = 0, bB = 0;
      for (let y = 0; y < h; y++) {
        const rowBase = (sy1 + y) * bg.w;
        for (let x = 0; x < w; x++) {
          const k = y * w + x;
          const wt = pW[k]; if (!wt) continue;
          const bi = (rowBase + xL + x) * 4;
          bR += bg.data[bi] * wt; bG += bg.data[bi + 1] * wt; bB += bg.data[bi + 2] * wt;
        }
      }
      bR /= wsum; bG /= wsum; bB /= wsum;
      let dot = 0, bnorm2 = 0;
      for (let y = 0; y < h; y++) {
        const rowBase = (sy1 + y) * bg.w;
        for (let x = 0; x < w; x++) {
          const k = y * w + x;
          const wt = pW[k]; if (!wt) continue;
          const bi = (rowBase + xL + x) * 4;
          const dr = bg.data[bi] - bR, dg = bg.data[bi + 1] - bG, db = bg.data[bi + 2] - bB;
          dot += wt * (pR[k] * dr + pG[k] * dg + pB[k] * db);
          bnorm2 += wt * (dr * dr + dg * dg + db * db);
        }
      }
      if (bnorm2 < 1e-6) continue;
      scores.push({ x: xL, score: dot / (pnorm * Math.sqrt(bnorm2)) });
    }
    if (!scores.length) return null;
    scores.sort((a, b) => b.score - a.score);
    const best = scores[0];
    // margin against the best candidate OUTSIDE the peak's neighbourhood —
    // adjacent columns are the same peak sampled one pixel apart, and treating
    // them as competitors made every photo background look "flat" (0.008).
    let second = 0;
    for (let i = 1; i < scores.length; i++) {
      if (Math.abs(scores[i].x - best.x) > 6) { second = scores[i].score; break; }
    }
    const margin = best.score > 0 ? (best.score - second) / best.score : 0;
    return { x: best.x, score: best.score, margin, w, h, top: scores.slice(0, 5) };
  }

  /** Template match: where in the background does the piece content sit? */
  function matchPiece(bg, piece, ab) {
    const maxX = bg.w - ab.w;
    const y0 = ab.y;
    let best = null;
    let second = Infinity;
    for (let x = 0; x <= maxX; x += 1) {
      let sum = 0, n = 0;
      for (let y = 0; y < ab.h; y += 2) {
        for (let dx = 0; dx < ab.w; dx += 2) {
          const a = piece.data[((y + ab.y) * piece.w + (dx + ab.x)) * 4 + 3];
          if (a < 32) continue;
          const p = grayAt(piece, dx + ab.x, y + ab.y);
          const g = grayAt(bg, x + dx, y0 + y);
          sum += Math.abs(p - g); n++;
        }
      }
      if (!n) continue;
      const score = sum / n;
      if (best === null || score < best.score) { second = best ? best.score : Infinity; best = { x, score }; }
      else if (score < second) second = score;
    }
    if (!best) return null;
    const margin = second === Infinity ? 0 : (second - best.score) / Math.max(1, second);
    return { x: best.x, score: best.score, margin };
  }

  /** Column-profile: the punched hole is a darker run ~piece-width wide. */
  function darkRun(bg, pieceW) {
    const H = bg.h;
    const cols = new Float64Array(bg.w);
    const y0 = Math.floor(H * 0.15);
    const y1 = Math.floor(H * 0.9);
    for (let x = 0; x < bg.w; x++) {
      let s = 0;
      for (let y = y0; y < y1; y += 2) s += grayAt(bg, x, y);
      cols[x] = s / Math.max(1, Math.ceil((y1 - y0) / 2));
    }
    const smooth = new Float64Array(bg.w);
    for (let x = 0; x < bg.w; x++) {
      let s = 0, n = 0;
      for (let k = -2; k <= 2; k++) { const xx = x + k; if (xx >= 0 && xx < bg.w) { s += cols[xx]; n++; } }
      smooth[x] = s / n;
    }
    const minW = Math.max(12, Math.round(pieceW * 0.7));
    const maxW = Math.max(minW + 2, Math.round(pieceW * 1.35));
    let best = null;
    for (let x = 4; x < bg.w - minW; x++) {
      for (let w = minW; w <= Math.min(maxW, bg.w - x - 4); w += 2) {
        let inside = 0, outside = 0, ni = 0, no = 0;
        for (let k = 0; k < w; k++) { inside += smooth[x + k]; ni++; }
        for (let k = 1; k <= 6; k++) {
          if (x - k >= 0) { outside += smooth[x - k]; no++; }
          if (x + w - 1 + k < bg.w) { outside += smooth[x + w - 1 + k]; no++; }
        }
        if (!ni || !no) continue;
        const contrast = (outside / no) - (inside / ni);
        if (contrast > 6 && (!best || contrast > best.contrast)) best = { x, w, contrast };
      }
    }
    if (!best) return null;
    return { x: best.x, w: best.w, contrast: best.contrast };
  }

  /**
   * Outline template match (validated live on slidercaptcha: cat/ink/
   * corridor/truck backgrounds, ±2px every time). The hole carries a dark
   * stroke ON the piece silhouette, so sliding the piece's OUTLINE across the
   * background and maximizing the background's horizontal gradient there finds
   * the hole even when darkRun and full-texture matchPiece both fail (flat
   * synthetic pieces, dark photos, low-contrast fills).
   */
  function outlineGap(bg, piece, ab, skipFrom = 0) {
    // outline = mask pixels with at least one 4-neighbour outside the mask
    // SOLID mask only: the piece's antialiased halo (alpha 17..199) fragments
    // the silhouette and shifts the gradient peak. gap_v3 (the offline
    // reference that verified 15+ live drags) uses alpha > 200 for BOTH the
    // bbox and the outline — mirror that exactly, including recomputing the
    // bbox from the same threshold (alphaBox uses 16 and includes the halo).
    const SOLID = 200;
    const solid = (x, y) => piece.data[((y) * piece.w + (x)) * 4 + 3] > SOLID;
    let sx1 = 1e9, sy1 = 1e9, sx2 = -1, sy2 = -1;
    for (let y = 0; y < piece.h; y++) {
      for (let x = 0; x < piece.w; x++) {
        if (!solid(x, y)) continue;
        if (x < sx1) sx1 = x; if (x > sx2) sx2 = x;
        if (y < sy1) sy1 = y; if (y > sy2) sy2 = y;
      }
    }
    if (sx2 < sx1) return null;
    const abx = sx1, aby = sy1, abw = sx2 - sx1 + 1, abh = sy2 - sy1 + 1;
    const inMask = (x, y) => solid(x, y);
    const outline = [];
    for (let y = aby; y <= sy2; y++) {
      for (let x = abx; x <= sx2; x++) {
        if (!inMask(x, y)) continue;
        if (!inMask(x + 1, y) || !inMask(x - 1, y) || !inMask(x, y + 1) || !inMask(x, y - 1)) {
          outline.push([x, y]);
        }
      }
    }
    if (outline.length < 24) return null;
    // skipFrom: the bg canvas DRAWS THE PIECE at its current position, so the
    // outline self-matches perfectly at xL≈piece.x and returns offset≈0-4
    // (measured live: dragged:4 rounds). Search only to the RIGHT of the piece.
    const minX = Math.max(1, skipFrom);
    const maxX = bg.w - abw - 2;
    const scores = [];
    for (let xL = minX; xL <= maxX; xL++) {
      let sum = 0, n = 0;
      for (let k = 0; k < outline.length; k++) {
        const bx = xL + (outline[k][0] - abx);
        const by = outline[k][1];
        if (bx <= 0 || bx + 1 >= bg.w || by < 0 || by >= bg.h) { sum = -1; break; }
        sum += Math.abs(grayAt(bg, bx + 1, by) - grayAt(bx - 1, by));
        n++;
      }
      if (n && sum >= 0) {
        const score = sum / n;
        scores.push({ x: xL, score });
      }
    }
    scores.sort((a, b) => b.score - a.score);
    const best = scores[0] || null;
    window.__ubbOutlineTop3 = scores.slice(0, 3);
    if (!best) return null;
    // second-best margin: a decisive peak beats a flat field
    let second = 0;
    for (let xL = minX; xL <= maxX; xL++) {
      let sum = 0, n = 0;
      for (let k = 0; k < outline.length; k++) {
        const bx = xL + (outline[k][0] - abx);
        const by = outline[k][1];
        if (bx <= 0 || bx + 1 >= bg.w || by < 0 || by >= bg.h) { sum = -1; break; }
        sum += Math.abs(grayAt(bg, bx + 1, by) - grayAt(bx - 1, by));
        n++;
      }
      if (n && sum >= 0 && xL !== best.x) {
        const score = sum / n;
        if (score > second) second = score;
      }
    }
    const margin = best.score > 0 ? (best.score - second) / best.score : 0;
    return { x: best.x, score: best.score, margin };
  }

  async function canvasGap(args = {}) {
    try {
      return await canvasGapInner(args);
    } catch (e) {
      throw new Error('canvasGap failed: ' + (e?.message || e) + ' @ ' + String(e?.stack || '').split(String.fromCharCode(10)).slice(1, 4).join(' || '));
    }
  }

  async function canvasGapInner(args = {}) {
    const parts = sliderParts();
    if (!parts || !parts.bg_el) { window.__ubbCanvasGapDebug = { noParts: true }; return null; }
    window.__ubbCanvasGapDebug = { stage: 'parts-ok' };
    // the widget ZEROES the canvases during reset/fade (width→0, getImageData
    // throws, readPixels→null) — wait for a real render instead of silently
    // returning null and dropping the solve to the screenshot self-match path
    let bg = null;
    for (let i = 0; i < 12; i++) {
      bg = readPixels(parts.bg_el);
      if (bg && bg.w >= 100) {
        // painted check: a blank/unpainted canvas has near-zero column variance
        // (all zeros during the widget's reset window — the exact race that made
        // the internal canvasGap compute garbage while the grabbed PNG was fine)
        let mn = 255, mx = 0;
        for (let x = 0; x < bg.w; x += 4) {
          const i2 = (Math.floor(bg.h / 2) * bg.w + x) * 4;
          const l = bg.data[i2] + bg.data[i2 + 1] + bg.data[i2 + 2];
          if (l < mn) mn = l; if (l > mx) mx = l;
        }
        if (mx - mn > 30) break; // real photo content
      }
      await new Promise((r) => setTimeout(r, 350));
      bg = null;
    }
    if (!bg) { window.__ubbCanvasGapDebug = { stage: 'bg-null' }; return null; }
    try { window.__ubbBgDump = parts.bg_el.toDataURL('image/png'); } catch { window.__ubbBgDump = null; }
    let piece = null;
    let lastCount = -1;
    for (let i = 0; i < 10; i++) {
      piece = parts.slice_el ? readPixels(parts.slice_el) : null;
      let cntN = 0;
      if (piece) {
        const c = piece.data;
        for (let i = 3; i < c.length; i += 4) if (c[i] > 32) cntN++;
      }
      // the piece canvas fades in — its alpha count grows until settled.
      // Require the count to STABILIZE across two consecutive reads before
      // trusting the mask (a mid-fade mask produces a shifted outline).
      if (piece && cntN > 100 && cntN === lastCount) break;
      lastCount = cntN;
      await new Promise((r) => setTimeout(r, 350));
    }
    const pbox = piece ? alphaBox(piece) : null;
    // the piece lives inside a full-width slice canvas; its own bbox is the
    // only honest width for the hole profile
    const pieceW = pbox ? pbox.w : Math.round(bg.h * 0.22);
    let hit = null;
    let method = '';
    let confidence = 0;
    // Prefer the punched-hole profile: every slider in the wild darkens or
    // lightens the hole, while template-matching the piece only works when the
    // piece carries real background texture (a flat synthetic piece matches
    // half the gradient and lands on the wrong column).
    // SELF-MATCH GUARD: the bg canvas paints the piece at its CURRENT position
    // (yjsgll soak) — dark-run keys on the piece's own dark edges
    // (x≈pieceW) and matchPiece keys on the piece's exact render (x≈0), both
    // returning 'the piece itself' as the hole → dx≈0-4 → the handle jiggles
    // 4px and resets, forever. The hole is ALWAYS to the right of the piece.
    const d = darkRun(bg, pieceW);
    if (d && d.contrast >= 8 && d.x > pieceW + 6) { hit = { x: d.x }; method = 'dark-run'; confidence = Math.min(0.95, 0.5 + d.contrast / 60); }
    if ((!hit || hit.method === 'dark-run') && piece) {
      const ab = pbox;
      if (ab) {
        const m = matchPiece(bg, piece, ab);
        if (m && m.score < 24 && m.margin > 0.25 && (m.x - ab.x) > pieceW + 6) { hit = { x: m.x - ab.x }; method = 'template'; confidence = Math.min(0.95, 0.5 + m.margin * 2); }
      }
    }
    // The bg canvas paints the piece at its CURRENT position — an outline
    // search from xL=1 self-matches there and returns offset≈0-4 (yjsgll soak).
    // Start the search one piece-width to the right of the piece.
    const bgCssW0 = parts.bg ? parts.bg.width : bg.w;
    const scale0 = bgCssW0 > 0 ? bgCssW0 / bg.w : 1;
    const pieceLeftCss = parts.slice ? (parts.slice.x - (parts.bg ? parts.bg.x : 0)) : 0;
    const skipFrom = Math.max(1, Math.round(pieceW * 1.15)); // skip the piece's own render on the bg canvas (self-match at xL≈1 scores 50+, 4× the hole stroke)
    // NCC IS THE AUTHORITY — unconditionally, and the reason matters.
    //
    // The piece is a crop OF THE BACKGROUND ITSELF, so at the true hole its
    // texture correlates at 0.5-0.96. NCC subtracts the mean and divides by
    // the norm, which makes it invariant to the widget's darkening overlay.
    //
    // Every edge/contrast method in this file answers a DIFFERENT question —
    // "where is there a lot of contrast?" — and a photo is full of contrast.
    // Measured, on live captures:
    //   * night camper van: outlineGap 74 (198.3) vs runner-up 73 (196.4) —
    //     a 1% spread on the tent's edge, while NCC said 106 (0.958). Dragging
    //     74 FAILED, 106 PASSED on the first try.
    //   * balloons over clouds: outline scored 250.1 at x=133 (wrong) while
    //     NCC scored 0.52 at x=191 (right — the grey hole spans x=190..229).
    //     The two scores are not on a comparable scale, so "outline looks
    //     stronger, trust it" is a category error; that exact mistake caused
    //     the only failure in a 21-attempt live run.
    //
    // Live result with NCC as the sole decider: 20/20 verified drags.
    // NCC is used whenever it returns ANY candidate; the thresholds below only
    // decide how CONFIDENT to report, never whether to accept the answer.
    let nccRes = null;
    if (piece && pbox) {
      nccRes = matchPieceNCC(bg, piece, pbox);
      if (nccRes && Number.isFinite(nccRes.x)) {
        hit = { x: nccRes.x }; method = 'ncc';
        confidence = Math.min(0.97, Math.max(0.3,
          0.45 + nccRes.margin * 0.9 + Math.max(0, nccRes.score - 0.5) * 0.4));
      }
    }
    // Outline is a LAST RESORT for the ANSWER, but it is always COMPUTED so the
    // candidates block can show both answers and an auditor can see the
    // disagreement. (Live: outline was wrong in 20/20 rounds where NCC was
    // right, so it must never win.)
    let outlineRes = null;
    if (piece && pbox) {
      outlineRes = outlineGap(bg, piece, pbox, skipFrom);
      if (!hit && outlineRes && outlineRes.score > 8) {
        hit = { x: outlineRes.x }; method = 'outline';
        confidence = Math.min(0.97, 0.55 + Math.min(outlineRes.margin, 0.4) * 2);
      }
    }
    // dark-run may still be the only thing that survives on a widget whose
    // piece canvas is unreadable
    if (!hit && d && d.contrast >= 8 && d.x > pieceW + 6) {
      hit = { x: d.x }; method = 'dark-run';
      confidence = Math.min(0.95, 0.5 + d.contrast / 60);
    }
    if (!hit) {
      // diagnostic throw: the solver's .catch swallows null returns silently —
      // surface WHY nothing matched (self-match guards, empty piece canvas…)
      throw new Error('no-hit ' + JSON.stringify({
        pieceW, skipFrom, bgW: bg.w, bgH: bg.h,
        hasPiece: !!piece, pbox,
        darkRun: d ? { x: d.x, contrast: Math.round(d.contrast * 10) / 10 } : null,
        outline: outlineRes ? { x: outlineRes.x, score: Math.round(outlineRes.score * 10) / 10, margin: Math.round(outlineRes.margin * 3) / 1000 } : null,
        ncc: nccRes ? { x: nccRes.x, score: Math.round(nccRes.score * 1000) / 1000, margin: Math.round(nccRes.margin * 1000) / 1000 } : null,
      }));
    }
    const bgLeft = parts.bg ? parts.bg.x : 0;
    const pieceLeft = parts.slice ? parts.slice.x : bgLeft;
    const bgCssW = parts.bg ? parts.bg.width : bg.w;
    // canvases are often device-pixel sized (2x on HiDPI); the handle travels
    // in CSS px, so the hole column must be converted before it becomes a drag
    const scale = bgCssW > 0 ? bgCssW / bg.w : 1;
    // INPUT FINGERPRINT: a solver is only trustworthy if you can prove which
    // pixels it scored. Two live calls on the SAME rendered image returned
    // different offsets (98 / 69 / 9) before this was added, because the piece
    // canvas is re-rendered by the widget and alphaBox() then reports a
    // different silhouette. These digests make every answer auditable.
    const fp = (px, step) => {
      let h = 0;
      if (!px || !px.data) return null;
      for (let i = 0; i < px.data.length; i += step) h = (h * 31 + px.data[i]) | 0;
      return h;
    };
    const out = {
      offset: Math.max(0, Math.round(hit.x * scale + bgLeft - pieceLeft)),
      gap_x_in_bg: Math.round(hit.x * scale),
      bg_css_width: Math.round(bgCssW),
      piece_width_css: parts.slice ? Math.round(parts.slice.width) : Math.round(pieceW),
      handle_x: Math.round(parts.handle.x),
      method, confidence,
      outline_top3: method === 'outline' ? (window.__ubbOutlineTop3 || null) : null,
      // every candidate from every method, so a caller can audit a disagreement
      candidates: {
        ncc: nccRes ? { x: nccRes.x, score: Math.round(nccRes.score * 1000) / 1000, margin: Math.round(nccRes.margin * 1000) / 1000 } : null,
        outline: outlineRes ? { x: outlineRes.x, score: Math.round(outlineRes.score * 10) / 10, margin: Math.round(outlineRes.margin * 1000) / 1000 } : null,
        dark_run: d ? { x: d.x, contrast: Math.round(d.contrast * 10) / 10 } : null,
      },
      note: 'offset = how far the piece must travel (CSS px) to sit in the gap',
      // audit trail: which pixels produced this answer
      input: {
        bg_hash: fp(bg, 97), piece_hash: fp(piece, 97),
        bg_w: bg.w, bg_h: bg.h, piece_w: piece ? piece.w : null, piece_h: piece ? piece.h : null,
        pbox: pbox ? { x: pbox.x, y: pbox.y, w: pbox.w, h: pbox.h } : null,
        piece_w_used: pieceW, skip_from: skipFrom,
      },
    };
    window.__ubbCanvasGapDebug = { hit: hit.x, method, skipFrom, pieceW, input: out.input };
    return out;
  }

  function canvasMap() {
    const canvas = firstVisible('canvas[role="img"], canvas[class*="challenge"], canvas[class*="captcha"]');
    if (!canvas) return null;
    const r = canvas.getBoundingClientRect();
    if (r.width < 180 || r.height < 120) return null;
    const sub = submitButton();
    return {
      canvas: vbox(canvas),
      prompt: gridPrompt() || text(document.body).slice(0, 240),
      // hCaptcha swaps Skip -> Verify once a piece is placed; exposing the box
      // lets challenge.pick finish the round in one call.
      submit: sub ? vbox(sub.el) : null,
      submit_label: sub ? sub.label : null,
    };
  }

  function textMap() {
    const tc = textCaptcha();
    if (!tc) return null;
    const submit = submitButton();
    return {
      img_box: vbox(tc.img),
      input_box: vbox(tc.input),
      submit: submit ? vbox(submit.el) : null,
      label: submit ? submit.label : null,
    };
  }

  // Fallback: gap detection on a SCREENSHOT of the widget. The caller passes
  // the clip rect in CSS px; the PNG may be device-scaled, so every box is
  // multiplied by (img.width / clip.width).
  async function analyzeImage({ data_b64, clip, piece_box, track_box }) {
    const bin = atob(String(data_b64 || ''));
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
    const cv = new OffscreenCanvas(bmp.width, bmp.height);
    const ctx = cv.getContext('2d', { willReadFrequently: true });
    ctx.drawImage(bmp, 0, 0);
    const img = { w: bmp.width, h: bmp.height, data: ctx.getImageData(0, 0, bmp.width, bmp.height).data };
    const sx = img.w / Math.max(1, clip.width);
    const rel = (b) => (b ? { x: (b.x - clip.x) * sx, y: (b.y - clip.y) * sx, width: b.width * sx, height: b.height * sx } : null);
    const pb = rel(piece_box);
    const tb = rel(track_box);
    if (!pb || pb.width < 4 || pb.height < 4) return { ok: false, error: 'piece_box missing or too small' };
    // crop the piece out and the track band, then reuse the same algorithms
    const crop = (b) => {
      const x = Math.max(0, Math.round(b.x)), y = Math.max(0, Math.round(b.y));
      const w = Math.min(img.w - x, Math.round(b.width)), h = Math.min(img.h - y, Math.round(b.height));
      return { w, h, data: (() => {
        const out = new Uint8ClampedArray(w * h * 4);
        for (let yy = 0; yy < h; yy++) {
          const si = ((y + yy) * img.w + x) * 4;
          out.set(img.data.subarray(si, si + w * 4), yy * w * 4);
        }
        return out;
      })() };
    };
    const piece = crop(pb);
    const band = tb || { x: 0, y: pb.y, width: img.w, height: pb.height };
    const bg = crop(band);
    // SELF-MATCH GUARD (screenshot variant): the piece renders at the band's
    // left edge in the screenshot, so dark-run/template both match IT (x≈0-4)
    // instead of the hole → dx≈4 → 4px jiggle, never passes. The hole is
    // always to the right of the piece's current position: candidates inside
    // [0, pieceW+8] are the piece itself, not the hole.
    // the screenshot has NO alpha channel (alphaBox would return the whole
    // crop) — the piece's visible extent in the band ≈ the handle width, which
    // equals the piece width for slidercaptcha. Pass handle width via piece_box
    // extras instead: the solver sends handle_w. Fall back to 15% of the band.
    const selfEnd = Math.min(Math.round((p.handle_w || piece.w * 0.16) * sx) + 8, bg.w);
    function rejectSelf(h) {
      return h && h.x > selfEnd ? h : null;
    }
    let hit = rejectSelf(darkRun(bg, Math.min(piece.w, piece.h)));
    let method = 'screenshot-dark-run';
    let confidence = hit ? Math.min(0.85, 0.4 + hit.contrast / 70) : 0;
    if (!hit) {
      const ab = { x: 0, y: 0, w: piece.w, h: piece.h };
      const m = matchPiece(bg, piece, ab);
      if (m && m.score < 40 && rejectSelf(m)) { hit = { x: m.x }; method = 'screenshot-template'; confidence = Math.min(0.9, 0.5 + m.margin * 3); }
    }
    // The screenshot path exists because the canvas was unreadable — but the
    // piece crop itself carries the silhouette (alpha or luminance edge), so
    // the outline match still applies here and is the more reliable method
    // (yjsgll soak: dark-run 0.85 conf with a 30-60px miss, outline ±2px).
    if (piece.w > 20 && piece.h > 20) {
      const fake = { w: piece.w, h: piece.h, data: piece.data };
      const ab = alphaBox(fake) || { x: 0, y: 0, w: piece.w, h: piece.h };
      const ncc = matchPieceNCC(bg, fake, ab);
      if (ncc && ncc.score > 0.45 && ncc.margin > 0.05 && rejectSelf(ncc)) {
        hit = { x: ncc.x }; method = 'screenshot-ncc';
        confidence = Math.min(0.95, 0.5 + ncc.margin * 0.9 + Math.max(0, ncc.score - 0.5) * 0.4);
      } else {
        const o = outlineGap(bg, fake, ab, piece.w + 8);
        if (o && o.score > 8) {
          hit = { x: o.x + ab.x }; method = 'screenshot-outline'; confidence = Math.min(0.95, 0.55 + Math.min(o.margin, 0.4) * 2);
        }
      }
    }
    if (!hit) return { ok: false, error: 'no gap candidate', candidates: [] };
    return {
      ok: true,
      gap_x_in_band: Math.round(hit.x / sx),
      offset_css: Math.round(hit.x / sx),
      confidence,
      method,
      band_css_width: Math.round(band.width),
      piece_css_width: Math.round(pb.width),
    };
  }

  // ------------------------------------------------------------- verify

  function verifyState() {
    const tokens = tokenState();
    const local = detectLocal({ quick: false });
    return {
      url: String(location.href || ''),
      title: String(document.title || ''),
      tokens,
      token_present: tokens.some((t) => t.length > 0),
      findings: local.findings.map((f) => ({ vendor: f.vendor, kind: f.kind, role: f.role })),
      still_present: local.findings.some((f) => f.kind !== 'score' && f.kind !== 'interstitial'),
      challenge_present: local.findings.length > 0,
      epoch: DOC_EPOCH,
    };
  }

  function domDump({ max = 140 } = {}) {
    const out = [];
    for (const el of document.querySelectorAll('body *')) {
      if (out.length >= max) break;
      const r = el.getBoundingClientRect();
      if (r.width < 8 || r.height < 8) continue;
      const st = getComputedStyle(el);
      if (st.visibility === 'hidden' || st.display === 'none') continue;
      const cls = String(el.className || '').slice(0, 90);
      const aria = attr(el, 'aria-label');
      const role = attr(el, 'role');
      const drag = attr(el, 'draggable');
      const txt = (el.innerText || '').trim().slice(0, 40);
      if (!drag && !aria && !role && !cls && !txt) continue;
      out.push({ tag: el.tagName, cls, role, aria, drag, box: [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)], txt });
    }
    return out;
  }

  function slidercaptchaState() {
    const code = document.getElementById('sliderCaptchaDynamicCode');
    const txtEl = document.querySelector('.sliderText');
    const txt = text(txtEl || document.querySelector('#captcha-id, .slidercaptcha') || document.body).slice(0, 200);
    const success = /success|通过|成功|验证成功/i.test(txt) && !/try again|失败|retry|重试/i.test(txt);
    return { code_len: code ? String(code.value || '').length : 0, text: txt, success };
  }

  /**
   * CAS/JSU login-form state probe. Edge's credential autofill only materializes
   * in the DOM after the username field gains FOCUS — a plain JS value read
   * always reports empty and makes the agent think it must ask the user. This
   * op reports the form layout + current fill so the caller can (a) focus the
   * username field, (b) re-probe to confirm the fill landed, (c) only then
   * request a trusted click on the submit button. Passwords are never read
   * beyond the length bit — no plaintext ever leaves the page.
   */
  function casFormState() {
    const f = document.getElementById('casLoginForm') || document.querySelector('form[id*="cas"], form[action*="cas"]');
    if (!f) return { present: false };
    const box = (el) => {
      const r = el.getBoundingClientRect();
      return { x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2), w: Math.round(r.width), visible: r.width > 0 && r.height > 0 };
    };
    const u = f.querySelector('#username, input[name="username"]');
    const p = f.querySelector('#password, input[type="password"]');
    const submit = [...f.querySelectorAll('button, input[type="submit"]')]
      .find((b) => /sign in|登录|登陆|log\s?in/i.test(b.textContent || b.value || ''));
    return {
      present: true,
      username: u ? { ...box(u), filled_len: (u.value || '').length } : null,
      // NB: named pwdlen_audit — the result passes through redact(), which
      // kills any key matching /password|secret|credential|token/. Only a
      // length is exposed; the plaintext never enters this object.
      pwdlen_audit: p ? (p.value || '').length : -1,
      submit: submit ? box(submit) : null,
      error_text: (() => {
        const e = document.querySelector('#msg, #errorMsg, .auth_error, [class*="error"][class*="msg"]');
        return e ? text(e).slice(0, 120) : null;
      })(),
      captcha_visible: (() => {
        const c = document.getElementById('captcha-id');
        return c ? getComputedStyle(c).display !== 'none' : false;
      })(),
    };
  }

  function bgDump() {
    return { data: window.__ubbBgDump || null, canvases: (() => {
      try {
        return [...document.querySelectorAll('#captcha canvas, .slidercaptcha canvas')].map((c) => ({
          cls: c.className || '', w: c.width, h: c.height,
          cssW: Math.round(c.getBoundingClientRect().width),
        }));
      } catch { return []; }
    })() };
  }

  function pageState() {
    return {
      url: String(location.href || ''),
      title: String(document.title || ''),
      ready: document.readyState,
      text_sample: text(document.body).slice(0, 400),
      viewport: { w: innerWidth, h: innerHeight },
      dpr: window.devicePixelRatio || 1,
      scroll: { x: scrollX || 0, y: scrollY || 0 },
      epoch: DOC_EPOCH,
    };
  }

  // ------------------------------------------------------------- dispatch

  const api = {
    probe: detectLocal,
    gridMap,
    canvasMap,
    sliderMap,
    textMap,
    canvasGap,
    slidercaptchaState,
    casFormState,
    analyzeImage,
    verify: verifyState,
    pageState,
    bgDump,
    domDump,
  };

  window.__ubbChallenge = { DOC_EPOCH, probe: detectLocal };

  chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
    if (msg?.type !== 'UBB_CHALLENGE') return;
    try {
      const fn = api[msg.op];
      if (typeof fn !== 'function') throw new Error(`unknown challenge op ${msg.op}`);
      Promise.resolve(fn(msg.args || {})).then(
        (result) => sendResponse({ ok: true, result, frameId: sender.frameId, docEpoch: DOC_EPOCH }),
        (e) => sendResponse({ ok: false, error: String(e?.message || e), frameId: sender.frameId, docEpoch: DOC_EPOCH }),
      );
      return true;
    } catch (e) {
      sendResponse({ ok: false, error: String(e?.message || e), frameId: sender.frameId, docEpoch: DOC_EPOCH });
    }
  });
})();
