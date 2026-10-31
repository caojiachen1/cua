# cua — Windows computer-use CLI

Rust CLI for driving Windows desktop apps: UIA accessibility tree + screenshots, a11y-first
actions, real `SendInput`, message-level background input, a hidden-desktop sandbox for true
background control, browser control bridged to UBB, screen recording, and JSON action
receipts with safety guards.

## Install

Needs Windows 10/11 x64, Rust (MSVC), PowerShell 5.1+. Browser tasks need the `ubb` CLI and
its Edge extension connected (`cua browser status` → `extension: true`).

```powershell
git clone https://github.com/caojiachen1/cua.git
cd cua
powershell -ExecutionPolicy Bypass -File install.ps1    # build -> bin\ -> skill -> smoke test
```

Uninstall: `powershell -ExecutionPolicy Bypass -File uninstall.ps1`.

## Usage

```powershell
cua apps                                               # windows + foreground window
cua state --window Notepad --patterns                  # UIA tree + screenshot + state_id
cua state --window "VSCode" --depth 22 --raw           # raw view: IsControlElement=false nodes too
cua element-at --x 500 --y 238                         # which element is at this point? (passive)
cua set-value --window Notepad --index 6 --value hi    # Value > RangeValue > Legacy
cua click --window Notepad --index 4 --right           # right/double always use real events
cua toggle --window App --index 3                      # also: select / expand / collapse / set-range
cua grid --window App --index 3 --row 2 --col 1         # GridPattern: address a table cell
cua transform --window App --index 0 --move-to 400,300  # move a window with no mouse
cua type --window App --text hi --background           # message-level, focus stays put
cua type --window App --text hi --background --index N # aim at one field in a multi-edit window
cua wait --window "title:Report" --gone                # also: hover / cursor / resize / restore
cua wait --window App --index 6 --value "Ready"        # element-level wait (toggle/selected/expanded/enabled)
cua text --window Notepad --index 6 --max-chars 80     # TextPattern range (bounded)
cua scroll-item --window App --index 9 --align-top     # ScrollItemPattern into view
cua table --window Explorer --index 28                  # Table column headers
cua find-item --window Explorer --index 28 --property name --value file.txt
cua select-add --window App --index 4                   # multi-select: also select-remove
cua ocr --window Notepad                                # local Windows OCR; also --screen/--file
cua ocr --file "$env:TEMP\shot.png" --max-lines 50
cua sandbox start --exe D:\app.exe                     # run and control on an invisible desktop
cua launch --exe "C:\Windows\System32\msinfo32.exe" --cwd D:\    # env sanitized by default
cua rec start --fps 2 --out session                    # record the screen while you work
cua rec stop                                           # -> mp4 with ffmpeg, else an HTML player
cua rec cancel                                         # force-stop a stuck recorder, keep frames
cua browser tab.create '{"url":"https://example.com"}'  # bridged to UBB (the user's Edge)
cua browser find '{"query":"Sign in"}'                   # -> @ref
cua browser click '{"ref":"@d1"}'
```

Selectors: title substring, `title:Exact`, `process:name.exe`, `class:X`, `pid:N`, hwnd.
Coordinate actions need a fresh `--shot-id` from the observed window (screen captures cover
any point); this includes sandbox scroll/drag at explicit coordinates. Every action prints
`cua-action-receipt-v1`; exit 0 ok, 1 failed, 2 refused. Set `CUA_DATA_DIR` to keep a
session's config, cache, sandbox state and recordings out of `%LOCALAPPDATA%\cua`.
`launch` strips `ELECTRON_*`/`NODE_*`/crashpad variables so Electron apps do not start as bare
Node and exit silently; `--no-clean-env` inherits them. Electron/Chromium UIs need
`--args "--force-renderer-accessibility"` before their DOM shows up in the UIA tree.

UIA parse completeness: `state` flags `truncated` (budget hit) and `errors`/`last_error`
(walker failures that survived one retry); Chromium's "S_OK but null child" answers count as
clean branch ends. Deep container chains (VSCode and other Electron apps) need `--depth 22+`.
When a runtime id goes stale, actions re-locate the element by its cached attributes
(automation id / name / class / type / parent / sibling position) with an ambiguity margin —
ambiguous matches are refused, not guessed — and record an old→new rid override.
Core properties ride a UIA cache request bundled into the walk (~1.6× faster; `CUA_UIA_NO_CACHE=1`
opts out). Every command accepts `--observation-id <id>` to resolve `--index` against that
immutable snapshot — a wrong id fails instead of silently re-observing. Screenshots report a
graded `status` (`captured` / `suspected_black` / `no_pixels` / `capture_failed`).

Never automated: terminals, task manager, registry, `mmc`, Control Panel, password managers,
Windows Settings (including `ms-settings:` URIs), and the Win key.

## Tests

```powershell
powershell -ExecutionPolicy Bypass -File tests\run-all.ps1    # 8 suites, 372 passed + 1 env skip
powershell -ExecutionPolicy Bypass -File tools\uia-probe.ps1 -Apps qqmusic,cloudmusic  # capability report
```

Suites cover the CLI surface, deep behaviours (unicode input, drag, capture, error paths),
control patterns and geometry, the UBB browser bridge, stock Windows apps (Notepad, Character
Map, System Information, Volume Mixer, Explorer) and third-party desktop apps (QQ Music,
NetEase Cloud Music, Soda Music — skipped when not installed), and a long-horizon
Bilibili scenario that is screen-recorded end to end (`cua rec`).

`tools/uia-probe.ps1` is a separate diagnostic: it launches an app, measures the UIA tree
(provider size, name / rectangle / runtime-id coverage, pattern histogram), attempts one
safety-filtered pattern action with read-back, probes message-level input and capture, then
classifies the app as `NO_PROVIDER` / `READ_ONLY` / `A11Y_RECT_ONLY` / `MSG_LEVEL_ONLY` /
`A11Y_PARTIAL` / `A11Y_FULL` / `NEEDS_REAL_INPUT` / `CAPTURE_BLACK`. `-SelfTest` checks the
ladder without touching any app.

## Known limits

The real-app suites need a quiet desktop: they click at screen coordinates, so an unrelated
full-screen window (a music client left open, say) can swallow the click. Run them with no
third-party app occupying the screen.
The hidden desktop drives apps through UIA patterns and window messages; UWP apps cannot start
there, and DRM-protected windows refuse `PrintWindow`. Message-level input reaches classic
controls (EDIT/RICHEDIT/BUTTON) but not XAML/DirectUI surfaces — use the a11y actions for those.
Browser control is a bridge: it needs the `ubb` daemon + extension, works on the user's real
profile, and never opens a window of its own. `file://` pages need explicit file access on
the extension; screenshotting a background tab needs `tab.select` first.
