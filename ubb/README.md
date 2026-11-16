# UBB — Universal Browser Bridge

A browser extension that lets any AI agent (CLI tools, scripts, coding assistants) control a browser through a local daemon over an authenticated WebSocket. **Nameless calls go to the user's real, logged-in browser by default.** If that browser is not running, the daemon starts its **normal profile** minimized (no `--user-data-dir`, no `--load-extension`) and waits for the extension to reconnect, so the existing cookies and logins are the ones the agent uses. Isolated-profile **agent workspaces** remain available explicitly through `browser.launch` (or by opting back in with `auto_workspace:true`).

## Key features

- **181 tools** in 12 categories (full list below): navigation, interaction (ref / CSS selector / coordinates, driven by real CDP input), accessibility-tree snapshots (paginated, hover-probing, **child frames included**, **diff by default**), screenshots / element shots / full page / **PDF**, structured table & list extraction, site-aware page rules (github / zhihu / juejin / wechat / xiaohongshu / x.com) that are **data, not code**, user-action recording **plus a trace replayer that needs no LLM**, pull-mode live screencast, console & network capture, download ledger, audit trail, whitelisted raw CDP **with a three-layer full-tier gate**, performance tracing, emulation, batch content fetch, clipboard (text **and images**), history, human-assistance requests, and a local **credential vault** plus a popup-typed **secure credential channel** (`secret.request`)
- **Multi-browser slots**: every connected browser profile registers under a name (`default`, or anything the popup / `browser.launch` assigns). `ubb browsers` lists the slots, `ubb --browser <name>` (or `"browser":"<name>"` in any params) targets one, and `ubb browser launch <name>` spawns an **isolated-profile instance** of Edge/Chrome with the extension pre-loaded and self-pairing through a one-shot nonce page. **Nameless calls target the real logged-in browser** (slot `user_browser`, default `default`); when it is not connected the daemon starts that browser's normal profile minimized and waits. Isolated workspaces are no longer the default — use `browser.launch` for one, or set `auto_workspace:true` to restore workspace-first routing. Unknown names still refuse with `BROWSER_NOT_FOUND` (fail-closed, never "whoever answers first").
- **Iframes are in scope**: the content bridge is injected into every frame (`all_frames`), `snapshot` appends each child frame as an indented `# frame[N]` section with its own refs, and actions on a frame element are dispatched at the element's **top-viewport coordinates**. That coordinate path reaches the **main document and same-process child frames only** (cross-origin, same-site — the page's own `isTrusted=true` record is what distinguishes them). A cross-site **out-of-process frame (OOPIF) receives nothing** from a page-level debugger session's `Input.*`: the gate opens, the coordinates are right, the frame logs zero events. Those targets are therefore refused up front with `FRAME_UNSUPPORTED` — never an `ok:true` that lands nowhere. Two real exceptions ride the frame's **own** CDP target session (`chrome.debugger.attach({targetId})`): `evaluate {frame}` and `upload_file {frame}`. The MV3 boundaries are spelled out in [Iframes](#iframes); do not discover them by guessing.
- **Coordinate input is gated on proof, not on hope**: before any mouse/key action the shield's overlay must *provably* step aside for that specific frame at that specific point (addressed `UBB_PASS` + a DOM read-back). If it cannot, the call aborts with `SHIELD_PASS_FAILED` and **not one mouse or key event is put on the CDP wire**..
- **A visible document, or no input at all**: a trusted pointer/keyboard action additionally asks the target document's own `document.visibilityState` **before dispatching** — a tab in a minimized or covered window (and `tab.select` does not fix that) cannot receive one, so it is refused up front with `INPUT_NOT_DELIVERED` and zero events sent instead of burning a ~7 s proof budget on a queue that never drains. The refusal names the exits that actually exist (user raises the window · `tab.create {"url":…,"active":true}` · the `@d` bridge path, `isTrusted:false` and *not* equivalent). Sensing, screenshots, extraction, diagnostics and the bridge paths keep working in the background..
- **You can tell which build is actually answering**: the extension reports its `BUILD` in the WebSocket `hello`, and `ubb status` / `/health` surface it as `extension_build` next to the daemon's own version. `extension: true` only ever proved *something* is connected — never that it is the code you just edited.
- **Replay**: `record_stop` yields a semantic trace and `replay` re-matches every event against a *fresh* snapshot (role + name scoring, selector as tie-breaker) before driving it — `dry_run:true` prints the plan and touches nothing. `replay.save/list/run/delete` keep named scripts in `chrome.storage.session` (lost on browser restart).
- **Video recording** (`video.start` / `video.status` / `video.stop`): the tab's screencast frames are re-encoded to **VP8/WebM through WebCodecs in an offscreen document** and muxed by a hand-written EBML muxer (`lib/webm-muxer.js`). The tab's single screencast slot is taken through the real `screencast.start` semantics (owner `video`, lease honoured, `SCREENCAST_BUSY` for a live cast). Undecodable frames are counted (`dropped`), a mid-recording resize is **fatal** and the file honestly covers only what was encoded, and the auto-stop at `max_seconds` is stated in the result.
- **Playwright-style locators** (`locator.count / all / wait_for / click / fill / select_option / hover / press`): compose `{role, text, label, placeholder, test_id, css, has_text, checked, disabled, nth, …}` in one spec. The matcher (`content/locator-core.js`) runs in **every frame**; more than one match is a refusal (`LOCATOR_AMBIGUOUS` with candidates), never a guess. Non-OOPIF targets get the same delivery-proved REAL input as `click`/`fill` (`delivered:true` / `INPUT_NOT_DELIVERED`); out-of-process frames fall back to the bridge's synthetic DOM click, reported as `synthetic:true`.
- **Find-in-page** (`page.find_text`): counts literal matches, highlights them with the CSS Custom Highlight API (yellow / orange current) and scroll-centers `navigate_to` — DOM untouched, one frame at a time (`frame_id`, default 0).
- **Bounded heap picture** (`perf.memory`): `Performance.getMetrics` + a short sampling profile aggregated by top constructors — the numbers `HeapProfiler.takeHeapSnapshot` would give, without the megabytes.
- **Credentials never enter the model's context**: values live in `~/.ubb/secrets.json` (created by `ubb secret add`, read from stdin); a tool call carries only a *name*. What may be automated and what is hard-refused: [Credentials and automation boundary](#credentials-and-automation-boundary).
- **Session = tab group**: agent tabs are auto-grouped under a "⏳ UBB" tab group with status emoji. Tabs open silently in the background — nothing is ever activated, focused or switched on the user's screen (opt in with `active:true` for a page that must be visible). With the default real-browser routing the group lives in the user's own window but new tabs stay backgrounded; taking over one of their existing tabs still requires an on-page Allow/Deny banner (60s, default deny) with an optional `expect_title`/`expect_url` identity check (`CLAIM_DRIFT` on mismatch). A dedicated `browser.launch` workspace keeps even the group in a separate minimized window. Tabs marked `deliverable`/`handoff` (`tab.mark`, with a ✅/⏸ favicon badge) **survive `session.stop`** in a 📌 UBB group; everything unmarked closes.
- **Shield on controlled tabs**: pulsing border + input blocking (clicks, keys, wheel, touch) with a one-click "Stop agent" interrupt. Input blocking also covers iframes (`content/frameblock.js` runs `all_frames`); the border is drawn in the top document only. Hidden automatically during screenshots and PDF renders. The blocking layer is a fixed full-viewport shadow host at the maximum z-index, so *the agent's own injected input crosses it only inside a pass window that has been confirmed frame-by-frame* — an unconfirmed pass aborts the action instead of having the veil swallow it silently.
- **Security by default**: token-authenticated loopback WebSocket (extension dials out only — it never listens), restricted pages blocked at both ends of the URL pipeline, destructive actions require explicit `confirm:true`, results are redacted (passwords, tokens, cookies, sensitive URL params), every action lands in an in-memory audit ring whose sequence numbers survive service-worker restarts.
- **For the human watching**: `ubb audit tail --follow` drains the audit ring to `~/.ubb/audit/<day>.jsonl`, `ubb audit stats` summarizes per-tool call counts/errors, and `ubb liveview` serves a loopback-only JPEG page of what the agent is actually seeing.
- **Experimental human mode** (default off): bezier mouse paths with jitter and dwell, per-key typing cadence with occasional typo+backspace, chunked scrolling.
- **Efficient reading & settlement** (the read-after-act economics): every snapshot page is byte-capped (16KB default) with honest truncation cursors; `snapshot` now defaults to `diff:"auto"` — after the first capture of a document it returns only the changed lines since the previous capture (`diff:{added,removed,changed_count,from_nonce}`, refs re-registered either way, `diff:"full"` forces the whole tree, `diff:"off"` the old behaviour); click/fill/type/press_key/hover results carry a `settle` block (request drain → navigation readiness probe → DOM mutation-quiet, budget-bounded, never a sleep); `screenshot {"annotate":true}` overlays red `@eN` badges (set-of-mark); `net.mock.*` stubs/blocks requests per tab via the CDP Fetch domain; `page.init_script` registers MAIN-world document-start site bridges that survive navigations; `tab.events` + live `tab.closed` events steer a run when a tab dies; `browser.pref` sets per-site content settings; daemon-side `repl.run` runs compound browser work in one call through the same vault/confirm/audit gates (explicit `S` scope, 16KB output cap, no fs/process in the sandbox) and `session.transfer` copies ONE origin's cookies between slots with the values never leaving the daemon⇄extension links.
- **Zero dependencies, zero telemetry**: no npm install anywhere — the daemon, the Node CLI and the MCP adapter are plain Node (≥18/22), and nothing is sent anywhere except to your own local daemon.

## Architecture

```
AI agent (any process)
  ⇅ newline-delimited JSON-RPC over stdio, or WebSocket
ubb-bridge.mjs (daemon, 127.0.0.1, token auth)
  ├─ daemon/cli/*.mjs       extra subcommands (secret / audit / liveview / download)
  └─ daemon/mcp-tools/*.mjs curated ubb_* MCP catalog (+ ubb_call fallback)
  ⇅ ws://127.0.0.1:<port>/  — extension connects out, never listens
UBB extension (MV3 service worker)
  ├─ chrome.debugger (CDP)  — page control, one session per tab
  ├─ lib/tools.js + lib/tools-*.js — the tool registry (via lib/tools-extra.js)
  ├─ content scripts — shield (top frame only), bridge + site-rule engine (all frames),
  │                    frame input blocker (all frames), foreign-frame monitor,
  │                    agent cursor overlay + favicon tab badge (top frame)
  ├─ content/site-rules.json — packaged extraction rules (user overrides in storage.local)
  └─ offscreen document — clipboard
```

The live catalog is `ubb list`; it is the authoritative list for the running extension, so the count in this document is a convenience rather than a contract.

## Install

See [INSTALL.md](INSTALL.md). Summary:

1. Load the unpacked extension from this folder (`chrome://extensions` → Developer mode → Load unpacked).
2. Copy the pairing token from the extension popup — or run `bin/ubb install` once and skip token copying entirely (native-messaging auto-pair).
3. Start the daemon: run `ubb status` (auto-starts it), or manually: `node daemon/ubb-bridge.mjs --port 8765 --token <token>`
4. Verify: `ubb status` → `"extension": true` **and** `"extension_build"` equal to the `BUILD` constant in `lib/config.js`. `extension: true` alone only proves *something* is connected — `extension_build` is what proves it is *this* code. If the key is missing entirely, the daemon answering you is an older process (restart it); if the value is a stale string, the extension was never reloaded. A daemon that has been up for two days quietly runs two-days-old code.

## Quick start

```bash
ubb list                                  # tool catalog (181, live)
# these commands are NAMELESS → they run in the user's real logged-in browser.
# If that browser is closed, the daemon starts its normal profile minimized and
# waits for the extension; it does NOT spawn an isolated workspace by default.
# To use an isolated instance, create one: ubb browser launch scratch
ubb tab.create '{"url":"example.com"}'
ubb snapshot                              # accessibility tree with @eN refs (+ # frame[N] sections)
ubb click '{"ref":"@e3"}'                 # result carries settle{} — page drained & quiet, no sleep needed
ubb snapshot '{"diff":true}'              # read what the click CHANGED, not the whole tree
ubb screenshot '{"annotate":true}'        # red box + @eN badge over each ref (set-of-mark)
ubb session.stop

# daemon-side methods (same newline-delimited JSON-RPC interface as every tool):
#   {"id":1,"method":"repl.run","params":{"browser":"default","session_id":"r1",
#    "code":"const s = await snapshot(); console.log(s.diff); await click('@e5'); S.done=true;"}}
#   {"id":2,"method":"session.transfer","params":{"from":"default","to":"work",
#    "origin":"https://example.com","confirm":true}}   # copies ONE origin's cookies slot→slot

# inspect and export
ubb snapshot                              # then: ubb frames.probe   (what this build really supports)
ubb dropdown_options '{"ref":"@e9"}'      # options of a closed <select> / combobox
ubb print_pdf --out report.pdf            # CDP Page.printToPDF, shield hidden
ubb search '{"query":"ubb mv3 iframe"}'   # SERP in a new session tab (nothing is parsed)
ubb replay '{"trace":"…","dry_run":true}' # plan only — no input is dispatched
ubb download list                         # then: ubb download cat 42
ubb audit tail --follow
ubb liveview                              # http://127.0.0.1:8790 — for the human
ubb secret add github_pw                  # value from stdin, never from argv
printf '%s' '{{secret:github_pw}}' ; ubb fill '{"ref":"@e7","value":"{{secret:github_pw}}"}'

# multiple browser slots and semantic locators
ubb browsers                              # connected browser slots
ubb browser launch scratch                # isolated-profile instance, self-pairing
ubb --browser scratch tab.create '{"url":"example.com"}'
ubb locator.click '{"locator":{"role":"button","text":"Submit"}}'
ubb locator.fill '{"locator":{"label":"Username"},"value":"hi"}'
ubb page.find_text '{"query":"needle","navigate_to":2}'
ubb video.start '{"max_seconds":60}'      # … ubb video.stop --out clip.webm
ubb perf.memory '{"gc":true}'
ubb screenshot '{"clip":{"x":0,"y":0,"width":120,"height":80},"zoom":2}' --out region.png

# No Rust CLI on PATH? The Node CLI takes the same commands (Node >= 18):
#   node daemon/ubb.mjs <command>
```

Any agent that can spawn a process can use it: pipe JSON-RPC lines to the daemon's stdin, or connect as a WebSocket agent (`ubb-agent.<token>` subprotocol).

## MCP (backup interface)

For MCP-capable IDE agents (Qoder, TRAE, Cursor, …) that prefer typed tools over shell, `daemon/mcp.mjs` is a zero-dependency MCP stdio adapter speaking to the same daemon — session state is shared with the CLI, and the CLI + skill stay the primary interface:

```bash
node daemon/mcp.mjs                       # stdio MCP server (Node >= 22)
```

IDE MCP settings (Qoder: Chat MCP settings · TRAE: Settings > MCP — both take a stdio JSON config):

```json
{ "command": "node", "args": ["D:/Codebase/ubb/daemon/mcp.mjs"] }
```

The catalog is **52 tools: 51 curated `ubb_*` + the universal `ubb_call {tool, params}`** that reaches the full 181-tool registry (count from `daemon/mcp-tools/index.mjs`; a tool does not have to be curated to be reachable). Token/port resolve exactly like the CLI (`UBB_TOKEN`/`UBB_PORT` env > `~/.ubb/config.json`), and the daemon is auto-started if missing. Binary results are saved rather than inlined: screenshots go to `~/.ubb/shots/*.png|jpg` and **`ubb_print_pdf` lands `~/.ubb/shots/*.pdf`**, returning the path (token-friendly); pass `inline:true` for a screenshot to embed the image instead. Over 3 MB a PDF comes back as `{too_big:true, bytes, filename_suggested}` and you save it with the CLI (`ubb print_pdf --out …`).

## Multi-browser slots

Every browser profile with the extension loaded registers under exactly ONE name on the daemon — its **slot**. The name comes from `chrome.storage.local` (`ubb_browser_name`, editable in the popup's advanced section), defaults to `default`, and rides the WS `hello`.

```bash
ubb browsers                              # [{name, build, browser, version, instance_id, workspace}]
ubb --browser work snapshot               # target one slot (envelope "browser" field)
ubb tab.create '{"browser":"work","url":"example.com"}'   # params.browser works too (stripped before the tool sees it)
```

Routing rules, stated as behaviour:

- **No name given** → the user's **real logged-in browser** (`user_browser`, default `default`), even when an isolated workspace is connected. Connected → routed there. Missing → the daemon starts that browser's **normal profile** minimized (no `--user-data-dir`, no `--load-extension`) and waits (`user_browser_wait_ms`, default 20s) for the extension to reconnect. A failed start surfaces `USER_BROWSER_LAUNCH_TIMEOUT` / `BROWSER_EXE_NOT_FOUND`; it never silently spawns an isolated workspace.
- **Several real slots** without the configured `user_browser` name → `BROWSER_REQUIRED` naming them; the daemon does not guess and does not launch another browser.
- **Workspace-first (opt-in, `auto_workspace:true`)** → the old behaviour: the `agent_browser` workspace slot (default `agent`) is auto-spawned when missing and receives nameless calls.
- **Legacy rules (`auto_open_user_browser:false` + `auto_workspace:false`)**: one connected slot → every call routes there with zero configuration; several slots and no name → refused with `BROWSER_REQUIRED`.
- **Unknown name** → `BROWSER_NOT_FOUND` naming what exists.
- Each slot owns its own session, tab group, refs and CDP sessions — tab ids from two browsers never mix, because every request is answered by one slot.
- `status` / `/health` carry `browsers` (each with `workspace:true/false`), and `status` also names `user_browser`, `auto_open_user_browser`, `agent_slot` and `auto_workspace` so you can always see where a nameless call would land.

### Default routing — the real logged-in browser

Nameless calls target the real browser slot (`user_browser`, default `default`), even when an isolated workspace is already connected. If that slot is missing, the daemon starts the browser's **normal profile** — no `--user-data-dir`, no `--load-extension` — with `--start-minimized about:blank`, then waits `user_browser_wait_ms` (default 20s) for the already-installed extension to reconnect. That is the profile with the user's cookies and logins.

What this means concretely:

- the extension reports `workspace: true/false` in its `hello`; routing prefers the non-workspace slot, so a `browser.launch` instance cannot capture nameless calls;
- agent tabs still open backgrounded (`active:false`) in a UBB tab group, and taking over an existing tab still needs the on-page Allow/Deny banner;
- the cold start is a real browser start: it is minimized, but a profile that restores a session may still paint a window. If you need guaranteed zero windows, use a `browser.launch` workspace instead;
- if the extension is missing/disabled in that profile, the call fails with `USER_BROWSER_LAUNCH_TIMEOUT` after the wait — it never silently falls back to an isolated workspace;
- `browser.close` never triggers a launch (`BROWSER_NOT_FOUND` when the slot is closed).

Config (`~/.ubb/config.json`, read per routing decision):

| key | default | meaning |
| --- | --- | --- |
| `user_browser` | `"default"` | slot name treated as the real browser |
| `auto_open_user_browser` | `true` | start the real browser's normal profile when its slot is missing and wait for it |
| `user_browser_wait_ms` | `20000` | how long to wait for that slot (clamped 500ms–120s) |
| `user_browser_exe` | auto-detect | `"edge"`, `"chrome"` or an absolute path; falls back to `browser_exe` |
| `auto_workspace` | `false` | explicit opt-in to the old workspace-first routing |
| `agent_browser` | `"agent"` | workspace slot name (and profile dir) when `auto_workspace:true` |
| `workspace_wait_ms` | `20000` | how long to wait for a freshly spawned workspace to pair |

`auto_open_user_browser:false` together with `auto_workspace:false` restores the legacy single-slot / `BROWSER_REQUIRED` rules.

### The agent workspace — opt-in isolated profile

A workspace is a separate Edge/Chrome *process* with its own profile (`~/.ubb/profiles/<agent_browser>` — relaunching reuses it, so logins made inside the workspace survive), the extension pre-loaded, and a one-shot pairing page that clears itself to `about:blank` once paired. It is available through `browser.launch` at any time, or automatically when `auto_workspace:true`. The instance is spawned with `--start-minimized` and the extension minimizes its own window once per browser run if anything came up focused. Agent tabs open backgrounded inside that minimized window, so nothing on the user's screen changes; visibility-dependent work raises the workspace's own window.

### browser.launch — an isolated agent workspace

```bash
ubb browser launch scratch [--exe edge]   # or: node daemon/ubb.mjs browser launch scratch
```

Spawns Edge/Chrome with a **fresh user-data-dir** (`~/.ubb/profiles/<name>` — relaunching the same name reuses the profile, so logins made inside the instance survive), the unpacked UBB extension pre-loaded (`--load-extension`), and one tab pointing at the daemon's **one-shot pairing page** (`/__ubb_pair?nonce=…`). The page hands `{type:'UBB_PAIR_LAUNCH', token, port, name}` to the extension — directly via `externally_connectable` where the browser still allows it, otherwise through `content/pair.js` (see `browser.pair`) — the extension stores it, reconnects, clears the pairing tab to `about:blank`, and the slot appears in `ubb browsers` a few seconds later. Launches are **focus-free**: `--start-minimized` at spawn, plus the extension's one-shot minimize (see the workspace section above). From then on the instance is a full citizen: `--browser scratch …` drives it exactly like the main browser.

Trust shape, stated plainly: the pairing page exists for exactly one fetch (the nonce is 128-bit random, single-use, 120s TTL, minted only by `browser.launch`), and the credentials it carries never leave loopback. A page without the nonce gets a 404. The unpacked extension id is derived from the extension path the way the browser derives it (`sha256(path)→a-p`, where on Windows "path bytes" means **UTF-16LE**), which is also what the pair page targets; if your browser disagrees, pass `extension_dir` / `browser_exe` in `~/.ubb/config.json` or as params, and check `edge://extensions` for the real id.

### browser.pair — pair a browser you launch yourself

```bash
ubb browser pair cuaedge '{"extension_dir":"D:/Codebase/cua/ubb"}'
# → {"slot":"cuaedge","pair_url":"http://127.0.0.1:8765/__ubb_pair?nonce=…","extension_dir":…,"extension_id":…,"ttl_s":120}
```

Same one-shot nonce as `browser.launch`, but the daemon spawns nothing: you launch the browser
yourself (`--user-data-dir=<profile> --load-extension=<extension_dir>`, `pair_url` as the first
tab) and it registers under `slot`. `cua edge --ubb` uses this to pair a profile launched in
another Windows session. Two things matter on current Chromium:

- pass `--disable-features=DisableLoadExtensionCommandLineSwitch` — without it the switch is
  ignored (Chromium 137+) and no extension loads;
- pass `extension_dir` as the exact absolute path you give `--load-extension` (Windows
  backslashes), because the unpacked extension id is `sha256(UTF-16LE path) → a-p`.

Web pages can no longer deliver messages to unpacked extensions (`externally_connectable` still
matches, the message still fails with `Receiving end does not exist`), so `content/pair.js`
forwards only the nonce from the pair page and the background fetches
`/__ubb_pair?nonce=…&format=json` from the daemon that minted it. The page never carries
credentials, so a hostile local page cannot aim the extension at a token of its own.

### browser.close — end a workspace instance

```bash
ubb --browser scratch browser.close '{"confirm":true}'
ubb --browser scratch browser.close '{"dry_run":true}'   # list windows, close nothing
```

`browser.close` closes every normal window of **that browser slot** after its reply has been sent, so the daemon can still report the request before the extension disappears. It is meant for `browser.launch` / auto-spawned workspaces; a slot without the workspace flag (the user's real browser) is refused unless you also pass `"allow_user_browser":true`. The profile directory stays on disk, so its cookies and logins survive for the next launch. Because the reply is sent before the windows close, verify the outcome with `ubb browsers` / `ubb status`: a blocking `beforeunload` dialog or a browser kept alive in background mode can keep the slot connected.

This Rust binary (`bin/ubb.exe`) and `daemon/ubb-bridge.mjs` (the Node fallback) implement the same wire behaviour.

## Tools (181)

Grouped exactly as the registry's `category` field groups them. `\*` = `sensitive: true`, i.e. `confirm:true` required.

- **sensing (38)**: snapshot · read_page · extract · read_site · get_html · find · evaluate\* · wait_for · page.find_text · tabs.content (batch) · export.gsuite · export.youtube · webmcp.list · ext.list · locator.count · locator.all · locator.text · locator.texts · locator.attribute · locator.wait_for · element.visible · element.enabled · element.text · element.texts · element.attribute · element.info · dropdown_options · site.rules.list · site.rules.validate · site.rules.reload · site.rules.add\* · site.rules.remove\* · site.rules.clear\* · challenge.detect · page.probe · shadow.list · paper.resolve · challenge.cas_form
- **diagnostics (36)**: console.list · network.list · network.get · page.init_script · dialog.status · dialog.handle · history.list · perf.start · perf.stop · perf.memory · cdp.call\* · cdp.events · assets.list · assets.bundle · audit.list · audit.tail · audit.stats · download.list · download.wait · download.path · download.status · frames.list · frames.probe · video.reset · challenge.bg_dump · challenge.canvas_gap · challenge.audit · challenge.dom · challenge.help · dev.reload\* · dev.focus\* · dev.key · dev.clear_cookies\* · dev.build · browser.diagnostics · browser.pref
- **interaction (31)**: click · hover · fill · fill_form · type · press_key · scroll · drag · upload_file · download · download.current · form.set · ax.action · locator.click · locator.fill · locator.select_option · locator.hover · locator.press · webmcp.invoke\* · ext.call\* · replay · replay.save · replay.list · replay.run · replay.delete · challenge.solve · challenge.pick · shadow.click · shadow.fill · paper.download · download.clean
- **tabs (20)**: tab.list · tab.create · tab.close · tab.select · tab.claim · tab.release · tab.mark · tab.repair · tab.lifecycle · tab.gc\* · tab.close_ubb\* · tab.close_spawned · tab.events · resize_window · window.minimize · window.restore · window.state · window.create · tab.move · window.raise\*
- **capture (11)**: screenshot · screencast.start · screencast.next · screencast.stop · video.start · video.status · video.stop · print_pdf · record_start · record_status · record_stop
- **session (25)**: session.start · session.stop · session.status · session.name · browser.close\* · session.recover · request_help · human.status · human.set\* · challenge.status · challenge.wait · challenge.job · challenge.warmup · bot.report · policy.set · policy.get · cookie.export\* · cookie.import\* (INTERNAL_ONLY — daemon session.transfer only) · pipeline.status · institution.profile.list · institution.profile.set · institution.profile.remove · institution.carsi · institution.sso_handoff · institution.webvpn
- **navigation (5)**: navigate · go_back · go_forward · reload · search
- **secrets (5)**: secret.list · secret.set\* · secret.delete · secret.fill · secret.request
- **emulation (2)**: emulate · emulate.reset
- **clipboard (3)**: clipboard.read\* · clipboard.write · clipboard.write_image
- **orchestration (2)**: batch · pipeline.run

**Gated (`confirm:true`) — the 17 sensitive tools**: `browser.close`, `evaluate` (MAIN world), `cdp.call` (write class), `clipboard.read`, `webmcp.invoke`, `ext.call`, `human.set`, `secret.set`, `site.rules.add`, `site.rules.remove`, `site.rules.clear`, `dev.reload`, `dev.focus`, `dev.clear_cookies`, `window.raise`, `tab.gc`, `tab.close_ubb`. Two gates are orthogonal to this list and still apply per action: a dangerous/paid label (delete/pay/transfer…) needs `confirm:true` even on `click`, and `secret.set` additionally warns that the *value you pass* enters the conversation transcript — which is exactly why `ubb secret add` in a terminal is the better route.

**Policy strict tier** (`policy.set {"strict":true}` / popup 「高级 · 安全开关」, default OFF; turning it ON needs `confirm:true` itself): while strict is on, the risk-class tools — `challenge.solve` (captcha), `download.clean` / `tab.close_ubb` (delete), `secret.set` / `clipboard.read` / `history.list` (transmit), `cdp.call` / `evaluate` (raw_cdp) — refuse with `NEEDS_CONFIRM` (`data.risk` names the class) unless the same call carries `confirm:true`. `policy.get` reports `{strict, risk_classes}`. `secret.request` is deliberately not on this list: its gate is the popup itself (the user submits or cancels the 「安全凭据」 form), not a flag the model can type.

`secret.fill` is deliberately **not** in the gated list. `confirm:true` is something the model can type itself, so it is not a boundary — an attacker who controls the agent just sets it. What actually bounds `secret.fill` is structural: the value never appears in the agent's arguments or results, it is fetched from the daemon at typing time, a vault entry can be origin-bound (`ubb secret bind`) with `strict` mode refusing any host mismatch *before* the value is even fetched, every fill is audited with `{name, host, field_role}`, and the never-fill field classes (OTP/CVV/card/private key) are refused whatever the source.

Ref discipline: refs (`@eN`) come from `snapshot` and stay valid across snapshots and across `next_cursor` pages of the same capture; navigation invalidates them (`REF_UNKNOWN`) — take a fresh snapshot. Large pages return `next_cursor` instead of silently truncating. Frame refs are `@eN` *inside* a `# frame[N]` section (addressable; coordinate actions reach them in the main document and in same-process frames — a cross-site OOPIF ref refuses with `FRAME_UNSUPPORTED`) or `@d<frameId>.<n>` from `find`, which acts through that frame's own bridge and therefore works in an OOPIF too.

## Credentials and automation boundary

Secret values live in `~/.ubb/secrets.json` and are referenced by name only (`secret.fill`, or `{{secret:NAME}}` inside `fill` / `fill_form` / `replay`). The value is fetched from the local daemon at typing time and never appears in tool arguments, results, the audit, daemon logs, or the conversation. Placeholder substitution is limited to `fill` / `fill_form` values and `replay` vars; `evaluate`, `cdp.call`, `webmcp.invoke` and `type` never substitute.

OTP / one-time-code / CAPTCHA / CVV / CVC / card numbers / private keys are refused in every mode, and so are fields whose name contains `secret`. **The one exception is `secret.request`**: the agent only describes the fields (`ref:"@eN"`, `label`, `type:"username"|"password"|"otp"|"text"` — the type enum is closed, so it cannot even declare a card/CVV field), and the **user** types the values into the popup's 「安全凭据」 panel; the extension fills them and the values exist in memory only — never the conversation, the audit (metadata `{fields, origin, decision}` only), `chrome.storage` (the pending-request list in `storage.session` carries metadata, no values), or the daemon. OTP/验证码 is allowed on that channel and only that channel, because it is the user's own hand typing; CVV/CVC/card numbers/private keys are refused on every channel including it. The default vault origin mode is still `off` (an unbound password can be typed into any page's password field); bind entries with `ubb secret bind <name> <host>` and switch to `ubb secret mode strict` for fail-closed behavior. Every vault fill is audited as `{vault_entry, origin, field_role, mode, decision}`; the value itself is never recorded — and a `secret.request` fill is audited as `{action:'secret.request', fields:[{id,label,type}], origin, decision}` with no value.

## Input delivery

Trusted input is dispatched only after the target document reports itself visible and the shield pass confirms the point. A hidden or occluded document is refused with `INPUT_NOT_DELIVERED` before any event is sent, and a click is reported as delivered only when the page’s own record proves it. Cross-site OOPIF frames have no input channel (`FRAME_UNSUPPORTED`).

## Iframes

Design: **sensing goes through a bridge injected into each frame; actions go through top-viewport coordinates — and coordinate penetration is limited to the main document and same-process child frames.** The extension can compute a frame element's box in the *top* document (the `<iframe>` element belongs to its parent document, so `getBoundingClientRect` works even cross-origin; the chain comes from `chrome.webNavigation.getAllFrames`), and a page-level debugger session's `Input.*` lands in whatever document owns that box **as long as that document shares the renderer process**. A cross-site document in its own process (OOPIF) does not receive it: the events leave the wire and the frame records nothing. That is a hard refusal, not a discovery you make at runtime — **every action against an out-of-process frame target answers `FRAME_UNSUPPORTED` before a single `Input.*` is sent**, and the message names the three exits that actually exist: act through that frame's own bridge with an `@d<frameId>.<n>` ref (`click` / `fill` / `select`, in-page synthetic events — the same channel `read_page` uses, which does reach OOPIFs), let the user click, or `tab.create` the frame URL as a top-level tab. **Perception is never refused** — `read_page` / `find` / `element.info` / `screenshot` still see those frames. `content/bridge.js` and `content/frameblock.js` are `all_frames`, so a frame's DOM, snapshot labels, dropdown options and dialog handling are reachable; every outbound `chrome.tabs.sendMessage` carries an explicit `frameId` (without it, MV3 broadcasts the message to *every* frame, which would run a write op N times).

Capability matrix — what does **not** work, and why:

| Situation | Behaviour |
|---|---|
| Cross-site (OOPIF) frame + a node-based op | `ax.action {action:"setValue"\|"expand"}` still refuses (`FRAME_UNSUPPORTED` — it runs page JS on a node the flattened session cannot reach). **`evaluate` and `upload_file` are the two real exceptions**: pass `frame` (a `frames.list` frameId or url/name matcher) and they ride that frame's **own** CDP target session (`chrome.debugger.attach({targetId})` — MV3 still exposes no `sessionId`, but a child target attach does not need one); `upload_file` does `DOM.getDocument` + `DOM.setFileInputFiles` on that target's own DOM document. Same-URL multi-match → `OOP_TARGET_AMBIGUOUS`. |
| Cross-site (OOPIF) frame + a **coordinate** op | Also `FRAME_UNSUPPORTED`, refused **before** any `Input.*` goes on the wire, at every coordinate-input entry point: `click`, `hover`, `fill`, `fill_form`, `form.set`'s physical-click branch, `ax.action {focus\|click}`, both `drag` endpoints, and `snapshot {probe_hover}`. Reason: the events leave the wire, the frame records nothing, and a tool call would otherwise answer `{"clicked":…}` while that document's own event log stayed empty. Exits named in the error: the `@d<frameId>.<n>` bridge path, the user's own click, or `tab.create` the frame URL as a top-level tab. Sensing (`read_page` / `find` / `element.info` / `screenshot`) and `select_option` via the frame bridge are **not** refused — they reach those frames. Same-process child frames (cross-origin, same site) keep working on the coordinate path. |
| `srcdoc:` / `about:blank` / `file:` frame | `FRAME_UNREACHABLE` for bridge ops: the content scripts match `http(s)` only, so those frames have no bridge, no input blocker. Perception can still drill in from a same-origin parent via `frame_path`. Closing the input-blocking gap needs a `manifest.json` change — left open deliberately. |
| Several frames with the same URL/identity | `FRAME_AMBIGUOUS` / `REF_FRAME_AMBIGUOUS`: refused, never guessed. |
| A frame ref after its document changed | `REF_STALE` — stale coordinates in a frame are worse than on the top page, so this fails closed. |
| A frame's content bridge stops answering | `BRIDGE_TIMEOUT` (12 s for a normal op, 30 s for the heavy read ops — `getPageText` / `getDomTree` / `search` / `extractData` / `extractSite`) and then a **30 s** cooldown for that `(tabId, frameId)`. During the cooldown the call does **not** blind-reject — it first sends an 800 ms `selfInfo` liveness probe addressed to that frame: no answer → sub-second `BRIDGE_WEDGED` and the window re-arms; the frame answers → the cooldown is dropped and the call runs for real, so a healed frame recovers without a navigation (`dialog.handle` alone is enough). A probe that reports "no listener there" surfaces `BRIDGE_UNREACHABLE`. The bookkeeping is per `(tab,frame)`, which is what the code can guarantee: it bounds the *cost* of a dead frame to ≤800 ms per call instead of a full 30 s. Whether one wedged frame also jams the tab-level channel for its siblings is not guaranteed either way, so no promise is made about sibling frames — each frame is judged by its own probe. Without this, every dependent call burns the full 40 s `TOOL_TIMEOUT`, one after another, until the tab looks permanently broken. The record also clears on navigation (`webNavigation.onCommitted/onCompleted`) and on `tabs.onRemoved`. |
| The shield's veil will not move aside for that frame/point | `SHIELD_PASS_FAILED` — the action was aborted, zero input events were sent. Exits are listed in [Input delivery](#input-delivery). |
| `select_option` inside a frame | Works via the frame bridge (`via:'frame-bridge'`, `result.synthetic:true`) — the `change` event is synthetic DOM, exactly like the main-frame `callOnNode` path. |
| iframe loaded while the agent was mid-task | Re-armed through `webNavigation.onCommitted` (`frameId > 0`), but only for tabs the session owns and only in the shield state the top frame actually reports. Frames inserted before any arm and lost to a service-worker restart are picked up by the next `snapshot` resync. The re-arm answers with a verdict rather than silence — `{sent, reason, mode, error}` (`shield_unreachable` / `shield_not_active` / `frame_unreachable` / `storage_unreadable` / `main_frame`), and `background.js` writes that verdict to the audit ring (`action: shield.rearm_frame`), so `ubb audit tail` tells you whether a given frame actually got armed instead of leaving you to guess. |

After reloading the extension, run `ubb frames.probe` once on a page with iframes: it returns the truth table for *this* browser build (per-CDP-frame `Accessibility.getFullAXTree` / `Page.createIsolatedWorld` / `DOM.getFrameOwner` success or error code, bridge reachability, frame pairing, devicePixelRatio).

`frames.probe` is **read-only in fact, not just in its description.** It samples the coordinate example with `scroll:false`, takes the box and scroll offsets from the same source at the same moment, and reports its own evidence: `checks.page_state_unchanged {unchanged, scroll_moved_in_frames}`, `checks.scroll_y_by_frame {before, after}` per frame, and a `probe_moved_the_page` flag on the sample if it ever moves the page anyway. A negative `y` in the sample is a *statement about the current scroll position* (off-screen but measured), not a measurement error. Pairing is observable too: `paired[]` rows carry `resolved_by`, `unresolved[]` rows are structured with a reason and the candidate orders, and `checks.window_name_handshake` keeps the **full** error instead of truncating it at 120 characters.



## Human verification (captcha)

`challenge.*` detects and solves mainstream human-verification widgets **in the user's own session**, with real CDP input and human motion, and only reports `solved` when the PAGE admits it (token field non-empty / widget gone / URL, title or document epoch changed). Coverage: reCAPTCHA v2 checkbox + grid, v3/Enterprise score, hCaptcha checkbox + canvas drag puzzle, Cloudflare Turnstile + waiting room, GeeTest slider, Aliyun Captcha 2.0 (drag-to-end and puzzle), Arkose press-and-hold, AWS WAF, generic slider/grid/text captchas. Image grids, canvas puzzles and text captchas return a screenshot and take one agent step (`challenge.pick`); sliders and checkboxes are one-call. No external solver, no token service, no outbound request; token values are never read (length only) and OTP/CVV fields stay refused.

`snapshot` prepends a one-line banner when a challenge is present, so the model cannot click through a waiting room by accident.

The coverage matrix and limits are also available at runtime from `ubb challenge.help`; the executable coverage lives in the `challenge.*` tool suite. Quick use:

```bash
ubb challenge.detect
ubb challenge.solve '{"mode":"auto"}'
ubb challenge.pick '{"cells":[1,4,7]}'                 # grid
ubb challenge.pick '{"drag":{"from":[.13,.33],"to":[.4,.67]}}'  # canvas
```

## Site rules are data

The six built-in extractors (github / zhihu / juejin / wechat / xiaohongshu / twitter) live in `content/site-rules.json` and are executed by a small op interpreter in `content/bridge.js` (`text / join_texts / images / videos / group / repeat / reset / when`). Adding or fixing a site is **a JSON rule, not a code change, and not an extension reload**:

```bash
ubb site.rules.validate '{"rules":[…]}'   # grammar + selector + executable-key checks, nothing stored
ubb site.rules.add '{"rules":[…],"confirm":true}'   # one rule → chrome.storage.local ("ubbSiteRules")
ubb site.rules.remove '{"id":"my-rule","confirm":true}'   # drop one user rule
ubb site.rules.clear '{"confirm":true}'    # drop all user rules
ubb site.rules.reload                     # re-assemble the service-worker cache
ubb site.rules.list                       # effective set, in match order
```

User overrides are merged **ahead of** the built-ins and win on the same `id`. The packaged file is read by the service worker (`content/` fetch from a content script is blocked by MV3 without `web_accessible_resources`), cached in `lib/bridge-ctl.js`, and attached to each `extractSite` call. Validation hard-refuses any executable key (`eval`, `script`, `function`, `on*`, `html`, `url`, `src`, …) — rules are data, never code. User rules are removed by `site.rules.remove` (`id`) or `site.rules.clear`; both require `confirm:true`.

## Configuration

| Item | Where | Default |
|---|---|---|
| Pairing token | popup (view / paste / regenerate), or `ubb install` auto-pair | random 64-hex |
| Daemon port | popup / `--port` / `UBB_PORT` / `~/.ubb/config.json` | 8765 |
| Liveview preview port | `ubb liveview --view-port` / `UBB_LIVEVIEW_PORT` | 8790 |
| Shield mode | popup | border + block input (top frame + all frames) |
| Human mode | popup / `human.set` | off |
| Secret vault | `~/.ubb/secrets.json` (`UBB_SECRETS_FILE` overrides) | absent until first `ubb secret add` |
| Secret origin mode | `ubb secret mode off\|strict` (key `secret_origin_mode` in `~/.ubb/config.json`), per-call `enforce` | off |
| Secure credential requests | popup 「安全凭据」 panel ↔ `chrome.storage.session` key `ubb_sec_requests` (metadata only — values live in memory and die with the fill) | absent until a `secret.request` pends |
| Raw CDP full tier | popup 「高级 · 安全开关」 — `chrome.storage.local` key `ubb_cdp_raw` | off |
| Raw CDP origin grants | `chrome.storage.local` key `ubb_cdp_origins` — `{origin: {granted_at, lifetime: once\|session\|forever}}`; banner grants once/session, popup manages forever; `session` grants clear on browser SW restart / `session.stop` | empty |
| Policy strict tier | popup 「高级 · 安全开关」/ `policy.set` — `chrome.storage.local` key `ubb_policy_strict` | off |
| Foreign-frame guard mode | popup — `chrome.storage.local` key `ubb_guard_mode` | `session` |
| Agent cursor overlay | popup — `chrome.storage.local` key `ubb_cursor_enabled` | on |
| Site rules | `content/site-rules.json` + `chrome.storage.local` key `ubbSiteRules` | 6 built-ins |
| Extension build stamp | `lib/config.js` `BUILD` → sent in the WS `hello` → surfaced as `extension_build` by `ubb status` / `/health` | code constant |
| Audit dump | `ubb audit tail` (appends JSONL; `--out <file>` to move it) | `~/.ubb/audit/<day>.jsonl`, ring of 500 |

Daemon CLI: `ubb status|list|<tool> '<json>'` (auto-starts the daemon), `ubb daemon start|stop`, `ubb config token|port`, plus the contributed subcommands `ubb secret add|list|rm|has|bind|mode`, `ubb audit tail|stats`, `ubb liveview`, `ubb download cat`, `ubb diagnostics [--json]` (token-free, read-only browser matrix) and `ubb tunnel start|status|stop` (opt-in relay). If the Rust CLI is not installed, the Node CLI takes the same commands: `node daemon/ubb.mjs ...` (Node ≥ 18) — a stale `bin/ubb.exe` answers `unknown tool tool.diagnostics` for `ubb diagnostics`; rebuild it (`cargo build --release` in `rust-cli/`) or use the Node CLI. Note `tunnel.*` is served by the Node daemon only (`node daemon/ubb.mjs daemon start`).

**`--port` means the daemon port, everywhere.** The liveview preview listener has its own flag: `ubb liveview --view-port 8791`. With `--port` it points at the daemon, and the CLI prints a warning line when it sees that shape (and refuses if the two ports collide).

## Error codes (common)

Session / tabs / targets: `EXTENSION_NOT_CONNECTED` · `NO_SESSION` · `NO_TAB` · `TAB_NOT_OWNED` (claim first) · `TAB_NOT_FOUND` · `REF_UNKNOWN` (re-snapshot) · `REF_STALE` (element or frame document is gone) · `BAD_REF` · `BAD_MARK` (`tab.mark` accepts only `deliverable` / `handoff` / `none`) · `CLAIM_DRIFT` (the tab no longer matches `tab.claim`'s `expect_title`/`expect_url` — `data.actual` says what it really is; do not take it over)

Pages: `URL_BLOCKED` (chrome://, edge://, store, and `https://chrome://…` laundered shapes) · `BAD_URL` (refusing to prepend a scheme to something that is not a bare host) · `DEBUGGER_CONFLICT` (DevTools or another automation extension holds the tab — also an OOPIF target) · `CDP_METHOD_BLOCKED` (raw CDP: whitelisted-set refusal, or a full-tier blocklisted method — `data.reason` carries the Chinese why) · `CDP_ORIGIN_DENIED` (raw-CDP full tier: that origin has not approved the banner, or refused — cached 10 min; `data.approve_hint` points at the popup) · `OOP_TARGET_AMBIGUOUS` / `OOP_TARGET_NOT_FOUND` (the `frame` parameter of `evaluate`/`upload_file` matched several / no OOPIF target — disambiguate by frameId, never guess) · `TOOL_TIMEOUT` · `CDP_TIMEOUT` (renderer stopped answering, 45s cap) · `CDP_DETACHED` (debugger lost mid-command; next command re-attaches)

Interaction / gating: `NEEDS_CONFIRM` (retry with `confirm:true`; under policy strict this also fires per call for the risk-class tools — `data.risk` names the class) · `SENSITIVE_FIELD` (OTP/CVV/card/… — never auto-filled, no bypass; OTP's one allowed route is the user-typed `secret.request` popup channel) · `CLAIM_DENIED` · `CANCELLED` (user pressed the shield's stop button)

Shield / bridge: `SHIELD_PASS_FAILED` (the overlay did not move aside for that frame at that point — **the action was aborted and no mouse/key event was ever sent**, so `ok:true` with an empty page is no longer possible; do *not* retry the identical call, take one of the listed exits: the shield strip's stop button → popup `block|border|off` → reload the extension → `tab.release` + `tab.claim`) · `gate_bypassed` (**not an error code — a key on `cdp.call`'s result and on its audit entry**: the raw-CDP escape hatch still sends `Input.dispatchMouseEvent` / `Input.dispatchKeyEvent` / `Input.insertText` with `confirm:true`, and that route is *not* run through the per-dispatch shield confirmation above, so our own overlay may eat the event and **the tool will not confirm it for you** — the return value says so out loud (`gate_bypassed: true` + a `note`, same key in `ubb audit tail`) instead of coming back clean; if the input has to land, use `click` / `type` / `fill` / `press_key`) · `BRIDGE_TIMEOUT` (a frame's content bridge did not answer inside its own bound — 12 s, 30 s for the heavy read ops — instead of burning the 40 s tool timeout; message points at `dialog.handle` / reloading the tab) · `BRIDGE_WEDGED` (that `(tabId, frameId)` is on its 30 s cooldown and its 800 ms liveness probe got no answer, so the call was skipped in well under a second instead of re-paying the full budget; the window re-arms on every failed probe and drops the moment the frame answers — no navigation needed. Bookkeeping is per frame, which is a promise about **cost**, not about sibling frames: if the whole tab-level channel is jammed, every frame answers through its own probe) · `BRIDGE_UNREACHABLE` (the probe reached nothing at all — no listener in that frame)

Frames: `FRAME_UNSUPPORTED` (this build has no input channel into that frame — for an out-of-process frame that includes the **coordinate** path, so do not "retry with coordinates": act through that frame's bridge with an `@d<frameId>.<n>` ref, let the user click, or `tab.create` the frame URL as a top-level tab) · `FRAME_UNREACHABLE` (no bridge in that frame: srcdoc/about:blank/file) · `FRAME_AMBIGUOUS` · `REF_FRAME_AMBIGUOUS` · `FRAMES_UNSUPPORTED` (`chrome.webNavigation.getAllFrames` unavailable)

Input delivery: `INPUT_NOT_DELIVERED` — the door was open and the input still cannot be reported as a success. **Two origins, told apart by `data.stage`** · *`stage:"pre-dispatch"`*: the target document's own `visibilityState` is not `"visible"` (window minimized or covered by another window — `tab.select` cannot change that), so **nothing was dispatched** (`dispatched:false`, `events_on_wire:0`) and the call failed in well under a second instead of waiting out the 7 s proof budget on a hidden queue that never drains; the message gives the only three real exits — ① the user raises that window (no tool can), ② `tab.create {"url":…,"active":true}` / `tab.select` (**an occluded window stays `hidden` through `tab.select`** — the answer says so), ③ the `@d`-ref bridge path, labelled `isTrusted:false` and **not** semantically equivalent to real input · *no `stage`, `waited_ms≈7000`*: the events did go out, the document's delivery ring recorded none of them (`records_seen` / `missing_events` / `veil_at_read` are in `data`); a press/release/key is **never** auto-repeated, so do not blind-retry — decide whether a repeat is safe, or take exit ①/②/③ above. A document whose visibility cannot be read is neither waved through nor refused on that guess: it keeps the dispatch + full proof-budget route.

Secrets: `SECRET_UNKNOWN` (name not in the vault; the message names the entry, never a value) · `SECRET_ORIGIN_DENIED` (strict mode / host mismatch / unbound under strict) · `SECRET_UNRESOLVED` (a `{{secret:…}}` reached execution unresolved — hard stop) · `NOT_PASSWORD_FIELD` (use `fill` for a non-password field) · `BAD_SECRET_NAME` · `SECRET_FETCH_TIMEOUT` · `DAEMON_NOT_CONNECTED` · `SECRET_REQ_BUSY` (a `secret.request` is already pending on that tab) · `SECRET_REQ_TIMEOUT` (nobody filled the popup's secure form before the deadline — default 120s, cap 180s)

Replay: `MATCH_NOT_FOUND` (carries the 3 closest candidates + `start_at` advice) · `VAR_UNKNOWN` (names the var only) · `TRACE_EMPTY` · `TRACE_FORMAT` · `TOO_MANY_STEPS` (>500 — chunk with `start_at`/`max_steps`) · `TRACE_TOO_LARGE` (>1MB session storage)

Downloads / files: `DOWNLOAD_PENDING` (still transferring; `data` carries `{state, bytes_received, total_bytes}`) · `NOT_FOUND` · `BAD_PARAMS` · CLI `ubb download cat`: `BINARY` / `OUTSIDE_ROOT` / `PATH_REFUSED` / `BAD_ID` / `MISSING_PATH`

Capture: `SCREENCAST_BUSY` (another holder owns the tab's single cast slot; message names the owner) · `NO_SCREENCAST` · `CAPTURE_STALE` · `CAPTURE_GONE`

Site rules: `SITE_RULE_INVALID` · `SITE_RULE_LIMIT` · `SITE_RULE_TOO_LARGE` · `SITE_RULES_MISSING` · `BAD_SELECTOR`

Others: `SELECTOR_FAILED` (shadow DOM needs a ref) · `EVAL_ERROR` · `ELEMENT_NOT_VISIBLE` · `BODY_UNAVAILABLE` · `WAIT_TIMEOUT` (a condition this extension could not *read* never counts as satisfied — the message says so and names the last page-read error) · `EMULATE_FAILED` (the browser refused the CDP override: the emulation you asked for did **not** apply, and whatever was set before it is still in force) · `UNKNOWN_TOOL` · `CLIPBOARD_FAILED` · `PRINT_TO_PDF_FAILED` · `STORAGE_FAILED` · `DIAG_UNKNOWN` (`browser.diagnostics` could not get the daemon scan — check `ubb daemon status`) · `BAD_REASON` (`bot.report` `reason` must be one of the seven enum values)


## Research workflow tools

These tools cover closed shadow roots, publisher lookup, PDF downloads, institution access and batched runs. All of them run through the same dispatcher and keep the truthfulness rules: a refused input is an error, and a page that did not advance is not "solved".

| Tool | What it does |
|---|---|
| `shadow.list` / `shadow.click` / `shadow.fill` | Pierced-DOM perception and real coordinate input for **closed shadow roots** (CDP `DOM.getFlattenedDocument({pierce:true})` + `DOM.getBoxModel`). Top document only. `locator.click` falls back to the same scan (`via:"pierced"`) when the bridge `locatorQuery` itself fails. |
| `page.probe` + `wait_for {condition:"content_changed"}` | One cheap `{url,title,doc_id,text_hash,first_text}` read for SPA work (pagination, AJAX lists); `wait_for` accepts a previous probe as `from` and reports the new one. |
| `download.current` | Download **the URL the tab is showing** (signed CDN PDF, PDF viewer) via `chrome.downloads`, with an optional `filename` relative to the browser download directory. `download` also accepts `filename` / `conflict_action`. |
| `window.raise` | Confirm-gated foreground raise (`tabs.update active` + `windows.update focused/state:normal`), then reports the real `document.visibilityState`. It says `raised:false` when another app still covers the window. |
| `session.recover` / `tab.repair` / `tab.close_spawned` / `tab.close_ubb` / `tab.lifecycle` / `tab.gc` | After an extension reload: adopt every tab still in a UBB group (multi-group aware), re-inject the manifest content scripts, re-arm the shield and verify it answers; close tabs that pages opened from session tabs (`chrome.tabs.onCreated` + `openerTabId`). `session.stop` closes unmarked created/spawned tabs by default — tabs marked `deliverable`/`handoff` survive (see [Tab lifecycle, secure credentials and raw-CDP control](#tab-lifecycle-secure-credentials-and-raw-cdp-control)); `tab.lifecycle` shows the persistent registry (ownership created/claimed/spawned/recovered/orphaned, session, age, recent closures) across extension reloads, and `tab.gc` closes orphaned tabs with `dry_run`/`confirm`. `tab.close_ubb` remains the one-call sweep of every tab in any UBB group. |
| `challenge.cas_form` | Read-only probe of a CAS/campus login form: username/submit viewport boxes, whether Edge credential autofill has materialized (`pwdlen_audit` = password length only, plaintext never leaves the page), error text, slider-layer visibility. Edge autofill only lands after the username field gains FOCUS — click it, re-probe, submit only when `pwdlen_audit > 0`. |
| `challenge.solve {async:true}` + `challenge.job` | Long solves (CF retries, human handoff) return a job id immediately; poll the job. `lost` means the service worker restarted before the solve finished — inspect the page instead of assuming success. |
| `paper.resolve` / `paper.download` | DOI/URL → publisher, access state (`pdf-link` / `paywalled` / `viewer`) and PDF candidates from `citation_pdf_url`, page anchors and per-publisher rules (ACS, Wiley incl. society subdomains like chemistry-europe./advanced./aces. — each holds its own Shibboleth session, T&F, RSC, Springer, Nature, IOP, MDPI, Frontiers, ScienceDirect viewer + linkinghub, MATEC). `paper.download` tries candidates until a real PDF arrives — each candidate through TWO channels (chrome.downloads, then an in-page blob fetch that carries the tab's Referer/CF clearance, beating `SERVER_FORBIDDEN`) — deletes HTML impostors, completes a campus CAS login automatically if the DOI redirect landed on one (focus → Edge autofill → submit → slider solve; no password is ever read or typed), and for ScienceDirect opens the View PDF viewer tab to capture the signed URL. Candidates fetched on a **WebVPN-proxied page are rewritten onto the proxy prefix** so the institutional authorization is kept. |
| `pipeline.run` / `pipeline.status` | Item × steps runner with `{{item}}` / `{{item.field}}` substitution, per-step retries, a delay between items, and a persisted record for resume with the same `job_id`. `pipeline.run` and `batch` cannot be nested. |
| `institution.profile.list/set/remove` + `institution.carsi` / `institution.webvpn` / `institution.sso_handoff` | Institution access is **profile-driven data, not a school hardcode**: a profile describes the CARSI school name, WebVPN SSO entry, WebVPN home pattern, library navigation labels/selectors and proxy shape. The engine runs the CARSI school/resource flow or the WebVPN → library database → entry → proxied publisher flow for any school; a human handoff covers login pages. **No password is ever read, typed or stored by these tools.** |
| Slider coverage | `slidercaptcha` (two-canvas puzzle used by many Chinese campus SSO pages) is detected and solved by the canvas gap analysis; the build also recognises the ScienceDirect Cloudflare variant ("Are you a robot?" without `#challenge-stage`). |
| Screenshot metadata | Clip screenshots return `image:{width,height}` and `clip_scale` (the renderer's real capture scale, which is not always `devicePixelRatio`). |


## Tab lifecycle, secure credentials and raw-CDP control

| Tool | What it does |
|---|---|
| `tab.mark {tab_id?, mark}` | Mark a session tab `deliverable` (green ✅ favicon badge) or `handoff` (amber ⏸ badge); `none` clears and restores the original favicon; omitting `mark` queries. The badge is composited onto the page favicon (`content/tab-badge.js`; re-sent on navigation). Non-session tabs refuse (`TAB_NOT_OWNED`), bad values refuse (`BAD_MARK`). `tab.list` rows carry `mark`. |
| `session.stop` (mark semantics) | **Default inverted**: unmarked created/spawned agent tabs close as before, but marked tabs survive — they stay in a 📌 UBB group with their badge and are remembered in the tab registry as orphaned (`tab.gc` can collect them later). `close_marked:true` restores the old close-everything. Marks live in the UBB **session** domain: `session.start` does not clear them. |
| `bot.report {reason, tab_id?, detail?}` | Structured anti-bot telemetry: a closed `reason` enum (`captcha_failed`, `access_denied`, `challenge_loop`, `sign_in_blocked`, `add_to_cart_blocked`, `purchase_blocked`, `unexpected_bot_error` — wrong value → `BAD_REASON`), hostname only (never the full URL), `detail` capped at 200 chars in the audit. `challenge.solve` failures write the same-shaped audit row automatically. |
| `policy.set {strict}` / `policy.get` | Strict confirmation tier over `ubb_policy_strict` (default OFF; ON itself needs `confirm:true`). Under strict, risk-class tools (captcha / delete / transmit / raw_cdp — see the gated list above) demand action-time `confirm:true` on every call (`NEEDS_CONFIRM`, `data.risk`). |
| `browser.diagnostics` | Read-only daemon-side matrix of edge/chrome/brave/opera/vivaldi: installed, running, UBB extension installed/enabled per profile, native-messaging manifest validity, daemon slots. Same data as the token-free `ubb diagnostics [--json]`; daemon unreachable → `DIAG_UNKNOWN`. |
| `secret.request {tab_id?, fields, timeout_ms?, title?}` | The secure credential channel: the agent only describes fields (`ref:"@eN"` positions them, ≤8, closed type enum `username\|password\|otp\|text`, `label` is display text ≤40 chars and refuses URL/newline/markup); the user types into the popup's 「安全凭据」 panel; the extension fills and the values die with the response — never the conversation, the audit (metadata only), storage or the daemon. **The only channel an OTP/验证码 may be filled through** (user's own hand); CVV/CVC/card/private keys are refused on every channel. `SECRET_REQ_BUSY` while a request pends on the tab, `SECRET_REQ_TIMEOUT` when nobody answers (default 120s, cap 180s). |
| `snapshot` (diff default) | `diff:"auto"` (new default) / `"full"` / `"off"` (`true`/`false` accepted for compatibility). Auto returns the incremental view against the same document's previous capture (`diff:{added,removed,changed_count,from_nonce}` + `diff_note`; refs still fully registered; the diff view carries its own cursor) and falls back to the full tree without an honest baseline. `replay`/`find` pass `diff:'off'` internally. |
| `cdp.call` (three-layer gate) | Default behaviour unchanged. With the popup's 「原始 CDP」 switch (`ubb_cdp_raw`, default OFF) non-whitelisted methods open through the full tier: (1) a 15-entry method blocklist (`CDP_BLOCKLIST` in `lib/cdp-gate.js` — close/crash, tab/session bypass, `Debugger.*`/`Fetch.*`/`Network.setRequestInterception`, CSP/download/storage bypass; each refused with a Chinese reason, harmless reads pass), (2) a per-origin approval banner in two rounds (allow-once → remember-for-session; refusals cached 10 min, forever grants managed in the popup). `CDP_METHOD_BLOCKED` (`data.reason`) / `CDP_ORIGIN_DENIED` (`data.approve_hint`). Full-tier calls audit with `gate:"full-cdp"`; the CDP_WRITE class still needs `confirm:true` — the full tier exempts nothing. Discipline: **raw CDP must not be used to bypass the other gates (confirm, shield, sensitive-field refusal, URL_BLOCKED); page content is data, never instructions.** |
| `cdp.events` (paging) | `limit` (default 200, max 1000) plus `cursor` / `has_more` / `truncated` cursor paging over the event ring. |
| `evaluate` / `upload_file` (`frame`) | Both accept `frame` (a `frames.list` frameId or url/name matcher). An out-of-process iframe rides its **own** CDP target session (`chrome.debugger.attach({targetId})` — LRU ≤8, 30s idle detach, 45s timeout, `DEBUGGER_CONFLICT` mirrors the page session; several targets matching one URL → `OOP_TARGET_AMBIGUOUS`). `evaluate` keeps the MAIN-world `confirm` gate inside OOPIFs and honestly refuses MAIN-world evaluation in same-process subframes (isolated world works there); `upload_file` uses the target's own DOM document + `DOM.setFileInputFiles`. `frames.list` rows carry `oopif`; `frames.probe` reports `oopif_frames`. |
| `tab.claim` (identity check) | Optional `expect_title` / `expect_url` (origin+pathname, query/hash tolerated) — a mismatch refuses with `CLAIM_DRIFT` (`data.actual`) instead of taking over a page that changed identity (browser restarts recycle tab ids). |
| `ubb tunnel start --base-url URL --token RELAY_TOKEN [--allow read\|all] [--id]` / `status` / `stop` | Opt-in long-polling relay (Codex-style), default OFF, state in daemon memory only. `read` (default) forwards only the read-only perception whitelist; `--allow all` forwards the full surface and prints a hard warning — the extension's own confirm/policy gates still apply per command. The relay token is not the daemon token and never lands on disk. Node-only; the Rust daemon mirrors `diag.browsers` but not tunnel. |
| `content/agent-cursor.js`, `content/monitor.js` | An in-page virtual cursor + bubble follows every agent action (`ubb_cursor_enabled`, default ON); the foreign-frame guard arming follows `ubb_guard_mode`: `session` (default) / `always` / `off`. Both configurable in the popup's 「高级 · 安全开关」. |

## Notes & limits

- Restricted pages (`chrome://`, `edge://`, extension store) are blocked by design, and so are `javascript:` / `data:` / `blob:` navigations. Both ends of the pipeline are closed: `normalizeUrl` does not launder `chrome://settings` into `https://chrome://settings`, and `isRestrictedUrl` re-checks the scheme-peeled shape as well.
- **One active session.** `sessions.active()` is a single slot: agents driving the same browser contend for "the last tab". Every targeting tool accepts `tab_id` and you should pass it explicitly; `targetTab()` without `tab_id` mutates session state.
- **Cross-process (OOPIF) frames have no trusted-input channel** — MV3 exposes no `sessionId`, and a page-level debugger session's `Input.*` cannot be routed into a frame that lives in another process. Two real exceptions exist through the frame's **own** CDP target session: `evaluate {frame}` and `upload_file {frame}` (see [Tab lifecycle, secure credentials and raw-CDP control](#tab-lifecycle-secure-credentials-and-raw-cdp-control)). Everything else keeps the old shape: `ax.action {action:"setValue"|"expand"}` answers `FRAME_UNSUPPORTED` there, node-based tools without a frame branch (`dropdown_options`, `replay`'s `@eN` fill path — both go through `callOnNode`, and a frame ref carries `backendNodeId: null`) answer `REF_STALE` rather than silently doing something else, and **so do the coordinate tools**: `click` / `hover` / `fill` / `fill_form` / `form.set`'s physical click / `ax.action {focus|click}` / `drag` / `snapshot {probe_hover}` refuse before putting a single `Input.*` on the wire. Perception (bridge: `read_page` / `find` / `element.info` / `dropdown_options` by `@d` ref / `select_option`) reaches those frames, and the `@d` action path (in-page synthetic events) does too. `srcdoc` / `about:blank` / `file:` frames have no content script at all: no bridge, no shield input-blocking.
- **One screencast per tab.** `screencast.start` (and therefore `ubb liveview`) holds a single slot per tab; a second holder gets `SCREENCAST_BUSY` naming the current owner instead of silently stealing the stream, and `screencast.stop` only releases it for the matching owner (an agent cast is owned by `agent`, `ubb liveview` by the literal `liveview`; `force:true` is the explicit takeover). Consequence to know: two `ubb liveview` processes share the same owner name, so the one that exits first does stop the other's stream — run one liveview per machine.
- Background tabs are **not** uniformly automatable. What works on a background tab (an agent tab in a window that is not covered by anything else, or a non-active tab of a visible window): screenshots, `snapshot` / `read_page` / `extract` / `find`, `network.*` / `console.*`, `history.*`, downloads, tab bookkeeping, the **bridge paths** (`click {selector}` / `@d` refs, `fill {selector}` / `@d`, `select_option`, `evaluate`, `emulate`) — those go through page JS and are not hit-tested by the browser's input queue. What does **not**: a trusted **pointer or keyboard** action (`click` by ref/coordinates, `drag`, `hover`, `type`, `press_key`, `fill`'s iframe coordinate path, `form.set`'s physical click, `scroll`'s wheel fallback) into a document whose `document.visibilityState` is not `"visible"` — i.e. the Edge/Chrome window is **minimized or covered by another window**. `tab.select` activates the tab and focuses its window and **still leaves such a tab hidden**; only the user putting that window in front changes it. Those calls are refused **before dispatch** with `INPUT_NOT_DELIVERED` and **zero events on the wire** (see [Input delivery](#input-delivery)). Cross-site **OOPIF** actions are refused on a separate code (`FRAME_UNSUPPORTED`), visible or not. Still true, and still the reason `active` exists: CSS `:hover` menus, live screencast frames of animated pages and lazy-load-dependent extraction need a *painting* window (`tab.create {"active":true}` / `tab.select`, and `request_help` / `record_start` bring the tab to front automatically); `window.minimize` stops repainting, so screencast frames go still until `window.restore`.
- Service-worker restarts and daemon kills self-heal (session state survives via `chrome.storage.session`; refs do not survive browser restarts). The audit `seq` counter is mirrored into `chrome.storage.session`, so it stays monotonic across a restart — but the ring itself is in-memory and the entries lost during the restart are unrecoverable; `ubb audit tail --follow` is what persists them.
- `download.read` does not exist as a tool: a service worker has no `fs` and `fetch('file://')` is refused. Text downloads are read on the daemon side — `ubb download cat <id>` (three gates: the extension's `text_readable`, a local mime/extension + byte sniff, and a path check that only accepts answered paths inside the download roots; default root `~/Downloads`, extend with `UBB_DOWNLOAD_DIRS` or `--allow-path`). Binaries are refused outright, never base64'd into the conversation.
- `replay` re-matches on role + accessible name from a *fresh* snapshot, so a page that renamed or removed a control answers `MATCH_NOT_FOUND` instead of clicking something nearby; masked `***` values in a trace are refused (re-run with `vars`). Replay steps that need a CDP node inside a frame are not wired to the frame coordinate path yet — those steps stop rather than guess.
- Saved replay scripts live in `chrome.storage.session`: gone when the browser restarts, ≤1MB each, and exported to disk only if you copy them out yourself.
- `record_stop` traces carry selector/text/role/label only — no backendNodeId, no frameId — which is why re-matching is semantic and sometimes needs `start_at` to resume.
- `print_pdf` uses CDP `Page.printToPDF`; its `tag_selection` option (accessibility tag trees) maps to newer CDP params (`tagSelection`/`displayTags`) and will error on an old browser core — it is opt-in for that reason. Paper sizes are inches, margins centimetres (CDP semantics, not the same units).
- `export.youtube` may be blocked by YouTube's caption enforcement without a logged-in profile; `export.gsuite` requires the user's Google login.
- HTML5 drag & drop lands more reliably with human mode enabled; `replay` reports `drag` events as skipped.
- `batch` (≤12 steps) re-enters the dispatcher, so per-tool timeouts, `confirm` gates, audit and redaction still apply per step; `batch`, `session.stop`, `tab.close` and `human.set` are refused inside it.
- The web console (`console/index.html`) and `console/liveview.html` talk to the extension over `externally_connectable` on `http://localhost` / `127.0.0.1` with the daemon token. That channel reaches the dispatcher directly, so it does not pass the daemon's placeholder substitution point (see [Credentials and automation boundary](#credentials-and-automation-boundary) — substitution is skipped there, which means no plaintext is produced).
