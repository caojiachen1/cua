# CUA Browser — Installation

CUA Browser is part of cua, not a separate program: the bridge daemon is compiled into `cua.exe` (source in `crates/browser-bridge`), the extension is plain JS in `browser/extension`, and the only entry point is `cua browser`. There is no Node runtime to install and no second binary to keep in sync.

Prerequisites: cua on `PATH` (see the repository's `install.ps1`), and Chrome or Edge v116+.

Check that the CLI half is there before touching the browser:

```bash
cua browser status      # "daemon": true is enough to know the binary and the config are sane
```

## 1. Install and load the extension

```bash
cua browser install
```

That one command copies the extension to `%LOCALAPPDATA%\cua\browser-ext`, records `extension_dir` in `%LOCALAPPDATA%\cua\browser.json`, registers the native-messaging host (`com.cua.browser`) under the per-user Edge and Chrome registry hives (no admin needed), and prints the folder plus the next steps and the pairing token. Run it from the cua checkout (it looks for `browser/extension` next to the binary and up to three directories above it); if the sources live elsewhere, set `CUA_BROWSER_EXTENSION_DIR` first.

Then load it:

1. Open `edge://extensions` (or `chrome://extensions`).
2. Enable **Developer mode** (bottom-left on Edge, top-right on Chrome).
3. Click **Load unpacked** and select the folder that contains `manifest.json` — `%LOCALAPPDATA%\cua\browser-ext` as printed by `cua browser install`, or the checkout copy `browser\extension`. **Either works**: both origins are registered in the native-messaging manifest.
4. Pin "CUA Browser" from the extensions menu if you want the toolbar icon.

After any edit to `lib/*`, `content/*`, `background.js` or `manifest.json`, press ↻ on the extension card — the service worker and the content scripts (`content/bridge.js` all frames, `content/frameblock.js` all frames, `content/shield.js` top frame only) restart with it. `content/site-rules.json` does **not** need this: see [Site rules](#5-site-rules-json-not-code).

Then prove the reload actually took, rather than assuming it: `cua browser status` reports `extension_build`, which must equal the `BUILD` constant in `browser/extension/lib/config.js` (currently `2026.11.18-cua-browser.2`). A click that does nothing or a `SECRET_FETCH_TIMEOUT` is more often "the extension answering me is older than the code I just read" than a bug — and the other half is the daemon: **it keeps its code in memory too**, so a daemon started days ago runs days-old logic no matter how fresh your checkout is.

## 2. Pair — no token copying

`cua browser install` already registered the native-messaging host, so pairing is automatic: at every extension startup the browser launches `cua.exe` in native-host mode, the host answers with the token from `%LOCALAPPDATA%\cua\browser.json` (generating one on a fresh machine) and ensures the daemon is running. The extension auto-pairs, and if the token ever rotates it self-heals on the next service-worker start.

```bash
cua browser status           # auto-starts the daemon; expect "extension": true
```

Manual fallback if the browser does not launch the native host: the popup shows the same token, and `extension_dir` / `port` / `token` live in `%LOCALAPPDATA%\cua\browser.json` (environment overrides: `CUA_BROWSER_EXTENSION_DIR`, `CUA_BROWSER_PORT`, `CUA_BROWSER_TOKEN`; `CUA_DATA_DIR` moves the whole data directory).

**What happens on first use**: an agent call *without* a browser name (`cua browser snapshot`) targets the **real logged-in browser** — slot `default` by default. If that browser is not running, the daemon starts its **normal profile minimized** (no `--user-data-dir`, no `--load-extension`) and waits up to `user_browser_wait_ms` for this extension to reconnect, so the existing cookies and logins are used. It does **not** create an isolated profile for a nameless call. Use `cua browser browser.launch '{"name":"scratch"}'` for an isolated workspace, or `auto_workspace:true` to restore workspace-first routing. Routing keys in `%LOCALAPPDATA%\cua\browser.json`: `user_browser` (slot name), `auto_open_user_browser` (false disables the auto-start), `user_browser_wait_ms`, `user_browser_exe` (defaults to auto-detected Edge/Chrome); the workspace keys remain `agent_browser`, `auto_workspace` (now default `false`), `workspace_wait_ms`.

To uninstall: delete `%LOCALAPPDATA%\cua\browser-ext`, remove the extension from the browser, and delete the native-messaging registry entries `cua browser install` wrote (`HKCU\Software\Microsoft\Edge\NativeMessagingHosts\` and `HKCU\Software\Google\Chrome\NativeMessagingHosts\` — the only two hives `cua browser install` writes).

## 3. Verify

```bash
cua browser status
# expect: "daemon": true, "extension": true, "extension_build": "2026.11.18-cua-browser.2"
# same proof straight from the daemon, no CLI at all:
curl -s http://127.0.0.1:8765/health   # → {"ok":true,"extension":true,"extension_build":"2026.11.18-cua-browser.2",...}
```

To prove the extension really picked up the code you just edited (not *something* connected): `cua browser status` **or** `curl http://127.0.0.1:8765/health` (port from `CUA_BROWSER_PORT` / `%LOCALAPPDATA%\cua\browser.json`) must report `extension_build` **equal to the `BUILD` constant in `browser/extension/lib/config.js`**. `extension: true` on its own only proves *a socket is up*, never that it is *this* code. Only the **extension** needs the ↻ for an extension change; a daemon-side change (the Rust bridge) needs the daemon restarted — `cua browser stop`, then the next call starts the current `cua.exe`.

If `extension: false`, the popup token and the token in `browser.json` differ — re-run `cua browser install` (it re-copies the extension and rewrites the registration), or copy the token shown in the popup into `browser.json` yourself. If the daemon is not running, `cua browser status` starts it (`cua browser serve` is the detached mode it spawns).

If `extension_build` is **missing from the output**, nothing has completed the WS `hello` yet: check that the extension is loaded and enabled, then look at the daemon's startup line for its start time. If it is present but **stale**, the extension was never reloaded (step 1's ↻).

## 4. First run

```bash
cua browser session.start
cua browser tab.create '{"url":"example.com"}'
cua browser snapshot
cua browser screenshot '{}' --out shot.png
cua browser session.stop
```

## 5. Site rules: JSON, not code

Page extraction for github / zhihu / juejin / wechat / xiaohongshu / x.com lives in `content/site-rules.json` and is executed by the rule interpreter inside `content/bridge.js`. Adding or fixing a site is a **data change**: validate, store, reload — no code edit, no extension reload.

```bash
cua browser site.rules.validate '{"rules":[{"id":"github","match":{"hosts":["github.com"]},"blocks":[…]}]}'
cua browser site.rules.add '{"rules":[…],"confirm":true}'   # → chrome.storage.local key "cuaSiteRules"
cua browser site.rules.remove '{"id":"my-rule","confirm":true}'   # drop one user rule
cua browser site.rules.clear '{"confirm":true}'              # drop the whole user layer
cua browser site.rules.reload                                # re-assemble the service-worker cache
cua browser site.rules.list                                  # effective set, in match order (user | builtin)
```

User rules merge ahead of the built-ins and win on the same `id`. Anything executable in a rule (`eval`, `script`, `function`, `on*`, `html`, `url`, `src`, …) is refused by validation, so a rule can never become code. `site.rules.remove` drops one user rule; `site.rules.clear` drops the whole user layer. Both require `confirm:true`.

## 6. Credentials: the user holds the value

There are two paths, and neither one puts a plaintext into a tool argument:

- **`secret.request` — the secure channel (preferred).** The agent describes the fields (`ref`, `label`, `type`), the popup shows a 「安全凭据」 form, and the **user** types the values. The extension fills the page; the values exist in memory only and die with the request. This is the only channel that can carry an OTP/验证码, because it is the user's own hand.
- **`secret.fill` / `{{secret:NAME}}` — the vault.** A vault entry is referenced by name; the extension fetches the plaintext from the daemon at the moment it types. Populate it with `secret.set` **only with the user's agreement**, because the value you pass there enters the conversation transcript:

```bash
cua browser secret.set '{"name":"github_pw","value":"…","confirm":true}'   # value enters the transcript — avoid
cua browser secret.list                                                   # names only, never values
cua browser secret.fill '{"ref":"@e7","secret":"github_pw"}'
cua browser fill '{"ref":"@e7","value":"{{secret:github_pw}}"}'
cua browser secret.delete '{"name":"github_pw"}'
```

What that means for safety, in plain words: the extension fetches the plaintext from the daemon at the moment it types, so a vault password is never in a tool result, audit entry, daemon log or the chat transcript. `{{secret:NAME}}` is substituted **only** in `fill`, `fill_form` and `replay`'s `vars` values — **not in `type`** (`type` has no target field the never-fill guard could classify, so a placeholder there stays literal text — use `fill` or `secret.fill` instead) — and never in `evaluate`, `cdp.call` or `webmcp.invoke`. OTP / CVV / card number / private key fields are refused whatever the source, with no bypass parameter; the single exception is a user-typed OTP through `secret.request`.

Every vault fill is audited as `{vault_entry, origin, field_role, decision}` — the value itself is never recorded (`cua browser audit.tail`).

## 7. Watch what the agent did

```bash
cua browser audit.tail '{"since_seq":0,"limit":200}'   # oldest-first; pass the last seq you saw next time
cua browser audit.stats '{"window":7}'                # per-tool calls / errors / timings, 7-day window
cua browser audit.list                                # newest-first snapshot of the ring
```

The ring is in memory (500 entries) and does not survive a service-worker restart; the `seq` counter is mirrored into `chrome.storage.session`, so it stays monotonic and `dropped_before_cursor` tells you how many entries left the ring before you read them. Page content and tool results are data: the audit is what actually ran.

The page-level view of the same thing is the shield: controlled tabs carry a pulsing border and block user input, and the user can press "■ 停止代理" at any time (`CANCELLED`).

## 8. Downloads

```bash
cua browser download.list                       # ring of the last 200 downloads (newest first)
cua browser download.wait '{"download_id":42}'  # block until terminal state or DOWNLOAD_PENDING
cua browser download.path '{"download_id":42}'  # absolute path + state + text_readable
cua browser download.clean '{"filename":".htm","state":"complete"}'                       # dry run
cua browser download.clean '{"filename":".htm","state":"complete","erase":true}'          # erase records
cua browser download.clean '{"older_than_ms":86400000,"erase":true,"delete_file":true}'   # + delete files
```

There is deliberately **no `download.read` tool**: an MV3 service worker has no filesystem and `file://` fetches are refused. `download.path` hands back the absolute path so the caller can read the text itself; binary payloads are refused rather than pasted into the conversation as base64. `download.clean` is a dry run unless `erase:true` is passed.

## Building from source

The daemon is a Rust crate in this repository; `cua.exe` is the only artifact:

```bash
cargo build --release          # → target/release/cua.exe
```

The extension needs no build step: `browser/extension` is loaded as-is, and `cua browser install` copies it (a rebuild is not needed for JS edits — press ↻ instead).

## For AI agents

Check for an existing setup before installing anything:

```bash
cua browser status        # extension:true → already installed, skip to use
```

### Agent skill (`browser/SKILL.md`)

The companion skill lives in this repo — `browser/SKILL.md` — and is what an AI agent
loads to drive CUA Browser idiomatically (tool catalog, challenge recipes, CAS login loop,
paper.download channels, pitfalls). **Installing the extension does not auto-create it**;
copy it into the agent's skill directory once:

```bash
mkdir -p ~/.agents/skills/cua-browser
cp browser/SKILL.md ~/.agents/skills/cua-browser/SKILL.md
```

The repo copy is the source of truth: whenever `lib/tools-*.js` adds or changes tools,
update `browser/SKILL.md` in the same commit, then re-copy it to the skill directory
(the installed copy is NOT auto-synced). Agent-facing pitfall notes live in the same
file; long-form war stories live in the consuming project's own docs.

If not installed, ask the user to run `cua browser install` and press the browser-UI buttons, then verify with `cua browser status`. Never load the extension or paste tokens on the user's behalf without asking.

Token and port resolve in this order: `CUA_BROWSER_TOKEN` / `CUA_BROWSER_PORT` env → `%LOCALAPPDATA%\cua\browser.json` (written by `cua browser install`).

After your first real task, one thing needs the user: new tools only exist once the extension has been reloaded (step 1's ↻). `cua browser list` against a stale extension shows the old catalog. Confirm the reload landed with `cua browser status` → `extension_build` matching `browser/extension/lib/config.js`'s `BUILD`; daemon-side changes need `cua browser stop` so the next call starts the current binary.

Two reporting rules for agents working with this browser: a green mock is **not** a live result, and an action that returns `ok:true` is not evidence that the page received anything — assert the *effect*, and if you cannot, say "not yet confirmed live" instead of "verified".

## Daemon lifecycle

```bash
cua browser status      # health; starts the daemon when it is down
cua browser stop        # stop the daemon
cua browser serve --port <n> --token <t>   # what status/calls spawn detached — rarely typed by hand
```

Any `cua browser` tool call auto-starts the daemon if it is down. The extension reconnects automatically after browser restarts, extension reloads, or daemon restarts.

**Nothing but a restart makes daemon code run.** A daemon is judged by its version only from the moment it boots, and an extension only picks up `lib/*` / `content/*` / `background.js` edits on ↻. `cua browser status` is the one command that separates the two: `daemon` says the port answered, `extension` says a socket is up, and the **pair of version fields** says whether either of them is the code you are reading. When a live result contradicts the source, check the versions before believing either.

## MCP clients (Claude Desktop, IDE agents)

Any MCP-capable client can drive this browser without Node. Register the stdio
server once (the daemon auto-starts, the catalog is fetched live from the
extension):

```json
{
  "mcpServers": {
    "cua-browser": {
      "command": "C:\\path\\to\\cua.exe",
      "args": ["browser", "mcp"]
    }
  }
}
```

Tool names map `.` → `__` (`tab.create` → `tab__create`); screenshot results come
back as image content. All confirm/policy gates apply to MCP calls exactly as
they do to CLI calls.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `extension: false` | The popup token and `browser.json` differ (or the extension is not loaded) — re-run `cua browser install`, reload the extension, check the popup says "connected" |
| `cua browser status` prints **no** `extension_build` | No extension has completed the `hello` yet — verify it is loaded and enabled in `edge://extensions`; if it says `extension: true` but the build is stale, press ↻ |
| `extension_build` ≠ the `BUILD` in `browser/extension/lib/config.js` | The extension was never reloaded — press ↻ on the extension card (step 1). `extension: true` only means *something* is connected |
| `EXTENSION_NOT_CONNECTED` on calls | Browser closed, or extension not loaded — run `cua browser status` |
| `SHIELD_PASS_FAILED` | The shield's overlay would not move aside for that frame at that point, so the action was **aborted and nothing was sent** (this is the fix for the old "`ok:true` but the page did nothing"). Do not blind-retry. Exits, in order: the stop button (■) in the page's top-right shield strip → popup shield mode `block`→`border`→`off` → reload the extension → `tab.release` + `tab.claim` |
| Click/type/press_key does nothing **and the call fails fast with `INPUT_NOT_DELIVERED`** (`data.stage:"pre-dispatch"`, `dispatched:false`, `events_on_wire:0`) | **Not a bug — it is this machine's input-queue behaviour.** The target document's own `document.visibilityState` is not `"visible"`: the Edge/Chrome window is **minimized or covered by another window**. A hidden document never drains its trusted-input queue, so a pointer/real-keyboard action (`click` by ref/coordinates, `hover`, `drag`, `type`, `press_key`, `fill`'s iframe coordinate path, `form.set`'s physical click, `scroll`'s wheel fallback) is refused **before dispatch** instead of waiting out the proof budget. **`tab.select` does not fix it** — it activates the tab and focuses the window, but an occluded window stays `hidden`; `window.restore` does not raise a covered window either. **Only the user raising that Edge/Chrome window to the front changes it.** The other real exits: `tab.create {"url":…,"active":true}` (a foreground tab), or the `@d<frameId>.<n>` / `{selector}` bridge path — but that is `isTrusted:false` in-page synthetic input and is **not semantically equivalent** to a real click. The second origin — **no `stage`, `waited_ms≈7000`** — means the events did go out but the document's delivery ring recorded none; a press/release/key is **never** auto-repeated, so decide whether a repeat is safe rather than blind-retrying. Sensing, screenshots, extraction, network/console, downloads and the bridge paths keep working on a background tab — only trusted pointer/keyboard input is refused. (See README's [Input delivery](README.md#input-delivery) and [Notes & limits](README.md#notes--limits).) |
| `BRIDGE_TIMEOUT` / `BRIDGE_WEDGED` | That frame's content bridge stopped answering; the bound is per frame (12 s / 30 s for heavy reads), then a **30 s** cooldown during which the call first pays an 800 ms `selfInfo` liveness probe: no answer → sub-second `BRIDGE_WEDGED` (window re-arms), answer → cooldown dropped and the call runs, so a healed frame needs no navigation. The guarantee is the **cost bound per `(tab,frame)`**, not that a sibling frame is unaffected — each frame is judged by its own probe. Clear a pending dialog (`dialog.handle`) or reload the tab; navigation and `tab.close` clear the record |
| `SECRET_FETCH_TIMEOUT` on `secret.fill` | The daemon did not answer the vault fetch in time — restart it (`cua browser stop`), then re-check |
| `REF_STALE: … gave no runtime handle` on a `{ref}` action | `DOM.resolveNode` handed back nothing for that node — take a fresh snapshot; frame-scoped and shadow-tree targets go through the bridge/coordinate path instead |
| `DEBUGGER_CONFLICT` | A DevTools window or another automation extension holds the tab |
| `TAB_NOT_OWNED` | Only session tabs are operable — create with `tab.create` or claim with `tab.claim` |
| `URL_BLOCKED` | Restricted page (chrome://, edge://, store) — by design |
| `BAD_URL` | Not a URL with a scheme and not a bare host — CUA Browser refuses to guess `https://` for odd shapes (that would launder `chrome://` past the gate) |
| Daemon port conflict | Change `port` in `%LOCALAPPDATA%\cua\browser.json` (or set `CUA_BROWSER_PORT`) and restart the daemon |
| `SECRET_UNKNOWN` | The vault has no entry with that name — `cua browser secret.list`; the message never contains a value |
| `SECRET_ORIGIN_DENIED` | The entry is bound to a host and this page does not match it — open the bound site, or use `secret.request` and let the user type it |
| `SECRET_UNRESOLVED` | A `{{secret:…}}` reached execution unresolved — the fill is refused; check the name and that the value went through `vars` / `fill` / `fill_form` (`type` never substitutes — the placeholder stays literal there) |
| `FRAME_UNSUPPORTED` | That frame is out-of-process (cross-site OOPIF): MV3 gives it no CDP input channel, **and a page-level debugger session's `Input.*` does not reach it either — so the coordinate tools refuse as well**. `click` / `hover` / `fill` / `fill_form` / `form.set`'s physical click / `ax.action {focus\|click}` / `drag` / `snapshot {probe_hover}` all answer this code **before** putting any `Input.*` on the wire; `ax.action {setValue\|expand}` too — but `evaluate {frame}` / `upload_file {frame}` are the two real exceptions that DO work there (they ride that frame's own CDP target session). Exits: act through that frame's own bridge with an `@d<frameId>.<n>` ref (`find {"frame_id":N}` produces them — in-page synthetic events), let the user click, or `tab.create` the frame URL as a top-level tab. Sensing (`read_page` / `find` / `element.info`) and same-process child frames (cross-origin, same site) are unaffected — the coordinate path lands there for real |
| `FRAME_UNREACHABLE` | No content bridge in that frame (`srcdoc:` / `about:blank` / `file:`) — manifest-level change needed; re-snapshot and work from the parent frame |
| `FRAME_AMBIGUOUS` / `REF_FRAME_AMBIGUOUS` | Two frames look alike — CUA Browser refuses to guess; pass `frame_id` explicitly |
| `SCREENCAST_BUSY` | Another client holds the tab's single cast (name is in the message) — let it stop, or take over with `force:true` |
| `DOWNLOAD_PENDING` | The transfer has not reached a terminal state — poll `download.status`, or `download.wait` with a bigger `timeout_ms` |
| `MATCH_NOT_FOUND` during replay | The page drifted from the recording — the message lists the 3 closest candidates; fix the trace, or resume with `start_at` |
