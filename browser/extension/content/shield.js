// CUA shield: the "controlled tab" indicator + input blocker.
// - pulsing highlight frame around the viewport
// - input-blocking veil (user clicks/keys are swallowed while armed)
// - status pill with a Stop (interrupt) button
// - pass-through gate for agent CDP input (CUA_PASS) and screenshot hygiene (CUA_VISIBLE)
// (visual language: pulsing indicator; gating: hide-for-tool-use pattern)

(() => {
  if (window.__cuaShield) return;
  window.__cuaShield = true;

  // window.__cuaShield only dedupes inside ONE JS world. The DOM is shared with
  // every other world of this document, and an extension reload tears the old
  // world down while leaving the nodes it appended in place — so the guard
  // above cannot keep the host unique. OWNER + pruneStaleHosts() make the
  // invariant "exactly one host owned by this extension id per document" a DOM
  // fact: a leaked host from a dead world carries the same OWNER but no live
  // listener, so it could never be told to pass (CUA_PASS) and its veil would
  // eat agent input forever.
  let OWNER = '';
  try {
    OWNER = chrome.runtime?.id || '';
  } catch {}

  let mode = 'off'; // off | armed | paused
  let blockMode = 'block'; // block | border | off (from popup setting)
  let visible = true;
  let helpActive = false; // human-assistance banner up → page belongs to the user
  let label = 'CUA agent control';
  let host = null;
  let veil = null;
  let pill = null;
  let pillLabel = null;
  let resumeBtn = null;
  let confirmOpen = false;
  let style = null;

  // An agent pass is a TRANSACTION, not a boolean. `let pass = false` was the
  // intermittent eater: one shared flag per frame means an earlier action's
  // delayed close (lib/shield-ctl.js caps its release at 800ms and forgets about
  // it) can flip the NEXT action's veil back to pointer-events:auto between the
  // readback and the CDP Input.* dispatch — the shield then swallows the agent's
  // own click while the tool reports {clicked:…}. Every open pass carries its own
  // token + its own deadline; the veil only blocks when NO token is alive.
  const passes = new Map(); // token → {expire, timer}
  const PASS_TTL_DEFAULT_MS = 45000; // caller-supplied ttl wins; bounds a wedged tool
  const SEP = '|';
  const AT = '@';

  const serialize = (list) => list.join(SEP);

  /** The ALIVE raw entries of "tok@expire|tok2@expire" — kept as written, because
   *  re-stamping a survivor without its expiry would make it immortal. */
  function aliveParts(el, attr) {
    const raw = el && el.getAttribute ? el.getAttribute(attr) : '';
    if (!raw) return [];
    const now = Date.now();
    const out = [];
    for (const part of raw.split(SEP)) {
      if (!part) continue;
      const i = part.lastIndexOf(AT);
      const expire = i > 0 ? Number(part.slice(i + 1)) : Infinity;
      if (Number.isFinite(expire) && expire <= now) continue;
      if (out.indexOf(part) < 0) out.push(part);
    }
    return out;
  }

  const tokenOf = (part) => {
    const i = part.lastIndexOf(AT);
    return i > 0 ? part.slice(0, i) : part;
  };

  /** The self-closing gate that does NOT depend on a timer (LIVE-RESULTS-3A 8-6:
   *  a token sat expired in the DOM for 287s because the only thing that ever
   *  re-read the stamp was the `setTimeout` inside the world that opened the pass
   *  — and a hidden/occluded tab throttles those, a torn-down world drops them
   *  with the world). Expiry is re-validated from the stamp at EVERY read of the
   *  ledger, so an expired token cannot keep this world claiming a pass. */
  function revalidatePasses() {
    const now = Date.now();
    for (const [token, p] of [...passes.entries()]) {
      if (p.expire > now) continue;
      clearTimeout(p.timer);
      passes.delete(token);
    }
    return passes;
  }

  /** Tokens the pass gate can see right now, for one host: this world's own
   *  (message-driven) plus whatever the DOM channel (chrome.scripting) stamped —
   *  that is how a host left behind by a torn-down world, or a force applied while
   *  our message was still in flight, still counts as open. */
  function hostOpen(h) {
    if (!h) return 0;
    return aliveParts(h, 'data-cua-pass').length + aliveParts(h, 'data-cua-forced').length;
  }

  /** EVERY host of this extension in this document — live or orphaned by a world
   *  that no longer exists. `data-cua-owner` missing means an older build of THIS
   *  shield, so it counts as ours. */
  function ourHosts() {
    return hostsInDoc().filter((h) => {
      const o = h.getAttribute('data-cua-owner');
      return o === null || o === OWNER;
    });
  }

  /**
   * THE reconciliation (the thing that was missing between the ledger and the
   * DOM, LIVE-RESULTS-3A 8-1 + 8-6). The ledger only ever caches WHICH tokens
   * exist — it is never evidence that a point is reachable — so every entry point
   * (world initialization, every apply(), every message) rewrites the DOM from the
   * stamps:
   *  ① expired tokens and tokens this world does not hold are removed FROM THE
   *     ATTRIBUTE (previously they were only filtered out of the verdict, so a
   *     stale entry sat in the DOM for 287s and stayed "alive" for any reader);
   *  ② a host with nothing alive under it is forced back to pointer-events:auto —
   *     fail-CLOSED for the user, and the only closer that does not depend on a
   *     setTimeout in a world that may be throttled or dead;
   *  ③ a host with an alive token under it is forced to 'none' — the mirror bug:
   *     a ledger claiming an open pass next to a veil still eating the agent's own
   *     events is what made 3A report {"clicked":…} for events nobody received.
   * `data-cua-forced` keeps every ALIVE entry regardless of this world's passes:
   *  that attribute is what the chrome.scripting channel stamps while our message
   *  is still in flight (fix7-shield), and this world cannot see the SW ledger.
   */
  function reconcile(born) {
    revalidatePasses();
    const mine = [];
    for (const [token, p] of passes) mine.push(`${token}${AT}${p.expire}`);
    const claimed = mine.map(tokenOf);
    for (const h of ourHosts()) {
      const mineHost = h === host;
      const forced = aliveParts(h, 'data-cua-forced');
      h.setAttribute('data-cua-forced', serialize(forced));
      // this world's ledger is the authority for its OWN host's attribute; on any
      // other host an alive entry survives (it may belong to a sibling world of a
      // different frame), except at init — a world that has never held a token can
      // assert that whatever else claims to be open on this document was stamped
      // by a world that no longer exists, and that is the fail-closed reading
      let stamp = mineHost ? mine : aliveParts(h, 'data-cua-pass');
      if (!mineHost && born) stamp = stamp.filter((p) => claimed.indexOf(tokenOf(p)) >= 0);
      h.setAttribute('data-cua-pass', serialize(stamp));
      const open = stamp.length + forced.length;
      const v = h.shadowRoot && h.shadowRoot.querySelector ? h.shadowRoot.querySelector('.veil') : null;
      const hMode = (mineHost ? mode : h.getAttribute('data-cua-mode') || mode) || 'off';
      const hBlock = (mineHost ? blockMode : h.getAttribute('data-cua-block') || blockMode) || 'block';
      const blocking = hMode === 'armed' && hBlock === 'block' && open === 0 && !(mineHost && helpActive);
      // the host box is never a hit target; blocking is the veil's job
      h.style.pointerEvents = 'none';
      if (v) v.style.pointerEvents = blocking ? 'auto' : 'none';
    }
  }

  function passing() {
    // read-only on purpose: this runs in every keydown/wheel listener, while the
    // attribute GC + veil rewrite belongs to apply()/init/messages (reconcile)
    revalidatePasses();
    return ourHosts().some((h) => hostOpen(h) > 0);
  }

  // A DOM-forced token whose issuer died (service worker torn down mid-action)
  // must not leave the veil open forever: this world re-applies the rule at the
  // earliest stamped expiry, so the shield closes itself without anybody's help.
  let sweepTimer = null;
  let sweepAt = 0;
  function scheduleSweep() {
    if (!host) return;
    const raw = host.getAttribute('data-cua-forced') || '';
    let soonest = Infinity;
    for (const part of raw.split(SEP)) {
      const i = part.lastIndexOf(AT);
      if (i <= 0) continue;
      const e = Number(part.slice(i + 1));
      if (Number.isFinite(e) && e > Date.now() && e < soonest) soonest = e;
    }
    if (soonest === sweepAt) return;
    sweepAt = soonest;
    if (sweepTimer) {
      clearTimeout(sweepTimer);
      sweepTimer = null;
    }
    if (!Number.isFinite(soonest)) return;
    sweepTimer = setTimeout(() => {
      sweepTimer = null;
      sweepAt = 0;
      apply();
    }, soonest - Date.now());
  }

  function addPass(token, ttlMs) {
    const ttl = Math.min(Math.max(Number(ttlMs) || PASS_TTL_DEFAULT_MS, 1000), 120000);
    const expire = Date.now() + ttl;
    const prev = passes.get(token);
    if (prev) clearTimeout(prev.timer);
    passes.set(token, { expire, timer: setTimeout(() => { passes.delete(token); apply(); }, ttl) });
    apply();
  }

  function dropPass(token) {
    const prev = passes.get(token);
    if (prev) clearTimeout(prev.timer);
    passes.delete(token);
    apply();
  }
  const CSS = `
    /* The edge treatment: a bright white rim that fades inward into a
       coloured wash, breathing on a controlled tab. The earlier version used
       percentage gradient stops, so an 8% stop was 154px wide on a 1920 screen
       but only 86px tall on a 1080 one — the sides read much thicker than the
       top and bottom. Every stop below is in px, so the falloff is identical
       on all four sides at any viewport size. */
    @keyframes cua-edge-breathe {
      0%,100% { opacity: .55; }
      50%     { opacity: 1; }
    }
    /* the colour itself drifts around the frame */
    @keyframes cua-edge-hue {
      to { --cua-angle: 360deg; }
    }
    * { all: initial; box-sizing: border-box; }
    /* "all: initial" above resets the <style> element's UA display:none to
       inline — without this rule the whole stylesheet is PAINTED as text into
       the document flow (seen verbatim in a print_pdf render, and visible to
       the user at the page end) */
    style { display: none !important; }
    /* the shield is chrome, never content: nothing of it may reach a printed /
       PDF render, whatever the visibility handshake is doing at that moment */
    @media print { .host, .pill, .veil, .frame, .confirm, .helpcard { display: none !important; } }
    .host { position: fixed; inset: 0; z-index: 2147483647; pointer-events: none; font-family: system-ui, -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif; }

    /* The edge glow: a bright white rim that fades inward into colour.
       Implemented as an INSET box-shadow stack, not a gradient mask — a
       mask-based approach made the falloff depend on the viewport aspect (the
       percentage stops) or collapse at the corners (px stops + mask-composite).
       Inset shadows are measured from the box edge in px on every side, so the
       four edges and the corners all fade identically at any size.
       Three shadows: a crisp white rim, a wide white haze, then the colour. */
    .frame {
      position: absolute; inset: 0; pointer-events: none;
      border-radius: 18px;
      background: transparent;
      /* A positive spread pushes an inset shadow OUTSIDE the box, which is
         why the first attempt showed a 2px line and nothing else. spread 0
         keeps the whole blur inside, so the wash really does reach inward. */
      box-shadow:
        /* 1. the white rim, 2px, hard */
        inset 0 0 0 2px rgba(255,255,255,.98),
        /* 2. white haze, 30px of blur from the edge inward */
        inset 0 0 30px 0 rgba(255,255,255,.55),
        /* 3. colour wash, 90px of blur from the edge inward */
        inset 0 0 90px 0 var(--cua-tint, rgba(139,92,246,.95));
      opacity: .9;
    }
    /* the colour cycles through the ring while it breathes */
    @keyframes cua-tint-cycle {
      0%   { --cua-tint: rgba(125,211,252,.92); }
      25%  { --cua-tint: rgba(167,139,250,.92); }
      50%  { --cua-tint: rgba(244,114,182,.92); }
      75%  { --cua-tint: rgba(251,191,36,.92); }
      100% { --cua-tint: rgba(125,211,252,.92); }
    }
    /* breathe + drift ONLY on a controlled tab */
    .host[data-cua-mode="armed"] .frame {
      animation:
        cua-edge-breathe 2.8s ease-in-out infinite,
        cua-tint-cycle 9s linear infinite;
    }
    @property --cua-tint {
      syntax: '<color>';
      initial-value: rgba(139,92,246,.9);
      inherits: false;
    }

    /* The veil darkens the page so the agent's control reads as a mode, and
       carries its own soft edge glow so the tint and the frame blend. */
    .veil {
      position: absolute; inset: 0; pointer-events: auto; cursor: not-allowed;
      border-radius: 18px;
      background: rgba(8,8,10,.32);
      box-shadow: inset 0 0 60px rgba(255,255,255,.05);
    }
    .pill {
      position: absolute; top: 8px; left: 50%; transform: translateX(-50%);
      display: flex; align-items: center; gap: 8px;
      background: rgba(0,0,0,.88); color: #fff;
      border: 1px solid rgba(255,255,255,.30); border-radius: 999px;
      padding: 4px 5px 4px 12px; font-size: 11.5px; line-height: 1;
      backdrop-filter: blur(8px);
      pointer-events: none; user-select: none;
      /* keep the label on one line: a wrapped pill reads as broken chrome.
         width:max-content stops the absolutely-positioned pill from being
         sized by its containing block, which is what forced the wrap. */
      white-space: nowrap;
      width: max-content;
    }
    /* the universal "all: initial" reset above sends every descendant back to
       the initial black text colour, so the label span painted black-on-black
       inside the pill. Re-assert white for the pill and everything in it (the
       two buttons set their own colour below and keep it). */
    .pill, .pill span { color: #fff; }
    .dot { width: 6px; height: 6px; border-radius: 50%; background: #fff; animation: cua-blink 1.2s infinite; }
    @keyframes cua-blink { 50% { opacity: .25; } }
    button.stop {
      all: unset; pointer-events: auto; cursor: pointer;
      background: #fff; color: #000; border-radius: 999px;
      padding: 4px 11px; font-size: 11px; font-weight: 600;
      white-space: nowrap;
    }
    button.stop:hover { background: #d4d4d4; }
    button.resume {
      all: unset; pointer-events: auto; cursor: pointer;
      background: #000; color: #fff; border: 1px solid rgba(255,255,255,.5); border-radius: 999px;
      padding: 4px 11px; font-size: 11px; font-weight: 600;
      white-space: nowrap;
    }
    button.resume:hover { background: #1a1a1a; }
    .confirm {
      position: absolute; inset: 0; pointer-events: auto; background: rgba(0,0,0,.5);
      display: flex; align-items: center; justify-content: center;
    }
    .card {
      background: #0a0a0a; color: #fff; border: 1px solid rgba(255,255,255,.22); border-radius: 12px; padding: 18px 22px;
      max-width: 420px; box-shadow: 0 24px 60px rgba(0,0,0,.5); font-size: 13.5px;
    }
    .card h3 { margin: 0 0 6px; font-size: 15px; color: #fff; }
    .card p { margin: 0 0 14px; color: #a3a3a3; }
    .card .btns { display: flex; gap: 8px; justify-content: flex-end; align-items: center; }
    .card button { all: unset; cursor: pointer; border-radius: 8px; padding: 7px 16px; font-size: 12.5px; font-weight: 600; }
    .card .allow { background: #fff; color: #000; }
    .card .allow:hover { background: #e5e5e5; }
    .card .deny { background: transparent; color: #fff; border: 1px solid rgba(255,255,255,.35); }
    .card .deny:hover { background: rgba(255,255,255,.12); }
    .count { color: #737373; font-size: 11.5px; margin-left: 10px; }
    .helpcard {
      position: absolute; top: 52px; left: 50%; transform: translateX(-50%);
      background: #0a0a0a; color: #fff; border: 1px solid rgba(255,255,255,.22); border-radius: 12px;
      padding: 12px 16px; max-width: 460px; pointer-events: auto;
      box-shadow: 0 18px 48px rgba(0,0,0,.5); font-size: 13px;
    }
    .helpcard .t { font-weight: 600; margin-bottom: 3px; font-size: 13px; color: #fff; }
    .helpcard .s { color: #a3a3a3; font-size: 11.5px; margin-bottom: 9px; }
    .helpcard .btns { display: flex; gap: 7px; justify-content: flex-end; align-items: center; }
    .helpcard button { all: unset; cursor: pointer; border-radius: 8px; padding: 5px 12px; font-size: 11.5px; font-weight: 600; }
    .helpcard .done { background: #fff; color: #000; }
    .helpcard .done:hover { background: #e5e5e5; }
    .helpcard .cancel { background: transparent; color: #fff; border: 1px solid rgba(255,255,255,.35); }
    .helpcard .cancel:hover { background: rgba(255,255,255,.12); }
  `;

  function hostsInDoc() {
    return Array.prototype.slice.call(document.querySelectorAll('.cua-host-root'));
  }

  /** Same-owner hosts are ours (live or orphaned by a torn-down world) — reap
   *  all of them except the one we keep, so the document ends with exactly one.
   *  A host with no owner attribute was built by an older build of this shield,
   *  so it counts as ours too. */
  function pruneStaleHosts(keep) {
    for (const h of hostsInDoc()) {
      if (h === keep) continue;
      const o = h.getAttribute('data-cua-owner');
      if (o !== null && o !== OWNER) continue; // somebody else's overlay — not ours to remove
      h.remove();
    }
  }

  function build() {
    pruneStaleHosts(null);
    host = document.createElement('div');
    host.className = 'cua-host-root';
    host.setAttribute('data-cua-owner', OWNER);
    const root = host.attachShadow({ mode: 'open' });
    style = document.createElement('style');
    style.textContent = CSS;
    root.appendChild(style);

    const hostEl = document.createElement('div');
    hostEl.className = 'host';
    root.appendChild(hostEl);

    const frame = document.createElement('div');
    frame.className = 'frame';
    hostEl.appendChild(frame);

    veil = document.createElement('div');
    veil.className = 'veil';
    hostEl.appendChild(veil);

    pill = document.createElement('div');
    pill.className = 'pill';
    const dot = document.createElement('span');
    dot.className = 'dot';
    pillLabel = document.createElement('span');
    pillLabel.textContent = label;
    const stop = document.createElement('button');
    stop.className = 'stop';
    stop.textContent = '■ 停止代理';
    stop.addEventListener('click', (e) => {
      if (!e.isTrusted) return;
      try {
        chrome.runtime.sendMessage({ type: 'CUA_INTERRUPT' });
      } catch {}
      // instant local relief: stop blocking until a human resumes
      mode = 'paused';
      apply();
    });
    resumeBtn = document.createElement('button');
    resumeBtn.className = 'resume';
    resumeBtn.textContent = '▶ 继续代理';
    resumeBtn.style.display = 'none';
    resumeBtn.addEventListener('click', (e) => {
      if (!e.isTrusted) return;
      try {
        chrome.runtime.sendMessage({ type: 'CUA_RESUME' });
      } catch {}
      mode = 'armed';
      apply();
    });
    pill.append(dot, pillLabel, resumeBtn, stop);
    hostEl.appendChild(pill);

    host.style.display = 'none';
    (document.documentElement || document.body).appendChild(host);
    ensureDeliveryRing(); // documentElement may have been replaced since document_start
    apply();
  }

  function apply() {
    // state stamps FIRST, so the reconciliation below and every outside reader
    // (page JS, the chrome.scripting probe, the restore after print_pdf) compute
    // the verdict from a picture of this exact moment — with no live world needed
    if (host) {
      host.setAttribute('data-cua-mode', mode);
      host.setAttribute('data-cua-block', blockMode);
      host.setAttribute('data-cua-visible', visible ? '1' : '0');
    }
    // THE reconciliation — GC of expired/unclaimed stamps and a fresh veil write
    // for EVERY host of this extension, this world's own host included. It runs
    // before the "do I even have a host" return, because that early return is
    // exactly what left an orphan host open over the user's page (3A 8-6) and a
    // blocking host under a gate that reported "confirmed" (3A 8-1).
    reconcile();
    if (!host) return;
    if (host.isConnected) pruneStaleHosts(host);
    // A takeover confirmation must be visible even before the tab is armed:
    // the old gate hid the entire host in mode "off", so the user could be
    // asked for consent by a card they could not see.
    const show = visible && (confirmOpen || mode === 'armed' || mode === 'paused');
    host.style.display = show ? '' : 'none';
    host.setAttribute('data-cua-confirm', confirmOpen ? '1' : '0');
    if (pillLabel) {
      pillLabel.textContent =
        mode === 'paused'
          ? '⏸ 已接管'
          : `${label} · 点击已屏蔽`;
    }
    if (resumeBtn) resumeBtn.style.display = mode === 'paused' ? '' : 'none';
    const blocking = mode === 'armed' && blockMode === 'block' && hostOpen(host) === 0 && !helpActive;
    if (veil) {
      // pointer-events toggling is invisible; the tint NEVER changes with
      // pass state, so the shield does not flicker during agent operations —
      // the gradient itself lives in the stylesheet and is never touched here
      veil.style.pointerEvents = blocking ? 'auto' : 'none';
    }
    scheduleSweep();
  }

  function stateOf(at) {
    const all = hostsInDoc();
    const ours = ourHosts();
    const dom = [];
    let blockingHosts = 0;
    for (const h of ours) {
      for (const p of aliveParts(h, 'data-cua-pass')) if (dom.indexOf(p) < 0) dom.push(p);
      for (const p of aliveParts(h, 'data-cua-forced')) if (dom.indexOf(p) < 0) dom.push(p);
      const v = h.shadowRoot && h.shadowRoot.querySelector ? h.shadowRoot.querySelector('.veil') : null;
      if (v && v.style.pointerEvents !== 'none') blockingHosts++;
    }
    const ring = deliveryRecords();
    const delEl = deliveryEl();
    const out = {
      mode,
      blockMode,
      visible,
      // DOM truth, never `passes.size`: a world whose host is gone (or whose
      // expiry timer was throttled) must not be able to answer "open"
      pass: passing(),
      pass_tokens: [...passes.keys()],
      dom_pass_tokens: dom.map(tokenOf),
      blocking_hosts: blockingHosts,
      host_in_dom: !!(host && host.isConnected),
      owner: OWNER,
      host_count: all.length,
      our_hosts: ours.length,
      veil_pointer_events: veil ? veil.style.pointerEvents : null,
      // fix17: what this document has ACTUALLY received, and how much of it was
      // this extension's own overlay eating it (see the delivery ring above)
      delivery: ring,
      delivery_seq: deliveryTop(ring),
      delivery_recorder: !!(delEl && delEl.getAttribute && delEl.getAttribute(DELIVERY_ATTR) !== null),
    };
    if (Array.isArray(at) && typeof at[0] === 'number' && typeof at[1] === 'number' && document.elementFromPoint) {
      const el = document.elementFromPoint(at[0], at[1]);
      out.hit = el
        ? {
            tag: String(el.tagName || '').toLowerCase(),
            is_shield_host: !!(el.classList && el.classList.contains && el.classList.contains('cua-host-root')),
          }
        : null;
    }
    return out;
  }

  function blockingKeys() {
    // during human assistance the page belongs to the user: typing, scrolling
    // and clicking must all reach the page (login forms, captchas, payment)
    return !helpActive && mode === 'armed' && blockMode === 'block' && !passing() && visible;
  }

  // ───────────────────────── fix17 DELIVERY FORENSICS (read-only) ───────────────
  // WHY THIS EXISTS: CDP Input.dispatchMouseEvent ACKS BEFORE IT DELIVERS. On an
  // agent tab whose document is visibilityState:"hidden" (an Edge window sitting
  // behind another window is hidden even when window.state is "maximized" and
  // focused:true, and tab.select cannot make it visible), the three commands ack
  // in milliseconds with zero errors and the events reach the document about FIVE
  // SECONDS later. The gate used to close on the ack, so by delivery time the veil
  // was pointer-events:auto again and the shield ate its OWN agent's input while
  // the tool returned ok:true (LIVE-RESULTS-6: 20/20, and 2/2 on the main line's
  // own self-test). An ack cannot prove delivery; only a listener INSIDE the
  // document can. So this is that listener: a capture-phase, read-only record of
  // every trusted input event the document actually received, with one field that
  // decides everything - shield_hit, "did this land on our own overlay instead of
  // on the page". Capture phase fires even when the veil swallows the event, which
  // is exactly why the two cases are separable at all.
  // It NEVER calls preventDefault, NEVER stops propagation, NEVER changes a single
  // page behaviour: the only thing it writes is one attribute on documentElement.
  const DELIVERY_ATTR = 'data-cua-delivery';
  const DELIVERY_MAX = 12; // ring of recent records kept in the shared DOM
  const DELIVERY_TTL_MS = 60000; // older than this it is not evidence any more
  const DELIVERY_SEP = ';';
  const DELIVERY_FIELD = '~';
  const DELIVERY_TYPES = ['mousedown', 'mouseup', 'click', 'keydown', 'keyup', 'beforeinput', 'input', 'wheel', 'touchstart'];

  function deliveryEl() {
    return document.documentElement || document.body || null;
  }

  // the SAME read-then-GC discipline reconcile() uses: the attribute is the ledger,
  // expiry is re-applied at EVERY read, and the writer keeps only the last
  // DELIVERY_MAX entries, so a ring left behind by a torn-down world ages out of
  // the evidence instead of sitting there forever
  function deliveryRecords(src) {
    const el = deliveryEl();
    const raw = typeof src === 'string' ? src : el && el.getAttribute ? el.getAttribute(DELIVERY_ATTR) : null;
    const out = [];
    if (!raw) return out;
    const now = Date.now();
    const parts = String(raw).split(DELIVERY_SEP);
    for (const part of parts) {
      if (!part) continue;
      const f = part.split(DELIVERY_FIELD);
      if (f.length < 6) continue;
      const seq = Number(f[0]);
      const t = Number(f[4]);
      if (!Number.isFinite(seq) || !Number.isFinite(t)) continue;
      if (t > now + 5000 || now - t > DELIVERY_TTL_MS) continue;
      const x = Number(f[2]);
      const y = Number(f[3]);
      out.push({
        seq,
        type: f[1],
        x: Number.isFinite(x) ? x : null,
        y: Number.isFinite(y) ? y : null,
        t,
        shield_hit: f[5] === '1',
        // fix20 identity fields (fields 6..10): WHICH element the page received
        // this on, and its own box - the audit question "is the thing I hit the
        // thing I wanted" is unreadable from a tag name alone (LIVE-RESULTS-6).
        id: f.length > 6 && f[6] !== '-' ? f[6] : null,
        cls: f.length > 7 && f[7] !== '-' ? f[7] : null,
        aria: f.length > 8 && f[8] !== '-' ? f[8] : null,
        text: f.length > 9 && f[9] !== '-' ? f[9] : null,
        box: parseDeliveryBox(f[10]),
      });
    }
    out.sort((a, b) => a.seq - b.seq);
    return out;
  }

  function deliveryTop(recs) {
    let m = 0;
    for (const r of recs) if (r.seq > m) m = r.seq;
    return m;
  }

  function parseDeliveryBox(v) {
    if (typeof v !== 'string') return null;
    const bs = v.split(',');
    if (bs.length !== 4) return null;
    const n = bs.map(Number);
    return n.every((k) => Number.isFinite(k)) ? n : null;
  }

  const deliveryLine = (r) =>
    [
      r.seq,
      r.type,
      r.x === null ? '-' : r.x,
      r.y === null ? '-' : r.y,
      r.t,
      r.shield_hit ? '1' : '0',
      r.id || '-',
      r.cls || '-',
      r.aria || '-',
      r.text || '-',
      r.box && r.box.length === 4 ? r.box.join(',') : '-',
    ].join(DELIVERY_FIELD);

  /** Did this event land on THIS EXTENSION'S overlay (veil / pill / stop button)
   *  rather than on the page? composedPath is the authoritative answer: it is
   *  complete at capture phase even for the event the veil is about to swallow,
   *  and every shadow node of the shield is inside it. */
  function shieldHit(e) {
    let path = null;
    try {
      if (typeof e.composedPath === 'function') path = e.composedPath();
    } catch {}
    if (path && path.length) {
      for (const n of path) {
        if (!n) continue;
        if (n === host) return true;
        if (n.classList && n.classList.contains && n.classList.contains('cua-host-root')) return true;
      }
      return false;
    }
    const t = e.target;
    return !!(t && t.classList && t.classList.contains && t.classList.contains('cua-host-root'));
  }

  /** fix20: the identity of the element the page ACTUALLY received this event on
   *  (retargeting already happened by capture time), plus its viewport box. The
   *  box is floored/ceiled OUTWARD so the point-in-box criterion can only ever be
   *  more generous by subpixels, never falsely strict. Values are flattened of the
   *  attribute grammar's delimiters; everything is best-effort behind a guard - a
   *  fake or exotic target must not cost the record itself. */
  function deliveryIdentity(e) {
    const out = { id: null, cls: null, aria: null, text: null, box: null };
    try {
      const flat = (s, n) => String(s).replace(/[~;\r\n]/g, ' ').replace(/\s+/g, ' ').trim().slice(0, n) || null;
      let n = null;
      try {
        if (typeof e.composedPath === 'function') {
          const p = e.composedPath();
          if (p && p.length && p[0] && p[0].nodeType === 1) n = p[0];
        }
      } catch {}
      if (!n && e.target && e.target.nodeType === 1) n = e.target;
      if (!n || typeof n.getAttribute !== 'function') return out;
      out.id = flat(n.getAttribute('id') || n.id || '', 32);
      const cs = String(typeof n.className === 'string' ? n.className : n.getAttribute('class') || '').split(/\s+/);
      for (const c of cs) {
        if (c && !c.startsWith('cua-')) {
          out.cls = flat(c, 24);
          break;
        }
      }
      out.aria = flat(n.getAttribute('aria-label') || n.getAttribute('name') || n.getAttribute('data-testid') || '', 24);
      out.text = flat(n.textContent || n.value || '', 24);
      if (typeof n.getBoundingClientRect === 'function') {
        const r = n.getBoundingClientRect();
        if (Number.isFinite(r.left) && Number.isFinite(r.top) && Number.isFinite(r.right) && Number.isFinite(r.bottom)) {
          out.box = [Math.floor(r.left), Math.floor(r.top), Math.ceil(r.right), Math.ceil(r.bottom)];
        }
      }
    } catch {}
    return out;
  }

  function recordDelivery(e) {
    try {
      // ONLY trusted events: page JS can dispatch a MouseEvent that looks exactly
      // like a delivered one, and false evidence would be worse than none.
      if (!e || e.isTrusted !== true) return;
      const el = deliveryEl();
      if (!el || !el.setAttribute || !el.getAttribute) return;
      if (e.__cuaDelivery17 === true) return; // registered at two nodes: record once
      try {
        e.__cuaDelivery17 = true;
      } catch {}
      const type = String(e.type || '');
      if (!type) return;
      // A key event that our OWN capture blocker (registered right below this
      // one) is about to eat is the same failure as a pointer event eaten by the
      // veil - the page will never see it, so it is a shield hit too.
      const keyish = type === 'keydown' || type === 'keyup' || type === 'keypress';
      const hit = keyish ? blockingKeys() : shieldHit(e);
      const recs = deliveryRecords();
      const seq = deliveryTop(recs);
      const x = Number.isFinite(e.clientX) ? Math.round(e.clientX) : null;
      const y = Number.isFinite(e.clientY) ? Math.round(e.clientY) : null;
      const now = Date.now();
      const last = recs.length ? recs[recs.length - 1] : null;
      // a SECOND live world of the same document (double injection) would note the
      // very same event twice; keep the ring honest by dropping the duplicate
      if (last && last.type === type && last.x === x && last.y === y && now - last.t < 3) return;
      const ring = recs.slice(-(DELIVERY_MAX - 1));
      const who = deliveryIdentity(e);
      ring.push({ seq: seq + 1, type, x, y, t: now, shield_hit: hit, id: who.id, cls: who.cls, aria: who.aria, text: who.text, box: who.box });
      el.setAttribute(DELIVERY_ATTR, ring.map(deliveryLine).join(DELIVERY_SEP));
    } catch {}
  }

  /** The ring's EXISTENCE is what tells the controller "this document can answer
   *  for delivery". Stamped at document_start, before anything is built: a page
   *  served by an older content-script world has no attribute at all, and the
   *  gate then degrades to its old ack-only behaviour instead of failing every
   *  action on a tab that has not been reloaded since the extension was. */
  function ensureDeliveryRing() {
    try {
      const el = deliveryEl();
      if (el && el.getAttribute && el.getAttribute(DELIVERY_ATTR) === null) el.setAttribute(DELIVERY_ATTR, '');
    } catch {}
  }

  // FIRST listeners of this document, registered before the blocking ones below so
  // our own stopImmediatePropagation can never hide an event from the record.
  ensureDeliveryRing();
  for (const type of DELIVERY_TYPES) {
    window.addEventListener(type, recordDelivery, true);
  }

  // Keyboard/context-menu blocking at capture phase. CDP-injected key events
  // are indistinguishable from real ones in-page, so the caller opens a pass
  // TRANSACTION (CUA_PASS with its own token) for the duration of each input
  // burst — exactly like the hide-for-tool-use gate. The listeners below read
  // the same token ledger the veil reads, so a force applied through the DOM by
  // chrome.scripting opens the keyboard path too, and no other transaction's
  // release can close ours.
  for (const type of ['keydown', 'keyup', 'keypress']) {
    window.addEventListener(
      type,
      (e) => {
        if (blockingKeys()) {
          e.preventDefault();
          e.stopImmediatePropagation();
        }
      },
      true,
    );
  }
  window.addEventListener(
    'contextmenu',
    (e) => {
      if (blockingKeys()) e.preventDefault();
    },
    true,
  );
  // wheel/touch DEFAULT ACTIONS (page scroll, pinch zoom) are not stopped by
  // the veil's pointer-events — they must be prevented explicitly (passive off)
  for (const type of ['wheel', 'touchstart', 'touchmove']) {
    window.addEventListener(
      type,
      (e) => {
        if (blockingKeys()) {
          e.preventDefault();
          e.stopImmediatePropagation();
        }
      },
      { capture: true, passive: false },
    );
  }
  // middle-click autoscroll / drag-select also die on the veil, but make sure
  window.addEventListener(
    'mousedown',
    (e) => {
      if (blockingKeys() && e.button !== 0) e.preventDefault();
    },
    true,
  );

  function showConfirm({ reqId, title, timeoutMs }) {
    if (!host) build();
    const root = host.shadowRoot;
    const overlay = document.createElement('div');
    overlay.className = 'confirm';
    const card = document.createElement('div');
    card.className = 'card';
    const h = document.createElement('h3');
    h.textContent = title || 'CUA 请求确认';
    const p = document.createElement('p');
    p.textContent = '如果你同意，此标签页将并入 CUA 分组并接受代理控制；不同意则保持现状。';
    const btns = document.createElement('div');
    btns.className = 'btns';
    const deny = document.createElement('button');
    deny.className = 'deny';
    deny.textContent = '拒绝';
    const allow = document.createElement('button');
    allow.className = 'allow';
    allow.textContent = '允许';
    const count = document.createElement('span');
    count.className = 'count';
    btns.append(count, deny, allow);
    card.append(h, p, btns);
    overlay.appendChild(card);
    root.querySelector('.host').appendChild(overlay);
    confirmOpen = true;
    apply();

    const t0 = Date.now();
    const tick = setInterval(() => {
      const left = Math.max(0, Math.ceil((timeoutMs - (Date.now() - t0)) / 1000));
      count.textContent = `${left}s`;
      if (left <= 0) finish(false);
    }, 500);
    function finish(ok) {
      clearInterval(tick);
      overlay.remove();
      confirmOpen = false;
      apply();
      try {
        chrome.runtime.sendMessage({ type: 'CUA_CONFIRM_RESULT', reqId, ok });
      } catch {}
    }
    // Open shadow DOM is not a trust boundary: page script can click these
    // buttons. Only a trusted (real/CDP-injected) event may answer consent.
    deny.addEventListener('click', (e) => {
      if (e.isTrusted) finish(false);
    });
    allow.addEventListener('click', (e) => {
      if (e.isTrusted) finish(true);
    });
    overlay.addEventListener('click', (e) => {
      if (e.isTrusted && e.target === overlay) finish(false);
    });
  }

  // ---- human assistance banner (non-blocking; page belongs to the user) -----

  let currentHelp = null; // { reqId, card, tick }

  function closeHelp() {
    if (!currentHelp) return;
    clearInterval(currentHelp.tick);
    currentHelp.card.remove();
    currentHelp = null;
    if (helpActive) {
      helpActive = false;
      apply();
    }
  }

  function showHelp({ reqId, title, timeoutMs }) {
    if (!host) build();
    closeHelp(); // supersede any earlier banner
    const root = host.shadowRoot;
    const card = document.createElement('div');
    card.className = 'helpcard';
    const t = document.createElement('div');
    t.className = 't';
    t.textContent = title || 'CUA 请求人工协助';
    const s = document.createElement('div');
    s.className = 's';
    s.textContent = '请在页面上完成操作；本横幅不拦截你的输入。完成后点"完成"，或等代理自动判定。';
    const btns = document.createElement('div');
    btns.className = 'btns';
    const count = document.createElement('span');
    count.className = 's';
    const cancel = document.createElement('button');
    cancel.className = 'cancel';
    cancel.textContent = '取消';
    const done = document.createElement('button');
    done.className = 'done';
    done.textContent = '✓ 完成了';
    btns.append(count, cancel, done);
    card.append(t, s, btns);
    root.querySelector('.host').appendChild(card);

    helpActive = true;
    apply();
    currentHelp = { reqId, card, tick: null };
    const t0 = Date.now();
    const tick = setInterval(() => {
      const left = Math.max(0, Math.ceil((timeoutMs - (Date.now() - t0)) / 1000));
      count.textContent = `${left}s`;
      if (left <= 0) finish(false);
    }, 500);
    currentHelp.tick = tick;
    function finish(ok) {
      try { chrome.runtime.sendMessage({ type: 'CUA_HELP_RESULT', reqId, ok }); } catch {}
      closeHelp();
    }
    done.addEventListener('click', () => finish(true));
    cancel.addEventListener('click', () => finish(false));
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (!msg || typeof msg !== 'object') return;
    if (msg.type === 'CUA_SHIELD') {
      mode = msg.mode || 'off';
      if (msg.label) label = msg.label;
      if (msg.blockMode) blockMode = msg.blockMode;
      if (!host && mode !== 'off') build();
      apply();
      sendResponse({ ok: true, state: stateOf(msg.at) });
    } else if (msg.type === 'CUA_SHIELD_QUERY') {
      if (host) pruneStaleHosts(host); // any probe is a chance to restore the 1-host invariant
      // a probe is ALSO a reconciliation point: an orphan host left by a dead
      // world gets GC'd and forced back to blocking here, for free, on a message
      // the controller had to send anyway
      apply();
      sendResponse({ ok: true, state: stateOf(msg.at) });
    } else if (msg.type === 'CUA_PASS') {
      // op:'open' with a token = one agent input transaction; op:'close' with
      // the SAME token releases only that one. `on` is the caller's aggregate
      // (content/frameblock.js still reads the boolean, and a bare {on:false}
      // without a token stays the legacy "release everything" shape).
      const token = typeof msg.token === 'string' && msg.token ? msg.token : 'legacy';
      if (msg.op === 'close' || msg.on === false) dropPass(token);
      else addPass(token, msg.ttlMs);
      // the state rides along so the caller can confirm the gate against this
      // document's real veil, not just against a boolean: `pass` below is computed
      // from the stamped attributes (DOM truth), never from `passes.size`, and
      // `hit`/`blocking_hosts` answer the exact point the event will hit-test
      sendResponse({ ok: true, pass: passing(), token, state: stateOf(msg.at) });
    } else if (msg.type === 'CUA_DELIVERY_QUERY') {
      // fix17 read-only evidence channel (the message world's copy of it; the
      // chrome.scripting channel reads the same attribute without needing this
      // listener at all). Answers for a document whose ring exists even when the
      // gate has no scripting channel — and never writes anything.
      const ring = deliveryRecords();
      const el = deliveryEl();
      sendResponse({
        ok: true,
        recorder: !!(el && el.getAttribute && el.getAttribute(DELIVERY_ATTR) !== null),
        records: ring,
        seq: deliveryTop(ring),
        veil_pointer_events: veil ? veil.style.pointerEvents : null,
      });
    } else if (msg.type === 'CUA_VISIBLE') {
      visible = !!msg.on;
      apply();
      sendResponse({ ok: true });
    } else if (msg.type === 'CUA_CONFIRM') {
      showConfirm(msg);
      sendResponse({ ok: true });
    } else if (msg.type === 'CUA_HELP') {
      showHelp(msg);
      sendResponse({ ok: true });
    } else if (msg.type === 'CUA_HELP_CLOSE') {
      if (currentHelp && (!msg.reqId || currentHelp.reqId === msg.reqId)) closeHelp();
      sendResponse({ ok: true });
    }
  });

  // Fail-CLOSED at world initialization, before anything is built: this world's
  // `passes` is empty by definition here, so every host of this extension that is
  // still standing in the shared DOM — including the one an extension reload left
  // behind with a live token stamped on it and a veil parked at 'none' — gets its
  // stamps GC'd and its veil written back to blocking. That orphan's listeners
  // died with its world, so no message will ever reach it: this is the only place
  // a newly born world can close a door its predecessor left open.
  reconcile(true);

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', build, { once: true });
  } else {
    build();
  }
})();
