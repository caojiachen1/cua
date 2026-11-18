//! edge.rs — an independent, persistent Chromium profile for the child session.
//!
//! The user's own Edge/Chrome profile can only be opened by one desktop at a
//! time. `cua edge` therefore gives the child session its own `CuaEdge` (or
//! `CuaChrome`) profile, launches a real browser window inside the session that
//! runs the command, and refuses to start when another Windows session already
//! holds that profile.
//!
//! Edge is preferred when both are installed; `--browser edge|chrome` forces one
//! and `--browser auto` (the default) keeps using whatever kind created the
//! profile, so a machine never silently switches browser mid-profile.
//!
//! First version deliberately uses the existing UIA/screenshot/real-input
//! commands (`cua child state/click/set-value/...`) instead of raw CDP or the
//! browser extension. `cua browser` is the bridge to the user's own browser.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_MORE_DATA, UNICODE_STRING};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::RestartManager::{
    RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources, RmStartSession,
};
use windows::Win32::System::Threading::{
    CreateProcessW, OpenProcess, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_DEFBUTTON1, MB_ICONWARNING, MB_YESNO, SW_SHOWMINNOACTIVE,
};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};

use crate::util::{self, Receipt};

const PROFILE_SCHEMA: &str = "cua-browser-profile-v1";
const STATUS_SCHEMA: &str = "cua-edge-status-v1";
const MAX_URLS: usize = 8;
const WAIT_STEP: Duration = Duration::from_millis(150);

/// What the user asked for on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum BrowserPref {
    /// Use whatever browser created the profile, else Edge, else Chrome
    Auto,
    Edge,
    Chrome,
}

impl BrowserPref {
    fn resolve(self) -> (BrowserKind, Option<PathBuf>) {
        match self {
            BrowserPref::Edge => (BrowserKind::Edge, locate(BrowserKind::Edge)),
            BrowserPref::Chrome => (BrowserKind::Chrome, locate(BrowserKind::Chrome)),
            BrowserPref::Auto => {
                // Keep the kind that created the recorded profile: switching
                // browsers would point the user at an empty new profile.
                if let Some(meta) = read_metadata() {
                    if let Some(kind) = BrowserKind::from_str(&meta.browser) {
                        if Path::new(&meta.browser_exe).is_file() {
                            return (kind, Some(PathBuf::from(&meta.browser_exe)));
                        }
                    }
                }
                match locate(BrowserKind::Edge) {
                    Some(p) => (BrowserKind::Edge, Some(p)),
                    None => (BrowserKind::Chrome, locate(BrowserKind::Chrome)),
                }
            }
        }
    }
}

/// The browser that will actually be launched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserKind {
    Edge,
    Chrome,
}

impl BrowserKind {
    fn as_str(self) -> &'static str {
        match self {
            BrowserKind::Edge => "edge",
            BrowserKind::Chrome => "chrome",
        }
    }

    fn label(self) -> &'static str {
        match self {
            BrowserKind::Edge => "Edge",
            BrowserKind::Chrome => "Chrome",
        }
    }

    fn exe_name(self) -> &'static str {
        match self {
            BrowserKind::Edge => "msedge.exe",
            BrowserKind::Chrome => "chrome.exe",
        }
    }

    fn profile_dir(self) -> &'static str {
        match self {
            BrowserKind::Edge => "CuaEdge",
            BrowserKind::Chrome => "CuaChrome",
        }
    }

    fn app_path_keys(self) -> [&'static str; 2] {
        match self {
            BrowserKind::Edge => [
                "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\msedge.exe",
                "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths\\msedge.exe",
            ],
            BrowserKind::Chrome => [
                "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\chrome.exe",
                "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths\\chrome.exe",
            ],
        }
    }

    fn install_subdir(self) -> &'static str {
        match self {
            BrowserKind::Edge => "Microsoft\\Edge\\Application",
            BrowserKind::Chrome => "Google\\Chrome\\Application",
        }
    }

    fn env_var(self) -> &'static str {
        match self {
            BrowserKind::Edge => "CUA_EDGE_EXE",
            BrowserKind::Chrome => "CUA_CHROME_EXE",
        }
    }

    fn config_key(self) -> &'static str {
        match self {
            BrowserKind::Edge => "edge_exe",
            BrowserKind::Chrome => "chrome_exe",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "edge" | "msedge" => Some(BrowserKind::Edge),
            "chrome" => Some(BrowserKind::Chrome),
            _ => None,
        }
    }

    fn is_browser_name(name: &str) -> bool {
        name.eq_ignore_ascii_case("msedge.exe") || name.eq_ignore_ascii_case("chrome.exe")
    }
}

/// What the user decided about the browser's first-run import of the main
/// profile's passwords/history into a brand-new cua profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportConsent {
    Yes,
    No,
    /// Nothing recorded yet; the caller decides (prompt, or default to Yes).
    Unrecorded,
}

impl ImportConsent {
    fn as_str(self) -> &'static str {
        match self {
            ImportConsent::Yes => "yes",
            ImportConsent::No => "no",
            ImportConsent::Unrecorded => "unrecorded",
        }
    }

    fn from_meta(meta: Option<&ProfileMeta>) -> Self {
        match meta.and_then(|m| m.local_import_consent.as_deref()) {
            Some("yes") => ImportConsent::Yes,
            Some("no") => ImportConsent::No,
            _ => ImportConsent::Unrecorded,
        }
    }

    /// An explicit flag always wins; otherwise the recorded decision; otherwise
    /// undecided.
    fn resolve(flag: Option<bool>, meta: Option<&ProfileMeta>) -> Self {
        match flag {
            Some(true) => ImportConsent::Yes,
            Some(false) => ImportConsent::No,
            None => Self::from_meta(meta),
        }
    }
}

/// Map the two mutually exclusive CLI flags onto the tri-state consent value.
pub fn consent_flag(import: bool, no_import: bool) -> Option<bool> {
    if import {
        Some(true)
    } else if no_import {
        Some(false)
    } else {
        None
    }
}

pub struct EdgeOptions {
    pub urls: Vec<String>,
    pub fresh_profile: bool,
    pub profile: Option<String>,
    pub automation: bool,
    pub cdp: bool,
    pub status: bool,
    pub wait_ms: u64,
    pub browser: Option<BrowserPref>,
    pub import_passwords: Option<bool>,
    pub pair: bool,
}

/// `cua child edge ...` entry point. The child agent executes `cua edge ...`
/// inside the child session, so everything here runs in the target desktop.
pub fn cmd_edge(opts: EdgeOptions) -> Result<i32> {
    let (kind, edge) = opts.browser.unwrap_or(BrowserPref::Auto).resolve();
    let profile = resolve_profile(kind, opts.profile.as_deref(), opts.fresh_profile);
    let wait_ms = opts.wait_ms.min(300_000);

    if opts.status {
        if !opts.urls.is_empty() {
            let r = Receipt::new("edge").refused(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
                &format!(
                    "`edge --status` does not launch {}; remove URL arguments",
                    kind.label()
                ),
            );
            return emit(r, launch_extra(kind, edge.as_deref(), &profile, &scan_profile(&profile), None, false, false, false));
        }
        return print_status(kind, edge.as_deref(), &profile, opts.fresh_profile);
    }

    if let Err(e) = validate_urls(&opts.urls) {
        let r = Receipt::new("edge").refused(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            &e,
        );
        return emit(r, launch_extra(kind, edge.as_deref(), &profile, &scan_profile(&profile), None, false, false, opts.automation));
    }

    let Some(edge) = edge else {
        let r = Receipt::new("edge").fail(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            &format!(
                "{} not found. Checked ${}, config `{}`, App Paths and the standard install paths",
                kind.exe_name(),
                kind.env_var(),
                kind.config_key()
            ),
        );
        return emit(r, launch_extra(kind, None, &profile, &scan_profile(&profile), None, false, opts.fresh_profile, opts.automation));
    };

    let current = current_session();
    let (before, refusal) = inspect_profile(&profile, current);
    if let Some(detail) = refusal {
        let r = Receipt::new("edge").refused(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        return emit(r, launch_extra(kind, Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
    }

    let same: Vec<&Holder> = before
        .holders
        .iter()
        .filter(|h| h.is_browser && h.session == current)
        .collect();
    let already_running = !same.is_empty();
    let cdp_port = if opts.cdp {
        match pick_free_port() {
            Some(p) => Some(p),
            None => {
                let r = Receipt::new("edge").fail(
                    json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                    "could not reserve a localhost port for the dedicated CDP endpoint",
                );
                return emit(r, launch_extra(kind, Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
            }
        }
    } else {
        None
    };

    // A brand-new profile is the only place Edge/Chrome imports the main
    // profile's passwords/history; `--no-import-passwords` wipes that import
    // before the real window opens.
    let new_profile = !profile.exists();
    let recorded = read_metadata();
    let consent = match ImportConsent::resolve(opts.import_passwords, recorded.as_ref()) {
        ImportConsent::Unrecorded => ImportConsent::Yes,
        decided => decided,
    };
    let purge_import = new_profile && consent == ImportConsent::No;

    // `--pair` registers this profile as a browser slot: mint the one-shot pair
    // URL here and let the browser open it as its first tab.
    let mut pair: Option<BrowserPair> = None;
    if opts.pair {
        match pair_profile(kind) {
            Ok(p) => pair = Some(p),
            Err(e) => {
                let r = Receipt::new("edge").fail(
                    json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                    &format!("{e:#}"),
                );
                return emit(r, launch_extra(kind, Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
            }
        }
    }

    let launched = if purge_import {
        launch_without_import(
            &edge,
            &profile,
            opts.automation,
            cdp_port,
            &opts.urls,
            pair.as_ref(),
        )
    } else {
        spawn_browser_with(
            &edge,
            &profile,
            opts.automation,
            cdp_port,
            &opts.urls,
            pair.as_ref(),
        )
        .map(|pid| (pid, Vec::new(), 0usize))
    };
    let (launched_pid, purged, stopped) = match launched {
        Ok(v) => v,
        Err(e) => {
            let r = Receipt::new("edge").fail(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                &format!("{e:#}"),
            );
            return emit(r, launch_extra(kind, Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
        }
    };

    if !opts.fresh_profile {
        let _ = write_metadata(&profile, kind, &edge, consent);
    }

    let mut after = scan_profile(&profile);
    let mut holder: Option<Holder> = if already_running {
        None
    } else {
        holder_in(&after, current).cloned()
    };

    if !already_running && wait_ms > 0 {
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        while holder.is_none() {
            if holder_outside(&after, current).is_some() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(WAIT_STEP);
            after = scan_profile(&profile);
            holder = holder_in(&after, current).cloned();
        }
    }

    if let Some(h) = holder_outside(&after, current) {
        let h = h.clone();
        let detail = format!(
            "{} was spawned (PID {launched_pid}), but the {} profile was taken by \
             session {} (PID {}). Close that {} window and retry.",
            kind.label(),
            kind.profile_dir(),
            h.session,
            h.pid,
            kind.label()
        );
        let r = Receipt::new("edge").fail(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        let extra = launch_extra(kind, Some(edge.as_path()), &profile, &after, Some(launched_pid), already_running, opts.fresh_profile, opts.automation);
        return emit(r, extra);
    }

    if !already_running && wait_ms > 0 && holder.is_none() {
        let detail = format!(
            "{} was spawned (PID {launched_pid}), but no process using '{}' appeared within {} ms. \
             Run `cua edge --status` and check whether {} showed an error dialog.",
            kind.label(),
            profile.display(),
            wait_ms,
            kind.label()
        );
        let r = Receipt::new("edge").fail(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        let extra = launch_extra(kind, Some(edge.as_path()), &profile, &after, Some(launched_pid), already_running, opts.fresh_profile, opts.automation);
        return emit(r, extra);
    }

    let profile_name = if opts.fresh_profile {
        profile
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| kind.profile_dir().to_string())
    } else {
        kind.profile_dir().to_string()
    };
    let detail = if already_running {
        let h = best_of(same);
        format!(
            "opened a new {} window in the existing {} instance (PID {}, session {})",
            kind.label(),
            profile_name,
            h.pid,
            h.session
        )
    } else if let Some(h) = &holder {
        format!(
            "launched {} with the {} profile (launcher PID {launched_pid}; process using the profile at launch: PID {} in session {})",
            kind.label(),
            profile_name,
            h.pid,
            h.session
        )
    } else {
        format!(
            "launched {} with the {} profile (launcher PID {launched_pid}); not waiting for the profile holder",
            kind.label(),
            profile_name
        )
    };
    let r = Receipt::new("edge").ok(
        json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
        &detail,
    );
    let mut extra = launch_extra(kind, Some(edge.as_path()), &profile, &after, Some(launched_pid), already_running, opts.fresh_profile, opts.automation);
    extra["profile_import"] = json!({
        "new_profile": new_profile,
        "consent": consent.as_str(),
        "purged": purged,
        "processes_stopped": stopped,
    });
    if let Some(p) = &pair {
        extra["browser"] = json!({
            "slot": p.slot,
            "pair_url": p.pair_url,
            "extension_dir": p.extension_dir.display().to_string(),
            "extension_id": p.extension_id,
        });
    }
    let cdp = if opts.cdp || after.holders.iter().any(|h| h.is_browser && h.cdp_port.is_some()) {
        let preferred = if already_running { None } else { cdp_port };
        wait_for_cdp_endpoint(&profile, &after, preferred, Duration::from_secs(3))
    } else {
        None
    };
    if let Some(c) = cdp {
        if let Ok(v) = serde_json::to_value(c) {
            extra["cdp"] = v;
        }
    } else if opts.cdp {
        extra["cdp_error"] = json!(if already_running {
            format!(
                "the existing {} instance was not launched with --cdp; close it and relaunch with --cdp",
                kind.profile_dir()
            )
        } else {
            "CDP endpoint did not appear within 3s; UIA still works".to_string()
        });
    }
    emit(r, extra)
}

fn launch_extra(
    kind: BrowserKind,
    edge: Option<&Path>,
    profile: &Path,
    scan: &Scan,
    launched_pid: Option<u32>,
    already_running: bool,
    fresh: bool,
    automation: bool,
) -> Value {
    json!({
        "backend": "native",
        "browser": kind.as_str(),
        "browser_exe": edge.map(|p| p.display().to_string()),
        "profile": profile.display().to_string(),
        "profile_kind": kind.profile_dir(),
        "session_id": current_session(),
        "child_session": in_child_session(),
        "automation": automation,
        "fresh_profile": fresh,
        "launched_pid": launched_pid,
        "already_running": already_running,
        "lockfile": scan.lockfile.as_str(),
        "restart_manager_ok": scan.rm_ok,
        "holders": scan.holders,
    })
}

fn emit(r: Receipt, extra: Value) -> Result<i32> {
    let code = r.exit_code();
    let mut v = serde_json::to_value(&r)?;
    if let (Some(dst), Some(src)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in src {
            dst.insert(k.clone(), val.clone());
        }
    }
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(code)
}

pub struct SessionEdgeOptions {
    pub sign_in: bool,
    pub status: bool,
    pub delete_profile: bool,
    pub confirm: bool,
    pub urls: Vec<String>,
    pub timeout_min: u64,
    pub browser: Option<BrowserPref>,
    pub import_passwords: Option<bool>,
}

/// `cua session edge ...`: the user's main-session side of the CuaEdge/CuaChrome
/// profile. `--sign-in` opens the profile where the user actually is, waits for
/// every browser process using it to exit, and leaves `cua child edge` with the
/// login.
pub fn cmd_session_edge(opts: SessionEdgeOptions) -> Result<i32> {
    let (kind, edge) = opts.browser.unwrap_or(BrowserPref::Auto).resolve();
    let profile = default_profile(kind);

    let modes = [opts.sign_in, opts.status, opts.delete_profile]
        .iter()
        .filter(|b| **b)
        .count();
    if modes > 1 {
        return session_edge_refusal(
            kind,
            &profile,
            "session-edge",
            "choose exactly one of --sign-in, --status, --delete-profile",
            json!({ "urls": opts.urls }),
        );
    }
    if opts.confirm && !opts.delete_profile {
        return session_edge_refusal(
            kind,
            &profile,
            "session-edge",
            "--confirm is only valid with --delete-profile",
            json!({ "urls": opts.urls }),
        );
    }

    if opts.status || modes == 0 {
        if !opts.urls.is_empty() {
            return session_edge_refusal(
                kind,
                &profile,
                "session-edge",
                &format!(
                    "--status does not launch {}; remove URL arguments",
                    kind.label()
                ),
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
        return print_status(kind, edge.as_deref(), &profile, false);
    }

    if opts.delete_profile {
        if !opts.urls.is_empty() {
            return session_edge_refusal(
                kind,
                &profile,
                "session-edge-delete",
                &format!(
                    "--delete-profile does not launch {}; remove URL arguments",
                    kind.label()
                ),
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
        if !opts.confirm {
            return session_edge_refusal(
                kind,
                &profile,
                "session-edge-delete",
                &format!(
                    "--delete-profile removes the {} cookies/passwords; pass --confirm to proceed",
                    kind.profile_dir()
                ),
                json!({ "profile": profile.display().to_string() }),
            );
        }
        return delete_session_profile(kind, edge.as_deref(), &profile);
    }

    // sign-in
    if let Some(reason) = main_session_guard() {
        return session_edge_refusal(
            kind,
            &profile,
            "session-edge-signin",
            &reason,
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
        );
    }
    if let Err(e) = validate_urls(&opts.urls) {
        return session_edge_refusal(
            kind,
            &profile,
            "session-edge-signin",
            &e,
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
        );
    }
    let Some(edge) = edge else {
        return session_edge_failure(
            kind,
            &profile,
            "session-edge-signin",
            &format!(
                "{} not found. Checked ${}, config `{}`, App Paths and the standard install paths",
                kind.exe_name(),
                kind.env_var(),
                kind.config_key()
            ),
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
        );
    };

    let current = current_session();
    let (scan, refusal) = inspect_profile(&profile, current);
    if let Some(detail) = refusal {
        let r = Receipt::new("session-edge-signin").refused(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        return emit(
            r,
            session_edge_extra(kind, Some(edge.as_path()), &profile, json!({
                "lockfile": scan.lockfile.as_str(),
                "holders": scan.holders,
            })),
        );
    }

    let page = match write_sign_in_page(kind, &opts.urls) {
        Ok(p) => p,
        Err(e) => {
            return session_edge_failure(
                kind,
                &profile,
                "session-edge-signin",
                &format!("could not write the sign-in page: {e:#}"),
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
    };
    let mut tabs = vec![file_url(&page)];
    tabs.extend(opts.urls.iter().cloned());

    let new_profile = !profile.exists();
    let recorded = read_metadata();
    let mut consent = ImportConsent::resolve(opts.import_passwords, recorded.as_ref());
    if new_profile && consent == ImportConsent::Unrecorded && std::io::stdin().is_terminal() {
        consent = prompt_import_consent(kind, &profile);
    }
    if consent == ImportConsent::Unrecorded {
        consent = ImportConsent::Yes;
    }
    let import_purge = new_profile && consent == ImportConsent::No;

    let launched = if import_purge {
        launch_without_import(&edge, &profile, false, None, &tabs, None)
    } else {
        spawn_browser(&edge, &profile, false, None, &tabs).map(|pid| (pid, Vec::new(), 0usize))
    };
    let (launched_pid, purged, stopped) = match launched {
        Ok(v) => v,
        Err(e) => {
            return session_edge_failure(
                kind,
                &profile,
                "session-edge-signin",
                &format!("{e:#}"),
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
    };
    let _ = write_metadata(&profile, kind, &edge, consent);
    let timeout_min = opts.timeout_min.clamp(1, 180);
    let started = Instant::now();
    let outcome = wait_for_sign_in(&profile, started, Duration::from_secs(timeout_min * 60));

    match outcome {
        SignInOutcome::Closed { scan, waited } => {
            let _ = write_metadata(&profile, kind, &edge, consent);
            let detail = format!(
                "{} sign-in finished: every browser process using the profile exited after {:.1}s. \
                 The profile keeps the cookies/passwords saved in that window; run `cua child edge` \
                 to use the same login in the child session.",
                kind.profile_dir(),
                waited.as_secs_f64()
            );
            let r = Receipt::new("session-edge-signin").ok(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                &detail,
            );
            emit(
                r,
                session_edge_extra(kind, Some(edge.as_path()), &profile, json!({
                    "sign_in_page": page.display().to_string(),
                    "launched_pid": launched_pid,
                    "profile_import": {
                        "new_profile": new_profile,
                        "consent": consent.as_str(),
                        "purged": purged,
                        "processes_stopped": stopped,
                    },
                    "closed": true,
                    "waited_ms": waited.as_millis() as u64,
                    "timeout_min": timeout_min,
                    "lockfile": scan.lockfile.as_str(),
                    "restart_manager_ok": scan.rm_ok,
                    "holders": scan.holders,
                })),
            )
        }
        SignInOutcome::Timeout { scan, seen, waited } => {
            let live_holders: Vec<&Holder> =
                scan.holders.iter().filter(|h| h.is_browser).collect();
            let list = if live_holders.is_empty() {
                "none (the profile lock may be held by another process)".to_string()
            } else {
                holder_summary(&live_holders)
            };
            let detail = if seen {
                format!(
                    "{} sign-in is still open after {} min. Browser processes using '{}': {}. \
                     cua never kills them; close those {} windows (or end the PIDs yourself), \
                     then run `cua session edge --status`.",
                    kind.profile_dir(),
                    timeout_min,
                    profile.display(),
                    list,
                    kind.label()
                )
            } else {
                format!(
                    "{} was spawned (PID {launched_pid}) but no process using '{}' appeared within 20s. \
                     Check whether {} showed an error dialog, then run `cua session edge --status`.",
                    kind.label(),
                    profile.display(),
                    kind.label()
                )
            };
            let r = Receipt::new("session-edge-signin").fail(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                &detail,
            );
            emit(
                r,
                session_edge_extra(kind, Some(edge.as_path()), &profile, json!({
                    "sign_in_page": page.display().to_string(),
                    "launched_pid": launched_pid,
                    "closed": false,
                    "seen": seen,
                    "waited_ms": waited.as_millis() as u64,
                    "timeout_min": timeout_min,
                    "lockfile": scan.lockfile.as_str(),
                    "restart_manager_ok": scan.rm_ok,
                    "holders": scan.holders,
                })),
            )
        }
    }
}

fn session_edge_refusal(
    kind: BrowserKind,
    profile: &Path,
    cmd: &str,
    detail: &str,
    fields: Value,
) -> Result<i32> {
    let r = Receipt::new(cmd).refused(fields.clone(), detail);
    emit(r, session_edge_extra(kind, None, profile, fields))
}

fn session_edge_failure(
    kind: BrowserKind,
    profile: &Path,
    cmd: &str,
    detail: &str,
    fields: Value,
) -> Result<i32> {
    let r = Receipt::new(cmd).fail(fields.clone(), detail);
    emit(r, session_edge_extra(kind, None, profile, fields))
}

fn session_edge_extra(
    kind: BrowserKind,
    edge: Option<&Path>,
    profile: &Path,
    fields: Value,
) -> Value {
    let mut v = json!({
        "backend": "native",
        "browser": kind.as_str(),
        "browser_exe": edge.map(|p| p.display().to_string()),
        "profile": profile.display().to_string(),
        "profile_kind": kind.profile_dir(),
        "session_id": current_session(),
        "child_session": in_child_session(),
    });
    if let (Some(dst), Some(src)) = (v.as_object_mut(), fields.as_object()) {
        for (k, val) in src {
            dst.insert(k.clone(), val.clone());
        }
    }
    v
}

enum SignInOutcome {
    Closed { scan: Scan, waited: Duration },
    Timeout { scan: Scan, seen: bool, waited: Duration },
}

/// The sign-in window must appear first (20s), then stay stable-empty for 1.5s:
/// Edge's elevated-to-normal relaunch briefly removes the first process.
fn wait_for_sign_in(profile: &Path, started: Instant, timeout: Duration) -> SignInOutcome {
    let deadline = started + timeout;
    let startup_deadline = started + Duration::from_secs(20);
    let mut seen = false;
    let mut empty_since: Option<Instant> = None;
    loop {
        let scan = scan_profile(profile);
        if scan.holders.iter().any(|h| h.is_browser) {
            seen = true;
            empty_since = None;
        } else if seen {
            let since = *empty_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(1500) {
                return SignInOutcome::Closed { scan, waited: started.elapsed() };
            }
        }
        let now = Instant::now();
        if (!seen && now >= startup_deadline) || now >= deadline {
            return SignInOutcome::Timeout { scan, seen, waited: started.elapsed() };
        }
        std::thread::sleep(WAIT_STEP);
    }
}

/// Sign-in and profile deletion belong to the user's main session. The child
/// agent already blocks `cua session`; this also covers a manual shell inside
/// the child session.
fn main_session_guard() -> Option<String> {
    if in_child_session() {
        return Some(
            "run `cua session edge ...` in the user's main session, not the child session; \
             use `cua child edge` for the child browser"
                .to_string(),
        );
    }
    let current = current_session();
    if let Some(child) = crate::session::child_session_id() {
        if child == current {
            return Some(format!(
                "this is child session {child}; run `cua session edge ...` from the user's main \
                 session (or use `cua child edge` for the child browser)"
            ));
        }
    }
    None
}

fn delete_session_profile(
    kind: BrowserKind,
    edge: Option<&Path>,
    profile: &Path,
) -> Result<i32> {
    if let Some(reason) = main_session_guard() {
        return session_edge_refusal(
            kind,
            profile,
            "session-edge-delete",
            &reason,
            json!({ "profile": profile.display().to_string() }),
        );
    }
    let current = current_session();
    let scan = scan_profile(profile);
    let holders: Vec<&Holder> = scan.holders.iter().filter(|h| h.is_browser).collect();
    if !holders.is_empty() {
        let list = holder_summary(&holders);
        let detail = format!(
            "refusing to delete '{}': {} is still using it ({list}). Close those {} windows first.",
            profile.display(),
            kind.label(),
            kind.label()
        );
        let r = Receipt::new("session-edge-delete").refused(
            json!({ "profile": profile.display().to_string(), "session": current }),
            &detail,
        );
        return emit(
            r,
            session_edge_extra(kind, edge, profile, json!({
                "lockfile": scan.lockfile.as_str(),
                "restart_manager_ok": scan.rm_ok,
                "holders": scan.holders,
            })),
        );
    }
    if !matches!(scan.lockfile, LockFileState::Absent | LockFileState::Stale) {
        let detail = format!(
            "refusing to delete '{}': its lockfile is {} (held or unreadable). Close any {} using \
             the profile, then retry.",
            profile.display(),
            scan.lockfile.as_str(),
            kind.label()
        );
        let r = Receipt::new("session-edge-delete").refused(
            json!({ "profile": profile.display().to_string(), "session": current }),
            &detail,
        );
        return emit(
            r,
            session_edge_extra(kind, edge, profile, json!({
                "lockfile": scan.lockfile.as_str(),
                "restart_manager_ok": scan.rm_ok,
                "holders": scan.holders,
            })),
        );
    }

    // A junction/symlink could point at the user's real browser profile; only
    // delete a path that really lives under cua's browser directory.
    let browser_dir = util::ensure_data_dir().join("browser");
    let canon_browser = std::fs::canonicalize(&browser_dir).unwrap_or(browser_dir);
    let exists = profile.exists();
    if exists {
        if let Ok(md) = std::fs::symlink_metadata(profile) {
            if md.file_type().is_symlink() {
                return session_edge_refusal(
                    kind,
                    profile,
                    "session-edge-delete",
                    "refusing to delete a symlinked profile; remove the link yourself if it is safe",
                    json!({ "profile": profile.display().to_string() }),
                );
            }
        }
        let canon_profile =
            std::fs::canonicalize(profile).unwrap_or_else(|_| profile.to_path_buf());
        if !canon_profile.starts_with(&canon_browser) {
            return session_edge_refusal(
                kind,
                profile,
                "session-edge-delete",
                "refusing to delete a profile that resolves outside the cua browser directory",
                json!({ "profile": profile.display().to_string() }),
            );
        }
        std::fs::remove_dir_all(profile)
            .with_context(|| format!("delete {}", profile.display()))?;
    }
    let meta_removed = read_metadata()
        .filter(|m| paths_equal(Path::new(&m.profile), profile))
        .and_then(|_| std::fs::remove_file(metadata_path()).ok())
        .is_some();
    let detail = if exists {
        format!(
            "deleted {} profile '{}' (its cookies and saved passwords are gone)",
            kind.profile_dir(),
            profile.display()
        )
    } else {
        format!(
            "{} profile did not exist; nothing to delete",
            kind.profile_dir()
        )
    };
    let r = Receipt::new("session-edge-delete").ok(
        json!({ "profile": profile.display().to_string(), "session": current }),
        &detail,
    );
    emit(
        r,
        session_edge_extra(kind, edge, profile, json!({
            "deleted": exists,
            "metadata_removed": meta_removed,
        })),
    )
}

// ---------------------------------------------------------------- sign-in page

fn write_sign_in_page(kind: BrowserKind, urls: &[String]) -> Result<PathBuf> {
    let path = util::ensure_data_dir().join("browser").join("sign-in.html");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    std::fs::write(&path, sign_in_page_html(kind, urls))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn sign_in_page_html(kind: BrowserKind, urls: &[String]) -> String {
    let list = if urls.is_empty() {
        String::new()
    } else {
        let items = urls
            .iter()
            .map(|u| format!("<li><code>{}</code></li>", html_escape(u)))
            .collect::<String>();
        format!("<p>本次已打开的站点：</p><ul>{items}</ul>")
    };
    let label = kind.label();
    let dir = kind.profile_dir();
    format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<title>cua · {dir} 登录</title>
<style>
body{{font:15px/1.7 system-ui,"Segoe UI",sans-serif;background:#1b1a20;color:#eee;max-width:760px;margin:40px auto;padding:0 24px}}
h1{{font-size:22px;font-weight:600}} code{{background:#2b2a33;padding:2px 5px;border-radius:4px}}
li{{margin:6px 0}} .warn{{color:#ffcf70}} .muted{{color:#aaa}}
</style></head><body>
<h1>{dir} 登录</h1>
<p>这是 cua 为子会话准备的<strong>独立 {label} 配置</strong>。请在这个窗口里登录需要的站点，2FA 也在这里完成。</p>
<ol>
  <li>在其它标签页完成登录。</li>
  <li>登录完成后，<strong>关闭所有 {dir} {label} 窗口</strong>（包括这个说明页）。</li>
  <li>cua 会等到所有使用该 profile 的 {label} 进程退出后返回。</li>
  <li>之后在子会话运行 <code>cua child edge</code>，会复用这里的 cookie 和已保存密码。</li>
</ol>
<p class="warn">cua 不会读取、打印或保存你的密码；主 {label} 的 cookie/密码不会自动复制到这里。不要把不想让子会话访问的账号登录到这个 profile。</p>
<p class="muted">{label} 可能通过 Windows 账号自动 SSO；同步默认开启，同一账号保存的密码可能同步到 {dir}。关闭窗口后如果命令仍不返回，运行 <code>cua session edge --status</code> 查看持有 profile 的 PID。</p>
{list}
</body></html>"#
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/'
            | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn print_status(kind: BrowserKind, edge: Option<&Path>, profile: &Path, fresh: bool) -> Result<i32> {
    let scan = scan_profile(profile);
    let meta = read_metadata();
    let current = current_session();
    let other = holder_outside(&scan, current);
    let same = holder_in(&scan, current);
    let note = if edge.is_none() {
        format!(
            "{} not found; checked ${}, config `{}`, App Paths and the standard install paths",
            kind.exe_name(),
            kind.env_var(),
            kind.config_key()
        )
    } else if let Some(h) = other {
        format!(
            "profile is held by {} in session {} (PID {}); close it before launching",
            kind.label(),
            h.session,
            h.pid
        )
    } else if same.is_some() {
        "profile is in use in this session; `cua edge <url>` opens a new window in that instance"
            .to_string()
    } else if matches!(scan.lockfile, LockFileState::Locked | LockFileState::Unknown)
        && !scan.rm_ok
    {
        "profile lockfile is held but no holder could be identified; launching is refused"
            .to_string()
    } else {
        "profile is free".to_string()
    };
    print_status_value(kind, edge, profile, fresh, &scan, meta, current, &note)
}

fn print_status_value(
    kind: BrowserKind,
    edge: Option<&Path>,
    profile: &Path,
    fresh: bool,
    scan: &Scan,
    meta: Option<ProfileMeta>,
    current: u32,
    note: &str,
) -> Result<i32> {
    let meta_matches = meta
        .as_ref()
        .map(|m| paths_equal(Path::new(&m.profile), profile))
        .unwrap_or(false);
    let cdp = cdp_endpoint(profile, scan);
    let v = json!({
        "schema": STATUS_SCHEMA,
        "ok": edge.is_some(),
        "browser": kind.as_str(),
        "browser_exe": edge.map(|p| p.display().to_string()),
        "profile": profile.display().to_string(),
        "profile_kind": kind.profile_dir(),
        "metadata_path": metadata_path().display().to_string(),
        "metadata": meta,
        "metadata_matches_profile": meta_matches,
        "current_session": current,
        "child_session": in_child_session(),
        "fresh_profile": fresh,
        "lockfile": scan.lockfile.as_str(),
        "restart_manager_ok": scan.rm_ok,
        "browser_processes": scan.browser_processes,
        "holders": scan.holders,
        "cdp": cdp,
        "note": note,
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(if edge.is_some() { 0 } else { 1 })
}

// ---------------------------------------------------------------- profile

fn default_profile(kind: BrowserKind) -> PathBuf {
    util::ensure_data_dir()
        .join("browser")
        .join(kind.profile_dir())
}

fn resolve_profile(kind: BrowserKind, override_path: Option<&str>, fresh: bool) -> PathBuf {
    if fresh {
        fresh_profile_path(kind)
    } else if let Some(p) = override_path.filter(|p| !p.trim().is_empty()) {
        absolutize(Path::new(p.trim()))
    } else {
        default_profile(kind)
    }
}

fn fresh_profile_path(kind: BrowserKind) -> PathBuf {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "cua-{}-fresh-{}-{ms}",
        kind.as_str(),
        std::process::id()
    ))
}

fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

fn path_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

// ---------------------------------------------------------------- metadata

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProfileMeta {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    browser: String,
    profile: String,
    #[serde(default)]
    browser_exe: String,
    created_at: String,
    last_used_at: String,
    #[serde(default)]
    ephemeral: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_import_consent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    consent_at: Option<String>,
}

fn metadata_path() -> PathBuf {
    util::ensure_data_dir()
        .join("browser")
        .join("browser-profile.json")
}

/// `cua edge` used to write `edge-profile.json`; keep reading it so an existing
/// CuaEdge profile is still recognised as Edge after the rename.
fn legacy_metadata_path() -> PathBuf {
    util::ensure_data_dir().join("browser").join("edge-profile.json")
}

fn read_metadata() -> Option<ProfileMeta> {
    read_metadata_at(&metadata_path()).or_else(|| read_metadata_at(&legacy_metadata_path()))
}

fn read_metadata_at(path: &Path) -> Option<ProfileMeta> {
    let b = std::fs::read(path).ok()?;
    let b = b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b);
    serde_json::from_slice(b).ok()
}

fn write_metadata(
    profile: &Path,
    kind: BrowserKind,
    edge: &Path,
    consent: ImportConsent,
) -> Result<()> {
    let path = metadata_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let existing = read_metadata().filter(|m| paths_equal(Path::new(&m.profile), profile));
    let created_at = existing
        .as_ref()
        .map(|m| m.created_at.clone())
        .unwrap_or_else(util::now_iso);
    let (consent_value, consent_at) = match consent {
        ImportConsent::Unrecorded => (
            existing.as_ref().and_then(|m| m.local_import_consent.clone()),
            existing.as_ref().and_then(|m| m.consent_at.clone()),
        ),
        decided => (Some(decided.as_str().to_string()), Some(util::now_iso())),
    };
    let meta = ProfileMeta {
        schema: PROFILE_SCHEMA.into(),
        browser: kind.as_str().into(),
        profile: profile.display().to_string(),
        browser_exe: edge.display().to_string(),
        created_at,
        last_used_at: util::now_iso(),
        ephemeral: false,
        local_import_consent: consent_value,
        consent_at,
    };
    let mut bytes = serde_json::to_vec_pretty(&meta)?;
    bytes.push(b'\n');
    std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------- browser lookup

fn locate(kind: BrowserKind) -> Option<PathBuf> {
    first_existing(&candidates(kind))
}

fn candidates(kind: BrowserKind) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var(kind.env_var()) {
        push_candidate(&mut out, &p);
    }
    if let Some(p) = util::load_config_json()
        .get(kind.config_key())
        .and_then(Value::as_str)
    {
        push_candidate(&mut out, p);
    }
    for path in kind.app_path_keys() {
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Some(p) = reg_value_string(hive, path) {
                push_candidate(&mut out, &p);
            }
        }
    }
    for base in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(dir) = std::env::var(base) {
            push_candidate(
                &mut out,
                &format!("{dir}\\{}\\{}", kind.install_subdir(), kind.exe_name()),
            );
        }
    }
    out
}

fn push_candidate(out: &mut Vec<PathBuf>, p: &str) {
    let p = p.trim().trim_matches('"');
    if p.is_empty() {
        return;
    }
    let pb = PathBuf::from(p);
    if !out.iter().any(|e| paths_equal(e, &pb)) {
        out.push(pb);
    }
}

fn first_existing(cands: &[PathBuf]) -> Option<PathBuf> {
    cands.iter().find(|p| p.is_file()).cloned()
}

fn reg_value_string(hive: HKEY, subkey: &str) -> Option<String> {
    let subkey: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let rc = RegGetValueW(
            hive,
            PCWSTR(subkey.as_ptr()),
            w!(""),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if rc.is_err() {
            return None;
        }
        let len = (size as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        let s = s
            .trim_matches(char::from(0))
            .trim()
            .trim_matches('"')
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

// ---------------------------------------------------------------- launching

fn build_args(
    profile: &Path,
    automation: bool,
    cdp_port: Option<u16>,
    urls: &[String],
    load_extension: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--new-window".into(),
        // The user wants the CuaEdge profile to sync/share the main profile's
        // data: do not pass --disable-sync. --disable-background-mode only stops
        // a windowless Edge from holding the profile after its windows close.
        "--disable-background-mode".into(),
    ];
    if automation {
        args.push("--force-renderer-accessibility".into());
    }
    if let Some(ext) = load_extension {
        // Chromium 137+ ignores --load-extension unless this feature is off
        // (Edge 157 measured: the pair page opened, no extension ever loaded).
        args.push("--disable-features=DisableLoadExtensionCommandLineSwitch".into());
        args.push(format!("--load-extension={}", ext.display()));
    }
    if let Some(port) = cdp_port {
        args.push(format!("--remote-debugging-port={port}"));
        args.push("--remote-debugging-address=127.0.0.1".into());
        args.push(format!("--remote-allow-origins=http://127.0.0.1:{port}"));
    }
    args.extend(urls.iter().cloned());
    args
}

fn validate_urls(urls: &[String]) -> Result<(), String> {
    if urls.len() > MAX_URLS {
        return Err(format!(
            "at most {MAX_URLS} URL arguments are accepted (got {})",
            urls.len()
        ));
    }
    for u in urls {
        if u.trim().is_empty() {
            return Err("empty URL argument".into());
        }
        if u.starts_with('-') {
            return Err(format!(
                "'{u}' looks like a browser switch; URL arguments must not start with '-' \
                 (use --automation for the one supported switch)"
            ));
        }
        if u.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
            return Err("URL arguments must not contain NUL or newline characters".into());
        }
    }
    Ok(())
}

fn spawn_browser(
    exe: &Path,
    profile: &Path,
    automation: bool,
    cdp_port: Option<u16>,
    urls: &[String],
) -> Result<u32> {
    spawn_browser_with(exe, profile, automation, cdp_port, urls, None)
}

/// Launch with an optional pairing tab: the pair page goes first (it
/// registers the slot), the caller's URLs follow.
fn spawn_browser_with(
    exe: &Path,
    profile: &Path,
    automation: bool,
    cdp_port: Option<u16>,
    urls: &[String],
    pair: Option<&BrowserPair>,
) -> Result<u32> {
    let mut tabs: Vec<String> = Vec::new();
    if let Some(p) = pair {
        tabs.push(p.pair_url.clone());
    }
    tabs.extend(urls.iter().cloned());
    let args = build_args(
        profile,
        automation,
        cdp_port,
        &tabs,
        pair.map(|p| p.extension_dir.as_path()),
    );
    spawn_browser_args(exe, &args, false)
}

/// First-run window used only while the import is being undone: start it
/// minimized so it does not steal the desktop for a second.
fn spawn_browser_minimized(exe: &Path, profile: &Path) -> Result<u32> {
    let args = build_args(profile, false, None, &[], None);
    spawn_browser_args(exe, &args, true)
}

fn spawn_browser_args(exe: &Path, args: &[String], minimized: bool) -> Result<u32> {
    let command_line = build_command_line(exe, args);
    let exe_w: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut cmd_w: Vec<u16> = command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let env = util::sanitized_env_block();
    let mut si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    if minimized {
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_SHOWMINNOACTIVE.0 as u16;
    }
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(exe_w.as_ptr()),
            PWSTR(cmd_w.as_mut_ptr()),
            None,
            None,
            // Never inherit the agent's stdout/stderr pipes: Edge outlives this
            // process, and an inherited pipe would keep the agent waiting for EOF
            // until the browser window closes.
            false,
            CREATE_UNICODE_ENVIRONMENT,
            Some(env.as_ptr() as *const _),
            PCWSTR::null(),
            &si,
            &mut pi,
        )
        .with_context(|| format!("CreateProcessW({})", exe.display()))?;
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(pi.dwProcessId)
}

// ------------------------------------------------------------- browser pairing

/// What `browser.pair` minted: cua launches the browser itself, so the pair
/// page must be the first tab of that launch.
#[derive(Clone, Debug)]
struct BrowserPair {
    slot: String,
    pair_url: String,
    extension_dir: PathBuf,
    extension_id: String,
}

fn browser_slot(kind: BrowserKind) -> &'static str {
    match kind {
        BrowserKind::Edge => "cuaedge",
        BrowserKind::Chrome => "cuachrome",
    }
}

fn pair_profile(kind: BrowserKind) -> Result<BrowserPair> {
    // Inside the child session our own client would auto-start a daemon
    // *there*, which answers the pair call but leaves the launch hanging; the
    // daemon has to be up in the main session before anything pairs.
    if in_child_session() && !crate::browser::daemon_alive() {
        anyhow::bail!(
            "no browser daemon on 127.0.0.1:{} — start it in the main session first (`cua browser status`), then retry",
            crate::browser::port()
        );
    }
    let ext = crate::browser::extension_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "browser extension not found: run `cua browser install` (or set CUA_BROWSER_EXTENSION_DIR)"
        )
    })?;
    let slot = browser_slot(kind);
    let ext_str = ext.display().to_string();
    let v = match crate::browser::call_json(
        "browser.pair",
        &serde_json::json!({
            "name": slot,
            "extension_dir": ext_str,
        }),
    ) {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("{e:#}");
            // A daemon left over from an older install answers but has no
            // browser.pair; cua cannot fix that without killing it.
            if msg.contains("UNKNOWN_TOOL") {
                anyhow::bail!(
                    "{msg}\n(hint: the running daemon is older than this cua — run `cua browser stop`, then retry)"
                );
            }
            return Err(e);
        }
    };
    let pair_url = v
        .get("pair_url")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("browser.pair returned no pair_url"))?;
    Ok(BrowserPair {
        slot: v
            .get("slot")
            .and_then(|x| x.as_str())
            .unwrap_or(slot)
            .to_string(),
        pair_url: pair_url.to_string(),
        extension_dir: PathBuf::from(
            v.get("extension_dir")
                .and_then(|x| x.as_str())
                .unwrap_or(ext_str.as_str()),
        ),
        extension_id: v
            .get("extension_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

// ---------------------------------------------------------------- import consent

/// Files Edge/Chrome fills from the main profile during its first run.
const IMPORTED_PROFILE_PATHS: [&str; 14] = [
    "Default/Login Data",
    "Default/Login Data-journal",
    "Default/Web Data",
    "Default/Web Data-journal",
    "Default/History",
    "Default/History-journal",
    "Default/Bookmarks",
    "Default/Bookmarks.bak",
    "Default/Top Sites",
    "Default/Favicons",
    "Default/Shortcuts",
    "Default/Network/Cookies",
    "Default/Network/Cookies-journal",
    "Default/Sessions",
];

/// Delete only what the first run imported. Missing files are normal (a profile
/// that imported nothing yet), so the report lists what was actually removed.
fn purge_imported_profile_data(profile: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    for rel in IMPORTED_PROFILE_PATHS {
        let path = profile.join(rel.replace('/', "\\"));
        if path.is_dir() {
            if std::fs::remove_dir_all(&path).is_ok() {
                removed.push(rel.to_string());
            }
        } else if path.is_file() && std::fs::remove_file(&path).is_ok() {
            removed.push(rel.to_string());
        }
    }
    removed
}

/// `--no-import-passwords` on a brand-new profile: let the browser run its
/// first-run import minimized, stop it, wipe the imported files, then open the
/// window the caller asked for.
fn launch_without_import(
    exe: &Path,
    profile: &Path,
    automation: bool,
    cdp_port: Option<u16>,
    urls: &[String],
    pair: Option<&BrowserPair>,
) -> Result<(u32, Vec<String>, usize)> {
    spawn_browser_minimized(exe, profile).context("first-run launch (minimized)")?;

    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        let scan = scan_profile(profile);
        if scan.holders.iter().any(|h| h.browser) {
            break;
        }
        std::thread::sleep(WAIT_STEP * 2);
    }
    // The importer writes a moment after the first window appears.
    std::thread::sleep(Duration::from_secs(3));

    let scan = scan_profile(profile);
    let mut stopped = 0usize;
    for h in scan
        .holders
        .iter()
        .filter(|h| h.browser && h.session == current_session())
    {
        stopped += crate::win::kill_process_tree(h.pid) as usize;
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let scan = scan_profile(profile);
        let free = scan.holders.is_empty()
            && matches!(
                scan.lockfile,
                LockFileState::Absent | LockFileState::Stale
            );
        if free || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(WAIT_STEP * 2);
    }

    let purged = purge_imported_profile_data(profile);
    let pid = spawn_browser_with(exe, profile, automation, cdp_port, urls, pair)
        .context("relaunch after removing the first-run import")?;
    Ok((pid, purged, stopped))
}

/// Yes/No prompt shown in the main session before a new profile is created.
/// Default button is Yes (import), matching the documented behaviour.
fn prompt_import_consent(kind: BrowserKind, profile: &Path) -> ImportConsent {
    let text = format!(
        "即将创建 {} profile：\n{}\n\n\
         {} 首次运行会把主 {} 的已保存密码和历史导入这份 profile（本机实测约 460 条登录、1190 条历史）。\n\n\
         导入后，这份数据以当前 Windows 用户身份可被读取（子会话与 agent 也一样）；它不是安全沙箱。\n\n\
         允许导入吗？\n\n是 = 允许导入（默认）\n否 = 不导入，只保留本次登录产生的数据",
        kind.profile_dir(),
        profile.display(),
        kind.label(),
        kind.label()
    );
    let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = format!("cua · {} profile", kind.profile_dir())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let rc = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON1,
        )
    };
    if rc == IDYES {
        ImportConsent::Yes
    } else {
        ImportConsent::No
    }
}


// ---------------------------------------------------------------- CDP endpoint

#[derive(Serialize, Clone, Debug)]
struct CdpEndpoint {
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_ws: Option<String>,
}

fn pick_free_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .ok()?
        .local_addr()
        .ok()
        .map(|a| a.port())
}

fn read_devtools_port(profile: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok()?;
    text.lines()
        .next()?
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
}

fn cdp_endpoint(profile: &Path, scan: &Scan) -> Option<CdpEndpoint> {
    probe_cdp(&cdp_ports(profile, scan))
}

fn cdp_ports(profile: &Path, scan: &Scan) -> Vec<u16> {
    let mut ports: Vec<u16> = Vec::new();
    for p in scan
        .holders
        .iter()
        .filter(|h| h.is_browser)
        .filter_map(|h| h.cdp_port)
    {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    // The DevToolsActivePort file is a fallback only when command lines were
    // unreadable (e.g. an RM-only holder in another session). A readable
    // command line without --remote-debugging-port means this Edge has no CDP,
    // so a stale file must not turn into a false endpoint.
    let has_cmdline = scan.holders.iter().any(|h| {
        h.is_browser
            && h.sources
                .iter()
                .any(|s| s.eq_ignore_ascii_case("command_line"))
    });
    if ports.is_empty() && !has_cmdline {
        if let Some(p) = read_devtools_port(profile) {
            if !ports.contains(&p) {
                ports.push(p);
            }
        }
    }
    ports
}

fn probe_cdp(ports: &[u16]) -> Option<CdpEndpoint> {
    for &port in ports {
        if let Some(v) = http_json(port, "/json/version") {
            if let Some(ep) = endpoint_from_version(port, &v) {
                return Some(ep);
            }
        }
    }
    None
}

fn wait_for_cdp_endpoint(
    profile: &Path,
    scan: &Scan,
    preferred: Option<u16>,
    timeout: Duration,
) -> Option<CdpEndpoint> {
    let deadline = Instant::now() + timeout;
    let mut ports: Vec<u16> = preferred.into_iter().collect();
    loop {
        for p in cdp_ports(profile, scan) {
            if !ports.contains(&p) {
                ports.push(p);
            }
        }
        if let Some(ep) = probe_cdp(&ports) {
            return Some(ep);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn endpoint_from_version(port: u16, v: &Value) -> Option<CdpEndpoint> {
    let browser = v
        .get("Browser")
        .and_then(Value::as_str)
        .map(str::to_string);
    let is_chromium = browser
        .as_deref()
        .map(|b| {
            let b = b.to_ascii_lowercase();
            b.contains("edg/") || b.contains("edge") || b.contains("chrome")
        })
        .unwrap_or(false);
    if !is_chromium {
        return None;
    }
    Some(CdpEndpoint {
        port,
        browser,
        protocol_version: v
            .get("Protocol-Version")
            .and_then(Value::as_str)
            .map(str::to_string),
        browser_ws: v
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn http_json(port: u16, path: &str) -> Option<Value> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(500))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    // Chromium's DevTools HTTP server may keep the socket open despite
    // Connection: close; parse as soon as a complete JSON body is available
    // instead of waiting for EOF.
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => break,
        }
        if let Some(v) = parse_http_json(&buf) {
            return Some(v);
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    parse_http_json(&buf)
}

fn parse_http_json(buf: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(buf).ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.lines().next()?.contains(" 200") {
        return None;
    }
    serde_json::from_str(body).ok()
}

fn build_command_line(exe: &Path, args: &[String]) -> String {
    let mut out = quote_arg(&exe.to_string_lossy());
    for a in args {
        out.push(' ');
        out.push_str(&quote_arg(a));
    }
    out
}

/// Quote one argument for CreateProcessW's command line (Windows rules).
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| c.is_whitespace() || c == '"') {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                for _ in 0..=backslashes {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(ch);
            }
        }
    }
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------- lock state

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LockFileState {
    Absent,
    Stale,
    Locked,
    Unknown,
}

impl LockFileState {
    fn as_str(self) -> &'static str {
        match self {
            LockFileState::Absent => "absent",
            LockFileState::Stale => "stale",
            LockFileState::Locked => "locked",
            LockFileState::Unknown => "unknown",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
struct Holder {
    pid: u32,
    session: u32,
    process: String,
    started_ms: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
    is_browser: bool,
    /// The main browser process (no Chromium `--type=` child role).
    browser: bool,
    /// Dedicated CDP port from this process's command line, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    cdp_port: Option<u16>,
}

struct Scan {
    holders: Vec<Holder>,
    lockfile: LockFileState,
    rm_ok: bool,
    browser_processes: usize,
}

#[derive(Clone, Debug)]
struct ProcInfo {
    pid: u32,
    session: u32,
    name: String,
    started_ms: Option<i64>,
    command_line: Option<String>,
    browser: bool,
    cdp_port: Option<u16>,
}

fn scan_profile(profile: &Path) -> Scan {
    let procs = browser_processes();
    let mut holders = Vec::new();
    for p in &procs {
        if let Some(cl) = &p.command_line {
            if cmdline_has_profile(cl, profile) {
                holders.push(Holder {
                    pid: p.pid,
                    session: p.session,
                    process: p.name.clone(),
                    started_ms: p.started_ms,
                    sources: vec!["command_line".into()],
                    is_browser: BrowserKind::is_browser_name(&p.name),
                    browser: p.browser,
                    cdp_port: p.cdp_port,
                });
            }
        }
    }
    let lockfile = lockfile_state(profile);
    let mut rm_ok = false;
    if matches!(
        lockfile,
        LockFileState::Locked | LockFileState::Stale | LockFileState::Unknown
    ) {
        if let Some(rm) = rm_holders(&profile.join("lockfile")) {
            rm_ok = true;
            for h in rm {
                merge_holder(&mut holders, h);
            }
        }
    }
    holders.sort_by_key(|h| (h.session, !h.browser, h.pid));
    Scan {
        holders,
        lockfile,
        rm_ok,
        browser_processes: procs.len(),
    }
}

fn merge_holder(holders: &mut Vec<Holder>, h: Holder) {
    if let Some(e) = holders.iter_mut().find(|x| x.pid == h.pid) {
        for s in h.sources {
            if !e.sources.contains(&s) {
                e.sources.push(s);
            }
        }
        if e.started_ms.is_none() {
            e.started_ms = h.started_ms;
        }
        if e.process.is_empty() {
            e.process = h.process;
        }
        e.is_browser |= h.is_browser;
        e.browser |= h.browser;
        if e.cdp_port.is_none() {
            e.cdp_port = h.cdp_port;
        }
    } else {
        holders.push(h);
    }
}

/// Prefer a real browser process over crashpad/renderer children in messages.
fn best_of(cands: Vec<&Holder>) -> &Holder {
    best_of_opt(cands).expect("best_of needs a non-empty list")
}

fn best_of_opt(cands: Vec<&Holder>) -> Option<&Holder> {
    cands.iter().copied().find(|h| h.browser).or_else(|| cands.first().copied())
}

fn holder_in(scan: &Scan, session: u32) -> Option<&Holder> {
    let cands = scan
        .holders
        .iter()
        .filter(|h| h.is_browser && h.session == session)
        .collect();
    best_of_opt(cands)
}

fn holder_outside(scan: &Scan, session: u32) -> Option<&Holder> {
    let cands = scan
        .holders
        .iter()
        .filter(|h| h.is_browser && h.session != session)
        .collect();
    best_of_opt(cands)
}

/// Human-readable holder list for errors: browser processes first, capped, with
/// a count for the renderer/crashpad children.
fn holder_summary(holders: &[&Holder]) -> String {
    let mut pick: Vec<&Holder> = holders.iter().copied().filter(|h| h.browser).collect();
    if pick.is_empty() {
        pick = holders.iter().copied().take(5).collect();
    } else {
        pick.truncate(5);
    }
    let shown = pick.len();
    let mut out = pick
        .iter()
        .map(|h| format!("PID {} in session {}", h.pid, h.session))
        .collect::<Vec<_>>()
        .join(", ");
    let extra = holders.len().saturating_sub(shown);
    if extra > 0 {
        out.push_str(&format!(" (+{extra} more browser processes)"));
    }
    out
}

/// Shared launchability check for `cua edge` and `cua session edge --sign-in`.
/// Returns the scan plus a user-facing refusal when another desktop holds the
/// profile or a held lockfile cannot be attributed.
fn inspect_profile(profile: &Path, current: u32) -> (Scan, Option<String>) {
    let scan = scan_profile(profile);
    if let Some(first) = holder_outside(&scan, current) {
        let detail = format!(
            "CuaEdge profile is open in Edge in session {} (PID {}). Edge allows one \
             desktop to use a profile at a time, so a window here would never appear. \
             Close that Edge window (or stop PID {} from your own shell), then run this \
             command again.",
            first.session, first.pid, first.pid
        );
        return (scan, Some(detail));
    }
    if matches!(scan.lockfile, LockFileState::Locked | LockFileState::Unknown)
        && !scan.holders.iter().any(|h| h.is_browser)
        && !scan.rm_ok
    {
        let detail = format!(
            "CuaEdge profile '{}' is locked, but no Edge process could be attributed to it. \
             Close any Edge using that profile, then run this command again (see `cua edge --status`).",
            profile.display()
        );
        return (scan, Some(detail));
    }
    (scan, None)
}

fn lockfile_state(profile: &Path) -> LockFileState {
    let lock = profile.join("lockfile");
    if !lock.is_file() {
        return LockFileState::Absent;
    }
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&lock)
    {
        Ok(_) => LockFileState::Stale,
        Err(e) => match e.raw_os_error() {
            Some(32) | Some(5) => LockFileState::Locked,
            _ => LockFileState::Unknown,
        },
    }
}

fn browser_processes() -> Vec<ProcInfo> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if BrowserKind::is_browser_name(&name) {
                    let pid = entry.th32ProcessID;
                    let command_line = process_command_line(pid);
                    let browser = command_line
                        .as_deref()
                        .map(|c| !c.to_ascii_lowercase().contains("--type="))
                        .unwrap_or(false);
                    let cdp_port = command_line.as_deref().and_then(cdp_port_from_cmdline);
                    out.push(ProcInfo {
                        pid,
                        session: session_of(pid),
                        name,
                        started_ms: crate::win::process_creation_time(pid).map(ticks_to_unix_ms),
                        command_line,
                        browser,
                        cdp_port,
                    });
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn cdp_port_from_cmdline(cmdline: &str) -> Option<u16> {
    let lower = cmdline.to_ascii_lowercase();
    let marker = "--remote-debugging-port=";
    let start = lower.find(marker)? + marker.len();
    let rest = cmdline[start..].trim_start_matches('"');
    let token = rest
        .split(|c: char| c.is_whitespace() || c == '"')
        .next()?;
    token.parse::<u16>().ok().filter(|p| *p > 0)
}

fn process_command_line(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut needed = 0u32;
        let _ = NtQueryInformationProcess(
            h,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
        if needed == 0 {
            let _ = CloseHandle(h);
            return None;
        }
        let mut buf = vec![0u8; needed as usize + 4096];
        let st = NtQueryInformationProcess(
            h,
            ProcessCommandLineInformation,
            buf.as_mut_ptr() as *mut _,
            buf.len() as u32,
            &mut needed,
        );
        let _ = CloseHandle(h);
        if st.is_err() || buf.len() < std::mem::size_of::<UNICODE_STRING>() {
            return None;
        }
        let us = std::ptr::read_unaligned(buf.as_ptr() as *const UNICODE_STRING);
        if us.Length == 0 || us.Buffer.0.is_null() {
            return None;
        }
        let base = buf.as_ptr() as usize;
        let ptr = us.Buffer.0 as usize;
        let bytes = us.Length as usize;
        if ptr < base || ptr.saturating_add(bytes) > base + buf.len() || bytes % 2 != 0 {
            return None;
        }
        let chars = std::slice::from_raw_parts(us.Buffer.0 as *const u16, bytes / 2);
        Some(String::from_utf16_lossy(chars))
    }
}

fn cmdline_has_profile(cmdline: &str, profile: &Path) -> bool {
    let key = path_key(profile);
    if key.is_empty() {
        return false;
    }
    let hay = cmdline.replace('/', "\\").to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = hay[from..].find(&key) {
        let end = from + pos + key.len();
        let next_ok = hay[end..]
            .chars()
            .next()
            .map(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .unwrap_or(true);
        if next_ok {
            return true;
        }
        from += pos + 1;
    }
    false
}

fn rm_holders(lockfile: &Path) -> Option<Vec<Holder>> {
    if !lockfile.is_file() {
        return Some(Vec::new());
    }
    let mut key = [0u16; 64];
    let mut handle = 0u32;
    unsafe {
        let rc = RmStartSession(&mut handle, 0, PWSTR(key.as_mut_ptr()));
        if rc.is_err() {
            return None;
        }
        let lock_w: Vec<u16> = lockfile
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let files = [PCWSTR(lock_w.as_ptr())];
        let rc = RmRegisterResources(handle, Some(files.as_slice()), None, None);
        if rc.is_err() {
            let _ = RmEndSession(handle);
            return None;
        }
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut reasons = 0u32;
        let rc = RmGetList(handle, &mut needed, &mut count, None, &mut reasons);
        if rc.0 != 0 && rc != ERROR_MORE_DATA {
            let _ = RmEndSession(handle);
            return None;
        }
        let mut out = Vec::new();
        if needed > 0 {
            let mut infos = vec![RM_PROCESS_INFO::default(); needed as usize];
            count = needed;
            let rc = RmGetList(
                handle,
                &mut needed,
                &mut count,
                Some(infos.as_mut_ptr()),
                &mut reasons,
            );
            if rc.0 != 0 {
                let _ = RmEndSession(handle);
                return None;
            }
            for info in infos.iter().take(count as usize) {
                let pid = info.Process.dwProcessId;
                if pid == 0 {
                    continue;
                }
                let app = decode_utf16(&info.strAppName);
                let name = crate::win::process_name(pid).unwrap_or_else(|| app.clone());
                let command_line = process_command_line(pid);
                let browser = command_line
                    .as_deref()
                    .map(|c| !c.to_ascii_lowercase().contains("--type="))
                    .unwrap_or(false);
                let started = ((info.Process.ProcessStartTime.dwHighDateTime as u64) << 32)
                    | info.Process.ProcessStartTime.dwLowDateTime as u64;
                let app_lower = app.to_ascii_lowercase();
                let is_browser = BrowserKind::is_browser_name(&name)
                    || app_lower.contains("microsoft edge")
                    || app_lower.contains("msedge")
                    || app_lower.contains("google chrome");
                out.push(Holder {
                    pid,
                    session: info.TSSessionId,
                    process: name,
                    started_ms: if started == 0 {
                        None
                    } else {
                        Some(ticks_to_unix_ms(started))
                    },
                    sources: vec!["restart_manager".into()],
                    is_browser,
                    browser,
                    cdp_port: command_line.as_deref().and_then(cdp_port_from_cmdline),
                });
            }
        }
        let _ = RmEndSession(handle);
        Some(out)
    }
}

fn decode_utf16(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(0);
    if len == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len])
}

fn ticks_to_unix_ms(ticks: u64) -> i64 {
    (ticks / 10_000) as i64 - 11_644_473_600_000
}

fn session_of(pid: u32) -> u32 {
    let mut sid = 0u32;
    unsafe {
        let _ = ProcessIdToSessionId(pid, &mut sid);
    }
    sid
}

fn current_session() -> u32 {
    session_of(std::process::id())
}

/// True inside `cua __agent` (child session). The agent sets both signals.
pub fn in_child_session() -> bool {
    std::env::var("CUA_CHILD_SESSION")
        .map(|v| v == "1")
        .unwrap_or(false)
        || util::session().starts_with("child-")
}

/// `cua launch --exe msedge.exe` (or chrome.exe) inside the child session would
/// use the user's default profile and fail invisibly. Route it to the dedicated
/// command.
pub fn child_launch_guard(exe: &str) -> Option<String> {
    if !in_child_session() || !is_browser_exe(exe) {
        return None;
    }
    Some(format!(
        "refused: launching '{exe}' from the child session would use the user's default browser \
         profile, which another desktop may already hold. Use `cua child edge <url>` (CuaEdge or \
         CuaChrome profile) instead."
    ))
}

fn is_browser_exe(exe: &str) -> bool {
    let base = Path::new(exe)
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| exe.trim().to_ascii_lowercase());
    let stem = base.trim_end_matches(".exe");
    stem == "msedge" || stem == "chrome"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_args_keep_the_documented_switches_and_urls() {
        let args = build_args(
            Path::new(r"C:\Users\me\AppData\Local\cua\browser\CuaEdge"),
            true,
            None,
            &["https://example.com".into()],
            None,
        );
        assert_eq!(args[0], r"--user-data-dir=C:\Users\me\AppData\Local\cua\browser\CuaEdge");
        assert!(args.iter().any(|a| a == "--no-first-run"));
        assert!(args.iter().any(|a| a == "--no-default-browser-check"));
        assert!(args.iter().any(|a| a == "--new-window"));
        assert!(!args.iter().any(|a| a == "--disable-sync"));
        assert!(args.iter().any(|a| a == "--disable-background-mode"));
        assert!(args.iter().any(|a| a == "--force-renderer-accessibility"));
        assert_eq!(args.last().unwrap(), "https://example.com");
    }

    #[test]
    fn automation_is_opt_in() {
        let args = build_args(Path::new(r"C:\CuaEdge"), false, None, &[], None);
        assert!(!args.iter().any(|a| a == "--force-renderer-accessibility"));
    }

    #[test]
    fn cdp_args_bind_localhost_with_the_chosen_port() {
        let args = build_args(Path::new(r"C:\CuaEdge"), false, Some(9333), &[], None);
        assert!(args.iter().any(|a| a == "--remote-debugging-port=9333"));
        assert!(args.iter().any(|a| a == "--remote-debugging-address=127.0.0.1"));
        assert!(args
            .iter()
            .any(|a| a == "--remote-allow-origins=http://127.0.0.1:9333"));
        let plain = build_args(Path::new(r"C:\CuaEdge"), false, None, &[], None);
        assert!(!plain.iter().any(|a| a.starts_with("--remote-debugging")));
    }

    #[test]
    fn cdp_port_is_read_from_an_edg_command_line() {
        assert_eq!(
            cdp_port_from_cmdline(
                r#""C:\Edge\msedge.exe" --remote-debugging-port=9333 --no-first-run"#
            ),
            Some(9333)
        );
        assert_eq!(
            cdp_port_from_cmdline(r#"--remote-debugging-port="9444" --type=renderer"#),
            Some(9444)
        );
        assert_eq!(cdp_port_from_cmdline("--no-first-run"), None);
        assert_eq!(cdp_port_from_cmdline("--remote-debugging-port=0"), None);
    }

    #[test]
    fn cdp_version_payload_accepts_edge_and_chrome() {
        let edge = serde_json::json!({
            "Browser": "Edg/157.0.0.0",
            "Protocol-Version": "1.3",
            "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/browser/x"
        });
        let ep = endpoint_from_version(9333, &edge).unwrap();
        assert_eq!(ep.port, 9333);
        assert_eq!(ep.browser.as_deref(), Some("Edg/157.0.0.0"));
        assert_eq!(ep.protocol_version.as_deref(), Some("1.3"));
        assert!(ep.browser_ws.is_some());

        let chrome = serde_json::json!({ "Browser": "Chrome/157.0.0.0" });
        assert!(endpoint_from_version(9333, &chrome).is_some());
        let other = serde_json::json!({ "Browser": "Firefox/130.0" });
        assert!(endpoint_from_version(9333, &other).is_none());
    }

    #[test]
    fn command_line_quotes_paths_and_urls() {
        let exe = Path::new(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe");
        let args = vec![
            r"--user-data-dir=C:\Users\John Doe\AppData\Local\cua\browser\CuaEdge".to_string(),
            "https://example.com/a b".to_string(),
            "--no-first-run".to_string(),
        ];
        let cl = build_command_line(exe, &args);
        assert!(cl.starts_with(
            r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" "#
        ));
        assert!(cl.contains(
            r#""--user-data-dir=C:\Users\John Doe\AppData\Local\cua\browser\CuaEdge""#
        ));
        assert!(cl.contains(r#""https://example.com/a b""#));
        assert!(cl.ends_with(" --no-first-run"));
    }

    #[test]
    fn quote_arg_follows_windows_rules() {
        assert_eq!(quote_arg("simple"), "simple");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg(r#"a "b" c"#), r#""a \"b\" c""#);
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }

    #[test]
    fn file_url_keeps_the_drive_and_encodes_spaces() {
        assert_eq!(
            file_url(Path::new(r"C:\Users\me\AppData\Local\cua\browser\sign-in.html")),
            "file:///C:/Users/me/AppData/Local/cua/browser/sign-in.html"
        );
        assert_eq!(
            file_url(Path::new(r"C:\Users\John Doe\sign-in.html")),
            "file:///C:/Users/John%20Doe/sign-in.html"
        );
    }

    #[test]
    fn sign_in_page_documents_the_flow_and_escapes_urls() {
        let html = sign_in_page_html(BrowserKind::Edge, &["https://example.com/?a=1&b=<2>".into()]);
        assert!(html.contains("cua child edge"));
        assert!(html.contains("关闭所有 CuaEdge Edge 窗口"));
        assert!(html.contains("同步默认开启"));
        assert!(html.contains("https://example.com/?a=1&amp;b=&lt;2&gt;"));
        assert!(!html.contains("a=1&b=<2>"));
    }

    #[test]
    fn html_escape_covers_the_four_entities() {
        assert_eq!(html_escape(r#"<&">"#), "&lt;&amp;&quot;&gt;");
    }

    #[test]
    fn holder_summary_prefers_browsers_and_counts_the_children() {
        let helper = Holder {
            pid: 11,
            session: 1,
            process: "msedge.exe".into(),
            started_ms: None,
            sources: Vec::new(),
            is_browser: true,
            browser: false,
            cdp_port: None,
        };
        let browser = Holder {
            pid: 22,
            session: 1,
            process: "msedge.exe".into(),
            started_ms: None,
            sources: Vec::new(),
            is_browser: true,
            browser: true,
            cdp_port: None,
        };
        let summary = holder_summary(&[&helper, &browser]);
        assert!(summary.contains("PID 22 in session 1"));
        assert!(!summary.contains("PID 11"));
        assert!(summary.contains("+1 more browser processes"));
    }

    #[test]
    fn command_lines_match_the_profile_with_quotes_case_and_slashes() {
        let p = Path::new(r"C:\Users\Me\AppData\Local\cua\browser\CuaEdge");
        assert!(cmdline_has_profile(
            r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --user-data-dir="C:\Users\Me\AppData\Local\cua\browser\CuaEdge" --no-first-run"#,
            p
        ));
        assert!(cmdline_has_profile(
            r"msedge.exe --user-data-dir=c:/users/me/appdata/local/CUA/browser/cuaedge/",
            p
        ));
        assert!(!cmdline_has_profile(
            r"msedge.exe --user-data-dir=C:\Users\Me\AppData\Local\cua\browser\CuaEdge2",
            p
        ));
    }

    #[test]
    fn urls_obey_the_limits_and_cannot_smuggle_switches() {
        assert!(validate_urls(&["https://example.com".into()]).is_ok());
        assert!(validate_urls(&["C:\\switches\\x".into()]).is_ok());
        assert!(validate_urls(&["--user-data-dir=C:\\other".into()]).is_err());
        assert!(validate_urls(&["".into()]).is_err());
        let too_many: Vec<String> = (0..MAX_URLS + 1).map(|i| format!("https://e{i}.example")).collect();
        assert!(validate_urls(&too_many).is_err());
    }

    #[test]
    fn browser_exe_detection_covers_paths_and_bare_names() {
        assert!(is_browser_exe("msedge.exe"));
        assert!(is_browser_exe("MSEDGE.EXE"));
        assert!(is_browser_exe(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"));
        assert!(is_browser_exe("msedge"));
        assert!(is_browser_exe("chrome.exe"));
        assert!(is_browser_exe(r"C:\Program Files\Google\Chrome\Application\chrome.exe"));
        assert!(!is_browser_exe("msedgewebview2.exe"));
        assert!(!is_browser_exe("chromedriver.exe"));
        assert!(!is_browser_exe(""));
    }

    #[test]
    fn metadata_roundtrips_and_keeps_created_at_for_the_same_profile() {
        let dir = std::env::temp_dir().join(format!(
            "cua-edge-meta-{}-{}",
            std::process::id(),
            util::now_iso().replace([':', '.', '-', 'T', 'Z'], "")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edge-profile.json");
        let profile = Path::new(r"C:\Users\me\AppData\Local\cua\browser\CuaEdge");
        let created = "2026-01-01T00:00:00.000Z".to_string();
        std::fs::write(
            &path,
            serde_json::to_vec(&ProfileMeta {
                schema: PROFILE_SCHEMA.into(),
                browser: "edge".into(),
                profile: profile.display().to_string(),
                browser_exe: r"C:\Edge\msedge.exe".into(),
                created_at: created.clone(),
                last_used_at: created.clone(),
                ephemeral: false,
                local_import_consent: Some("no".into()),
                consent_at: Some(created.clone()),
            })
            .unwrap(),
        )
        .unwrap();
        let meta = read_metadata_at(&path).unwrap();
        assert_eq!(meta.created_at, created);
        assert_eq!(meta.local_import_consent.as_deref(), Some("no"));
        assert!(paths_equal(Path::new(&meta.profile), profile));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn consent_fixture() -> ProfileMeta {
        ProfileMeta {
            schema: PROFILE_SCHEMA.into(),
            browser: "chrome".into(),
            profile: r"C:\Users\me\AppData\Local\cua\browser\CuaChrome".into(),
            browser_exe: r"C:\Chrome\chrome.exe".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            last_used_at: "2026-01-01T00:00:00.000Z".into(),
            ephemeral: false,
            local_import_consent: Some("no".into()),
            consent_at: Some("2026-01-01T00:00:00.000Z".into()),
        }
    }

    #[test]
    fn explicit_consent_flags_beat_the_recorded_decision() {
        assert_eq!(consent_flag(true, false), Some(true));
        assert_eq!(consent_flag(false, true), Some(false));
        assert_eq!(consent_flag(false, false), None);
        let meta = consent_fixture();
        assert_eq!(ImportConsent::from_meta(None), ImportConsent::Unrecorded);
        assert_eq!(ImportConsent::from_meta(Some(&meta)), ImportConsent::No);
        assert_eq!(
            ImportConsent::resolve(None, Some(&meta)),
            ImportConsent::No,
            "a recorded no survives later runs"
        );
        assert_eq!(
            ImportConsent::resolve(Some(true), Some(&meta)),
            ImportConsent::Yes,
            "an explicit flag overrides the record"
        );
        assert_eq!(ImportConsent::No.as_str(), "no");
    }

    #[test]
    fn purge_removes_imported_files_and_keeps_settings() {
        let dir = std::env::temp_dir().join(format!("cua-purge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Default").join("Network")).unwrap();
        std::fs::write(dir.join("Default").join("Login Data"), b"x").unwrap();
        std::fs::write(dir.join("Default").join("History"), b"x").unwrap();
        std::fs::write(dir.join("Default").join("Preferences"), b"keep").unwrap();
        std::fs::write(dir.join("Default").join("Network").join("Cookies"), b"x").unwrap();

        let removed = purge_imported_profile_data(&dir);
        assert!(removed.contains(&"Default/Login Data".to_string()));
        assert!(removed.contains(&"Default/History".to_string()));
        assert!(removed.contains(&"Default/Network/Cookies".to_string()));
        assert!(!dir.join("Default").join("Login Data").exists());
        assert!(dir.join("Default").join("Preferences").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browser_kinds_map_to_distinct_profiles_and_exes() {
        assert_eq!(BrowserKind::Edge.profile_dir(), "CuaEdge");
        assert_eq!(BrowserKind::Chrome.profile_dir(), "CuaChrome");
        assert_eq!(BrowserKind::Edge.exe_name(), "msedge.exe");
        assert_eq!(BrowserKind::Chrome.exe_name(), "chrome.exe");
        assert_eq!(BrowserKind::from_str("Chrome"), Some(BrowserKind::Chrome));
        assert_eq!(BrowserKind::from_str(" msedge "), Some(BrowserKind::Edge));
        assert_eq!(BrowserKind::from_str("firefox"), None);
        assert!(BrowserKind::is_browser_name("Chrome.exe"));
        assert!(!BrowserKind::is_browser_name("firefox.exe"));
    }

    #[test]
    fn chrome_candidates_use_chrome_install_paths() {
        let cands = candidates(BrowserKind::Chrome);
        let lower: Vec<String> = cands
            .iter()
            .map(|p| p.to_string_lossy().to_ascii_lowercase())
            .collect();
        assert!(lower.iter().any(|p| p.contains("google\\chrome\\application")));
        assert!(lower.iter().all(|p| p.ends_with("chrome.exe")));
        let edge_cands = candidates(BrowserKind::Edge);
        assert!(edge_cands
            .iter()
            .all(|p| p.to_string_lossy().to_ascii_lowercase().ends_with("msedge.exe")));
    }

    #[test]
    fn fresh_profiles_are_named_per_browser() {
        let edge = fresh_profile_path(BrowserKind::Edge);
        let chrome = fresh_profile_path(BrowserKind::Chrome);
        assert!(edge.to_string_lossy().contains("cua-edge-fresh"));
        assert!(chrome.to_string_lossy().contains("cua-chrome-fresh"));
    }

    #[test]
    fn load_extension_is_passed_before_urls() {
        let args = build_args(
            Path::new(r"C:\CuaEdge"),
            false,
            None,
            &["http://127.0.0.1:8765/__cua_pair?nonce=x".into()],
            Some(Path::new(r"D:\Codebase\cua\browser\extension")),
        );
        let ext = args
            .iter()
            .position(|a| a.starts_with("--load-extension="))
            .expect("--load-extension missing");
        let url = args
            .iter()
            .position(|a| a.starts_with("http://"))
            .expect("pair url missing");
        assert!(ext < url, "the switch must precede the first tab");
        assert_eq!(args[ext], r"--load-extension=D:\Codebase\cua\browser\extension");
    }

    #[test]
    fn browser_slots_are_per_browser() {
        assert_eq!(browser_slot(BrowserKind::Edge), "cuaedge");
        assert_eq!(browser_slot(BrowserKind::Chrome), "cuachrome");
    }

    #[test]
    fn first_existing_uses_the_first_installed_candidate() {
        let dir = std::env::temp_dir().join(format!("cua-edge-exe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("missing.exe");
        let b = dir.join("msedge.exe");
        std::fs::write(&b, b"x").unwrap();
        assert_eq!(first_existing(&[a, b.clone()]), Some(b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
