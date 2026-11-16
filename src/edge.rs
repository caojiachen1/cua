//! edge.rs — an independent, persistent Edge profile for the child session.
//!
//! The user's own Edge profile can only be opened by one desktop at a time.
//! `cua edge` therefore gives the child session its own `CuaEdge` profile,
//! launches a real Edge window inside the session that runs the command, and
//! refuses to start when another Windows session already holds that profile.
//!
//! First version deliberately uses the existing UIA/screenshot/real-input
//! commands (`cua child state/click/set-value/...`) instead of CDP or UBB.
//! `cua browser` stays the UBB bridge to the user's own browser.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
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
    PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};

use crate::util::{self, Receipt};

const PROFILE_SCHEMA: &str = "cua-edge-profile-v1";
const STATUS_SCHEMA: &str = "cua-edge-status-v1";
const MAX_URLS: usize = 8;
const WAIT_STEP: Duration = Duration::from_millis(150);

pub struct EdgeOptions {
    pub urls: Vec<String>,
    pub fresh_profile: bool,
    pub profile: Option<String>,
    pub automation: bool,
    pub cdp: bool,
    pub status: bool,
    pub wait_ms: u64,
}

/// `cua child edge ...` entry point. The child agent executes `cua edge ...`
/// inside the child session, so everything here runs in the target desktop.
pub fn cmd_edge(opts: EdgeOptions) -> Result<i32> {
    let edge = resolve_edge();
    let profile = resolve_profile(opts.profile.as_deref(), opts.fresh_profile);
    let wait_ms = opts.wait_ms.min(300_000);

    if opts.status {
        if !opts.urls.is_empty() {
            let r = Receipt::new("edge").refused(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
                "`edge --status` does not launch Edge; remove URL arguments",
            );
            return emit(r, launch_extra(edge.as_deref(), &profile, &scan_profile(&profile), None, false, false, false));
        }
        return print_status(edge.as_deref(), &profile, opts.fresh_profile);
    }

    if let Err(e) = validate_urls(&opts.urls) {
        let r = Receipt::new("edge").refused(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            &e,
        );
        return emit(r, launch_extra(edge.as_deref(), &profile, &scan_profile(&profile), None, false, false, opts.automation));
    }

    let Some(edge) = edge else {
        let r = Receipt::new("edge").fail(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            &format!(
                "msedge.exe not found. Checked $CUA_EDGE_EXE, config `edge_exe`, \
                 App Paths\\msedge.exe and the standard Program Files paths"
            ),
        );
        return emit(r, launch_extra(None, &profile, &scan_profile(&profile), None, false, opts.fresh_profile, opts.automation));
    };

    let current = current_session();
    let (before, refusal) = inspect_profile(&profile, current);
    if let Some(detail) = refusal {
        let r = Receipt::new("edge").refused(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        return emit(r, launch_extra(Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
    }

    let same: Vec<&Holder> = before
        .holders
        .iter()
        .filter(|h| h.is_edge && h.session == current)
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
                return emit(r, launch_extra(Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
            }
        }
    } else {
        None
    };

    let launched_pid = match spawn_edge(&edge, &profile, opts.automation, cdp_port, &opts.urls) {
        Ok(pid) => pid,
        Err(e) => {
            let r = Receipt::new("edge").fail(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                &format!("{e:#}"),
            );
            return emit(r, launch_extra(Some(edge.as_path()), &profile, &before, None, false, opts.fresh_profile, opts.automation));
        }
    };

    if !opts.fresh_profile {
        let _ = write_metadata(&profile, &edge);
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
            "Edge was spawned (PID {launched_pid}), but the CuaEdge profile was taken by \
             session {} (PID {}). Close that Edge window and retry.",
            h.session, h.pid
        );
        let r = Receipt::new("edge").fail(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        let extra = launch_extra(Some(edge.as_path()), &profile, &after, Some(launched_pid), already_running, opts.fresh_profile, opts.automation);
        return emit(r, extra);
    }

    if !already_running && wait_ms > 0 && holder.is_none() {
        let detail = format!(
            "Edge was spawned (PID {launched_pid}), but no process using '{}' appeared within {} ms. \
             Run `cua edge --status` and check whether Edge showed an error dialog.",
            profile.display(),
            wait_ms
        );
        let r = Receipt::new("edge").fail(
            json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
            &detail,
        );
        let extra = launch_extra(Some(edge.as_path()), &profile, &after, Some(launched_pid), already_running, opts.fresh_profile, opts.automation);
        return emit(r, extra);
    }

    let detail = if already_running {
        let h = best_of(same);
        format!(
            "opened a new Edge window in the existing CuaEdge instance (PID {}, session {})",
            h.pid, h.session
        )
    } else if let Some(h) = &holder {
        format!(
            "launched Microsoft Edge with the CuaEdge profile (launcher PID {launched_pid}; process using the profile at launch: PID {} in session {})",
            h.pid, h.session
        )
    } else {
        format!(
            "launched Microsoft Edge with the CuaEdge profile (launcher PID {launched_pid}); not waiting for the profile holder"
        )
    };
    let r = Receipt::new("edge").ok(
        json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
        &detail,
    );
    let mut extra = launch_extra(Some(edge.as_path()), &profile, &after, Some(launched_pid), already_running, opts.fresh_profile, opts.automation);
    let cdp = if opts.cdp || after.holders.iter().any(|h| h.is_edge && h.cdp_port.is_some()) {
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
            "the existing CuaEdge instance was not launched with --cdp; close it and relaunch with --cdp"
        } else {
            "CDP endpoint did not appear within 3s; UIA still works"
        });
    }
    emit(r, extra)
}

fn launch_extra(
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
        "edge_exe": edge.map(|p| p.display().to_string()),
        "profile": profile.display().to_string(),
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
}

/// `cua session edge ...`: the user's main-session side of the CuaEdge profile.
/// `--sign-in` opens the profile where the user actually is, waits for every
/// Edge process using it to exit, and leaves `cua child edge` with the login.
pub fn cmd_session_edge(opts: SessionEdgeOptions) -> Result<i32> {
    let modes = [opts.sign_in, opts.status, opts.delete_profile]
        .iter()
        .filter(|b| **b)
        .count();
    if modes > 1 {
        return session_edge_refusal(
            "session-edge",
            "choose exactly one of --sign-in, --status, --delete-profile",
            json!({ "urls": opts.urls }),
        );
    }
    if opts.confirm && !opts.delete_profile {
        return session_edge_refusal(
            "session-edge",
            "--confirm is only valid with --delete-profile",
            json!({ "urls": opts.urls }),
        );
    }

    let profile = default_profile();
    let edge = resolve_edge();

    if opts.status || modes == 0 {
        if !opts.urls.is_empty() {
            return session_edge_refusal(
                "session-edge",
                "--status does not launch Edge; remove URL arguments",
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
        return print_status(edge.as_deref(), &profile, false);
    }

    if opts.delete_profile {
        if !opts.urls.is_empty() {
            return session_edge_refusal(
                "session-edge-delete",
                "--delete-profile does not launch Edge; remove URL arguments",
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
        if !opts.confirm {
            return session_edge_refusal(
                "session-edge-delete",
                "--delete-profile removes the CuaEdge cookies/passwords; pass --confirm to proceed",
                json!({ "profile": profile.display().to_string() }),
            );
        }
        return delete_session_profile(edge.as_deref(), &profile);
    }

    // sign-in
    if let Some(reason) = main_session_guard() {
        return session_edge_refusal(
            "session-edge-signin",
            &reason,
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
        );
    }
    if let Err(e) = validate_urls(&opts.urls) {
        return session_edge_refusal(
            "session-edge-signin",
            &e,
            json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
        );
    }
    let Some(edge) = edge else {
        return session_edge_failure(
            "session-edge-signin",
            "msedge.exe not found. Checked $CUA_EDGE_EXE, config `edge_exe`, App Paths\\msedge.exe and the standard Program Files paths",
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
            session_edge_extra(Some(edge.as_path()), &profile, json!({
                "lockfile": scan.lockfile.as_str(),
                "holders": scan.holders,
            })),
        );
    }

    let page = match write_sign_in_page(&opts.urls) {
        Ok(p) => p,
        Err(e) => {
            return session_edge_failure(
                "session-edge-signin",
                &format!("could not write the sign-in page: {e:#}"),
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
    };
    let mut tabs = vec![file_url(&page)];
    tabs.extend(opts.urls.iter().cloned());

    let launched_pid = match spawn_edge(&edge, &profile, false, None, &tabs) {
        Ok(pid) => pid,
        Err(e) => {
            return session_edge_failure(
                "session-edge-signin",
                &format!("{e:#}"),
                json!({ "profile": profile.display().to_string(), "urls": opts.urls }),
            );
        }
    };
    let _ = write_metadata(&profile, &edge);
    let timeout_min = opts.timeout_min.clamp(1, 180);
    let started = Instant::now();
    let outcome = wait_for_sign_in(&profile, started, Duration::from_secs(timeout_min * 60));

    match outcome {
        SignInOutcome::Closed { scan, waited } => {
            let _ = write_metadata(&profile, &edge);
            let detail = format!(
                "CuaEdge sign-in finished: every Edge process using the profile exited after {:.1}s. \
                 The profile keeps the cookies/passwords saved in that window; run `cua child edge` \
                 to use the same login in the child session.",
                waited.as_secs_f64()
            );
            let r = Receipt::new("session-edge-signin").ok(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                &detail,
            );
            emit(
                r,
                session_edge_extra(Some(edge.as_path()), &profile, json!({
                    "sign_in_page": page.display().to_string(),
                    "launched_pid": launched_pid,
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
            let edge_holders: Vec<&Holder> =
                scan.holders.iter().filter(|h| h.is_edge).collect();
            let list = if edge_holders.is_empty() {
                "none (the profile lock may be held by a non-Edge process)".to_string()
            } else {
                holder_summary(&edge_holders)
            };
            let detail = if seen {
                format!(
                    "CuaEdge sign-in is still open after {} min. Edge processes using '{}': {}. \
                     cua never kills them; close those Edge windows (or end the PIDs yourself), \
                     then run `cua session edge --status`.",
                    timeout_min,
                    profile.display(),
                    list
                )
            } else {
                format!(
                    "Edge was spawned (PID {launched_pid}) but no process using '{}' appeared within 20s. \
                     Check whether Edge showed an error dialog, then run `cua session edge --status`.",
                    profile.display()
                )
            };
            let r = Receipt::new("session-edge-signin").fail(
                json!({ "profile": profile.display().to_string(), "urls": opts.urls, "session": current }),
                &detail,
            );
            emit(
                r,
                session_edge_extra(Some(edge.as_path()), &profile, json!({
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

fn session_edge_refusal(cmd: &str, detail: &str, fields: Value) -> Result<i32> {
    let r = Receipt::new(cmd).refused(fields.clone(), detail);
    emit(r, session_edge_extra(None, &default_profile(), fields))
}

fn session_edge_failure(cmd: &str, detail: &str, fields: Value) -> Result<i32> {
    let r = Receipt::new(cmd).fail(fields.clone(), detail);
    emit(r, session_edge_extra(None, &default_profile(), fields))
}

fn session_edge_extra(edge: Option<&Path>, profile: &Path, fields: Value) -> Value {
    let mut v = json!({
        "backend": "native",
        "edge_exe": edge.map(|p| p.display().to_string()),
        "profile": profile.display().to_string(),
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
        if scan.holders.iter().any(|h| h.is_edge) {
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

fn delete_session_profile(edge: Option<&Path>, profile: &Path) -> Result<i32> {
    if let Some(reason) = main_session_guard() {
        return session_edge_refusal(
            "session-edge-delete",
            &reason,
            json!({ "profile": profile.display().to_string() }),
        );
    }
    let current = current_session();
    let scan = scan_profile(profile);
    let holders: Vec<&Holder> = scan.holders.iter().filter(|h| h.is_edge).collect();
    if !holders.is_empty() {
        let list = holder_summary(&holders);
        let detail = format!(
            "refusing to delete '{}': Edge is still using it ({list}). Close those Edge windows first.",
            profile.display()
        );
        let r = Receipt::new("session-edge-delete").refused(
            json!({ "profile": profile.display().to_string(), "session": current }),
            &detail,
        );
        return emit(
            r,
            session_edge_extra(edge, profile, json!({
                "lockfile": scan.lockfile.as_str(),
                "restart_manager_ok": scan.rm_ok,
                "holders": scan.holders,
            })),
        );
    }
    if !matches!(scan.lockfile, LockFileState::Absent | LockFileState::Stale) {
        let detail = format!(
            "refusing to delete '{}': its lockfile is {} (held or unreadable). Close any Edge using \
             the profile, then retry.",
            profile.display(),
            scan.lockfile.as_str()
        );
        let r = Receipt::new("session-edge-delete").refused(
            json!({ "profile": profile.display().to_string(), "session": current }),
            &detail,
        );
        return emit(
            r,
            session_edge_extra(edge, profile, json!({
                "lockfile": scan.lockfile.as_str(),
                "restart_manager_ok": scan.rm_ok,
                "holders": scan.holders,
            })),
        );
    }

    // A junction/symlink could point at the user's real Edge profile; only
    // delete a path that really lives under cua's browser directory.
    let browser_dir = util::ensure_data_dir().join("browser");
    let canon_browser = std::fs::canonicalize(&browser_dir).unwrap_or(browser_dir);
    let exists = profile.exists();
    if exists {
        if let Ok(md) = std::fs::symlink_metadata(profile) {
            if md.file_type().is_symlink() {
                return session_edge_refusal(
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
            "deleted CuaEdge profile '{}' (its cookies and saved passwords are gone)",
            profile.display()
        )
    } else {
        "CuaEdge profile did not exist; nothing to delete".to_string()
    };
    let r = Receipt::new("session-edge-delete").ok(
        json!({ "profile": profile.display().to_string(), "session": current }),
        &detail,
    );
    emit(
        r,
        session_edge_extra(edge, profile, json!({
            "deleted": exists,
            "metadata_removed": meta_removed,
        })),
    )
}

// ---------------------------------------------------------------- sign-in page

fn write_sign_in_page(urls: &[String]) -> Result<PathBuf> {
    let path = util::ensure_data_dir().join("browser").join("sign-in.html");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    std::fs::write(&path, sign_in_page_html(urls))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn sign_in_page_html(urls: &[String]) -> String {
    let list = if urls.is_empty() {
        String::new()
    } else {
        let items = urls
            .iter()
            .map(|u| format!("<li><code>{}</code></li>", html_escape(u)))
            .collect::<String>();
        format!("<p>本次已打开的站点：</p><ul>{items}</ul>")
    };
    format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<title>cua · CuaEdge 登录</title>
<style>
body{{font:15px/1.7 system-ui,"Segoe UI",sans-serif;background:#1b1a20;color:#eee;max-width:760px;margin:40px auto;padding:0 24px}}
h1{{font-size:22px;font-weight:600}} code{{background:#2b2a33;padding:2px 5px;border-radius:4px}}
li{{margin:6px 0}} .warn{{color:#ffcf70}} .muted{{color:#aaa}}
</style></head><body>
<h1>CuaEdge 登录</h1>
<p>这是 cua 为子会话准备的<strong>独立 Edge 配置</strong>。请在这个窗口里登录需要的站点，2FA 也在这里完成。</p>
<ol>
  <li>在其它标签页完成登录。</li>
  <li>登录完成后，<strong>关闭所有 CuaEdge Edge 窗口</strong>（包括这个说明页）。</li>
  <li>cua 会等到所有使用该 profile 的 Edge 进程退出后返回。</li>
  <li>之后在子会话运行 <code>cua child edge</code>，会复用这里的 cookie 和已保存密码。</li>
</ol>
<p class="warn">cua 不会读取、打印或保存你的密码；主 Edge 的 cookie/密码不会自动复制到这里。不要把不想让子会话访问的账号登录到这个 profile。</p>
<p class="muted">Edge 可能通过 Windows 账号自动 SSO；同步默认开启，同一微软账号保存的密码可能同步到 CuaEdge。关闭窗口后如果命令仍不返回，运行 <code>cua session edge --status</code> 查看持有 profile 的 PID。</p>
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

fn print_status(edge: Option<&Path>, profile: &Path, fresh: bool) -> Result<i32> {
    let scan = scan_profile(profile);
    let meta = read_metadata();
    let current = current_session();
    let other = holder_outside(&scan, current);
    let same = holder_in(&scan, current);
    let note = if edge.is_none() {
        "msedge.exe not found; checked $CUA_EDGE_EXE, config `edge_exe`, App Paths\\msedge.exe and the standard Program Files paths".to_string()
    } else if let Some(h) = other {
        format!(
            "profile is held by Edge in session {} (PID {}); close it before launching",
            h.session, h.pid
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
    print_status_value(edge, profile, fresh, &scan, meta, current, &note)
}

fn print_status_value(
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
        "edge_exe": edge.map(|p| p.display().to_string()),
        "profile": profile.display().to_string(),
        "metadata_path": metadata_path().display().to_string(),
        "metadata": meta,
        "metadata_matches_profile": meta_matches,
        "current_session": current,
        "child_session": in_child_session(),
        "fresh_profile": fresh,
        "lockfile": scan.lockfile.as_str(),
        "restart_manager_ok": scan.rm_ok,
        "edge_processes": scan.edge_processes,
        "holders": scan.holders,
        "cdp": cdp,
        "note": note,
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(if edge.is_some() { 0 } else { 1 })
}

// ---------------------------------------------------------------- profile

fn default_profile() -> PathBuf {
    util::ensure_data_dir().join("browser").join("CuaEdge")
}

fn resolve_profile(override_path: Option<&str>, fresh: bool) -> PathBuf {
    if fresh {
        fresh_profile_path()
    } else if let Some(p) = override_path.filter(|p| !p.trim().is_empty()) {
        let p = absolutize(Path::new(p.trim()));
        p
    } else {
        default_profile()
    }
}

fn fresh_profile_path() -> PathBuf {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("cua-edge-fresh-{}-{ms}", std::process::id()))
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
    schema: String,
    browser: String,
    profile: String,
    browser_exe: String,
    created_at: String,
    last_used_at: String,
    #[serde(default)]
    ephemeral: bool,
}

fn metadata_path() -> PathBuf {
    util::ensure_data_dir().join("browser").join("edge-profile.json")
}

fn read_metadata() -> Option<ProfileMeta> {
    read_metadata_at(&metadata_path())
}

fn read_metadata_at(path: &Path) -> Option<ProfileMeta> {
    let b = std::fs::read(path).ok()?;
    let b = b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b);
    serde_json::from_slice(b).ok()
}

fn write_metadata(profile: &Path, edge: &Path) -> Result<()> {
    let path = metadata_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let created_at = read_metadata_at(&path)
        .filter(|m| paths_equal(Path::new(&m.profile), profile))
        .map(|m| m.created_at)
        .unwrap_or_else(util::now_iso);
    let meta = ProfileMeta {
        schema: PROFILE_SCHEMA.into(),
        browser: "edge".into(),
        profile: profile.display().to_string(),
        browser_exe: edge.display().to_string(),
        created_at,
        last_used_at: util::now_iso(),
        ephemeral: false,
    };
    let mut bytes = serde_json::to_vec_pretty(&meta)?;
    bytes.push(b'\n');
    std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------- Edge lookup

fn resolve_edge() -> Option<PathBuf> {
    first_existing(&edge_candidates())
}

fn edge_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("CUA_EDGE_EXE") {
        push_candidate(&mut out, &p);
    }
    if let Some(p) = util::load_config_json().get("edge_exe").and_then(Value::as_str) {
        push_candidate(&mut out, p);
    }
    for path in [
        "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\msedge.exe",
        "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths\\msedge.exe",
    ] {
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Some(p) = reg_value_string(hive, path) {
                push_candidate(&mut out, &p);
            }
        }
    }
    for base in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(dir) = std::env::var(base) {
            push_candidate(&mut out, &format!("{dir}\\Microsoft\\Edge\\Application\\msedge.exe"));
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

fn spawn_edge(
    exe: &Path,
    profile: &Path,
    automation: bool,
    cdp_port: Option<u16>,
    urls: &[String],
) -> Result<u32> {
    let args = build_args(profile, automation, cdp_port, urls);
    spawn_edge_args(exe, &args)
}

fn spawn_edge_args(exe: &Path, args: &[String]) -> Result<u32> {
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
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
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
        .filter(|h| h.is_edge)
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
        h.is_edge
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
    let is_edge = browser
        .as_deref()
        .map(|b| {
            let b = b.to_ascii_lowercase();
            b.contains("edg/") || b.contains("edge")
        })
        .unwrap_or(false);
    if !is_edge {
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
    is_edge: bool,
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
    edge_processes: usize,
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
    let procs = edge_processes();
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
                    is_edge: is_edge_name(&p.name),
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
        edge_processes: procs.len(),
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
        e.is_edge |= h.is_edge;
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
        .filter(|h| h.is_edge && h.session == session)
        .collect();
    best_of_opt(cands)
}

fn holder_outside(scan: &Scan, session: u32) -> Option<&Holder> {
    let cands = scan
        .holders
        .iter()
        .filter(|h| h.is_edge && h.session != session)
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
        out.push_str(&format!(" (+{extra} more Edge processes)"));
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
        && !scan.holders.iter().any(|h| h.is_edge)
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

fn edge_processes() -> Vec<ProcInfo> {
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
                if is_edge_name(&name) {
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

fn is_edge_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("msedge.exe")
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
                let is_edge = is_edge_name(&name)
                    || app.to_ascii_lowercase().contains("microsoft edge")
                    || app.to_ascii_lowercase().contains("msedge");
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
                    is_edge,
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

/// `cua launch --exe msedge.exe` inside the child session would use the user's
/// default profile and fail invisibly. Route it to the dedicated command.
pub fn child_launch_guard(exe: &str) -> Option<String> {
    if !in_child_session() || !is_msedge_exe(exe) {
        return None;
    }
    Some(format!(
        "refused: launching '{exe}' from the child session would use the user's default Edge \
         profile, which another desktop may already hold. Use `cua child edge <url>` (CuaEdge \
         profile) instead."
    ))
}

fn is_msedge_exe(exe: &str) -> bool {
    let base = Path::new(exe)
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| exe.trim().to_ascii_lowercase());
    let stem = base.trim_end_matches(".exe");
    stem == "msedge"
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
        let args = build_args(Path::new(r"C:\CuaEdge"), false, None, &[]);
        assert!(!args.iter().any(|a| a == "--force-renderer-accessibility"));
    }

    #[test]
    fn cdp_args_bind_localhost_with_the_chosen_port() {
        let args = build_args(Path::new(r"C:\CuaEdge"), false, Some(9333), &[]);
        assert!(args.iter().any(|a| a == "--remote-debugging-port=9333"));
        assert!(args.iter().any(|a| a == "--remote-debugging-address=127.0.0.1"));
        assert!(args
            .iter()
            .any(|a| a == "--remote-allow-origins=http://127.0.0.1:9333"));
        let plain = build_args(Path::new(r"C:\CuaEdge"), false, None, &[]);
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
    fn cdp_version_payload_must_be_edge() {
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
        assert!(endpoint_from_version(9333, &chrome).is_none());
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
        let html = sign_in_page_html(&["https://example.com/?a=1&b=<2>".into()]);
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
            is_edge: true,
            browser: false,
            cdp_port: None,
        };
        let browser = Holder {
            pid: 22,
            session: 1,
            process: "msedge.exe".into(),
            started_ms: None,
            sources: Vec::new(),
            is_edge: true,
            browser: true,
            cdp_port: None,
        };
        let summary = holder_summary(&[&helper, &browser]);
        assert!(summary.contains("PID 22 in session 1"));
        assert!(!summary.contains("PID 11"));
        assert!(summary.contains("+1 more Edge processes"));
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
    fn msedge_detection_covers_paths_and_bare_names() {
        assert!(is_msedge_exe("msedge.exe"));
        assert!(is_msedge_exe("MSEDGE.EXE"));
        assert!(is_msedge_exe(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"));
        assert!(is_msedge_exe("msedge"));
        assert!(!is_msedge_exe("msedgewebview2.exe"));
        assert!(!is_msedge_exe("chrome.exe"));
        assert!(!is_msedge_exe(""));
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
            })
            .unwrap(),
        )
        .unwrap();
        let meta = read_metadata_at(&path).unwrap();
        assert_eq!(meta.created_at, created);
        assert!(paths_equal(Path::new(&meta.profile), profile));
        let _ = std::fs::remove_dir_all(&dir);
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
