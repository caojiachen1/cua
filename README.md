# cua — Windows computer-use CLI

Rust CLI for driving Windows desktop apps: UIA accessibility tree + screenshots, a11y-first
actions, real `SendInput`, message-level background input, a hidden-desktop sandbox for true
background control, an isolated CDP browser, and JSON action receipts with safety guards.

## Install

Needs Windows 10/11 x64, Rust (MSVC), PowerShell 5.1+. Browser control needs Edge or Chrome.

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
cua set-value --window Notepad --index 6 --value hi    # Value > RangeValue > Legacy
cua click --window Notepad --index 4 --right           # right/double always use real events
cua toggle --window App --index 3                      # also: select / expand / collapse / set-range
cua type --window App --text hi --background           # message-level, focus stays put
cua wait --window "title:Report" --gone                # also: hover / cursor / resize / restore
cua sandbox start --exe D:\app.exe                     # run and control on an invisible desktop
cua launch --exe "C:\Windows\System32\msinfo32.exe" --cwd D:\    # env sanitized by default
cua browser open https://example.com
cua browser click --text "Sign in"
```

Selectors: title substring, `title:Exact`, `process:name.exe`, `class:X`, `pid:N`, hwnd.
Coordinate actions need a fresh `--shot-id` from the observed window (screen captures cover
any point). Every action prints `cua-action-receipt-v1`; exit 0 ok, 1 failed, 2 refused.
`launch` strips `ELECTRON_*`/`NODE_*`/crashpad variables so Electron apps do not start as bare
Node and exit silently; `--no-clean-env` inherits them. Electron/Chromium UIs need
`--args "--force-renderer-accessibility"` before their DOM shows up in the UIA tree.

Never automated: terminals, task manager, registry, `mmc`, Control Panel, password managers,
Windows Settings (including `ms-settings:` URIs), and the Win key.

## Tests

```powershell
powershell -ExecutionPolicy Bypass -File tests\run-all.ps1    # 242 assertions, 4 suites
```

Suites cover the CLI surface, deep behaviours (unicode input, drag, capture, error paths),
control patterns and geometry, browser tabs, and stock Windows apps (Notepad, Character
Map, System Information, Volume Mixer, Explorer).

## Known limits

The sandbox has no real `SendInput` (OS binds input to the input desktop), so it drives apps
through UIA patterns and window messages; UWP apps cannot start there, and DRM-protected
windows refuse `PrintWindow`. Message-level input reaches classic controls
(EDIT/RICHEDIT/BUTTON) but not XAML/DirectUI surfaces — use the a11y actions for those.
The browser uses its own profile and port 9334 and never touches your browser session.
