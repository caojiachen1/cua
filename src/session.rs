// session.rs — Windows Child Session backend.
//
// The child session itself is hosted by `cua-session-host.exe` (RDP ActiveX +
// ConnectToChildSession). This module owns the CLI side: setup/start/stop,
// viewer control, and the `cua child ...` proxy to the `cua __agent` process
// that runs inside the child session. The child agent executes the normal cua
// commands, so every existing command works with one extra prefix.
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, GetLastError, HANDLE};
use windows::Win32::System::Pipes::CallNamedPipeW;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::SessionOp;

const HOST_PIPE: &str = "cua-session-host";
const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
const ERROR_CANCELLED: u32 = 1223;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SessionState {
    #[serde(default)]
    pub host_pid: u32,
    #[serde(default)]
    pub host_pipe: String,
    #[serde(default)]
    pub child_session_id: u32,
    #[serde(default)]
    pub agent_pipe: String,
    #[serde(default)]
    pub agent_ready: bool,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub viewer_visible: bool,
    #[serde(default)]
    pub view_only: bool,
    #[serde(default)]
    pub cua_exe: String,
    #[serde(default)]
    pub data_dir: String,
    #[serde(default)]
    pub started: String,
    #[serde(default)]
    pub last_error: String,
}

#[link(name = "wtsapi32")]
extern "system" {
    fn WTSIsChildSessionsEnabled(enabled: *mut i32) -> i32;
}

fn state_path() -> PathBuf {
    crate::util::ensure_data_dir().join("session.json")
}

fn setup_path() -> PathBuf {
    crate::util::ensure_data_dir().join("session-setup.json")
}

fn error_path() -> PathBuf {
    crate::util::ensure_data_dir().join("session-error.json")
}

fn load_state() -> Option<SessionState> {
    std::fs::read(state_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Session id of the running child session, when one exists. Used to reject
/// `cua session edge --sign-in`/`--delete-profile` from inside that session.
pub fn child_session_id() -> Option<u32> {
    load_state()
        .map(|s| s.child_session_id)
        .filter(|id| *id > 0)
}

fn load_json(path: &Path) -> Option<Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

fn host_exe() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("CUA_SESSION_HOST") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    let current = std::env::current_exe().context("current_exe")?;
    let dir = current.parent().ok_or_else(|| anyhow!("cua.exe has no parent directory"))?;
    let p = dir.join("cua-session-host.exe");
    if p.is_file() {
        return Ok(p);
    }
    bail!(
        "cua-session-host.exe not found next to cua.exe ({}). Re-run install.ps1.",
        p.display()
    )
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wts_enabled() -> Result<bool> {
    let mut enabled = 0i32;
    let ok = unsafe { WTSIsChildSessionsEnabled(&mut enabled) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        bail!("WTSIsChildSessionsEnabled failed (0x{:08x})", err.0);
    }
    Ok(enabled != 0)
}

fn call_pipe(name: &str, body: &str, timeout_ms: u32) -> Result<String> {
    let name = wide(&format!("\\\\.\\pipe\\{name}"));
    let input = format!("{body}\n").into_bytes();
    let mut out = vec![0u8; 32 * 1024 * 1024];
    let mut read = 0u32;
    let ok = unsafe {
        CallNamedPipeW(
            PCWSTR(name.as_ptr()),
            Some(input.as_ptr() as *const _),
            input.len() as u32,
            Some(out.as_mut_ptr() as *mut _),
            out.len() as u32,
            &mut read,
            timeout_ms,
        )
    };
    if !ok.as_bool() {
        let err = unsafe { GetLastError() };
        if err == ERROR_PIPE_BUSY {
            bail!("pipe '{name:?}' is busy");
        }
        bail!("child session pipe unavailable (0x{:08x})", err.0);
    }
    out.truncate(read as usize);
    String::from_utf8(out).map_err(|e| anyhow!("pipe returned non-UTF-8: {e}"))
}

fn host_alive(state: &SessionState) -> bool {
    let pipe = if state.host_pipe.is_empty() { HOST_PIPE } else { &state.host_pipe };
    call_pipe(pipe, r#"{"op":"status"}"#, 3000)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["ok"].as_bool())
        .unwrap_or(false)
}

fn remove_stale_files() {
    let _ = std::fs::remove_file(state_path());
    let _ = std::fs::remove_file(error_path());
}

fn read_error() -> Option<String> {
    load_json(&error_path()).and_then(|v| {
        v["error"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| Some(v.to_string()))
    })
}

fn runas_wait(exe: &Path, args: &str, cwd: &Path) -> Result<()> {
    let verb = wide("runas");
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let params = wide(args);
    let dir: Vec<u16> = cwd.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: PCWSTR(dir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            if e.code().0 as u32 == 0x8007_04C7 || GetLastError().0 == ERROR_CANCELLED {
                anyhow!("administrator approval was cancelled")
            } else {
                anyhow!("ShellExecuteExW(runas) failed: {e}")
            }
        })?;
        if !info.hProcess.is_invalid() {
            let _ = WaitForSingleObject(info.hProcess, INFINITE);
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

fn print_value(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()));
}

fn host_call(state: &SessionState, op: &str, extra: Value) -> Result<Value> {
    let mut req = json!({ "op": op });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            req[k] = v.clone();
        }
    }
    let body = serde_json::to_string(&req)?;
    let pipe = if state.host_pipe.is_empty() { HOST_PIPE } else { &state.host_pipe };
    let text = call_pipe(pipe, &body, 15_000)?;
    serde_json::from_str(&text).with_context(|| format!("bad host response: {text}"))
}

pub fn dispatch(op: SessionOp) -> Result<i32> {
    match op {
        SessionOp::Setup { enable_rdp } => cmd_setup(enable_rdp),
        SessionOp::Start { view: _, hide, interactive, keep_startup, width, height } => {
            cmd_start(!hide, interactive, keep_startup, width, height)
        }
        SessionOp::Login => cmd_credentials(true),
        SessionOp::Logout => cmd_credentials(false),
        SessionOp::Stop => cmd_stop(),
        SessionOp::Status => cmd_status(),
        SessionOp::View { interactive } => cmd_view(interactive),
        SessionOp::Hide => cmd_simple("hide"),
        SessionOp::Takeover => cmd_simple("takeover"),
        SessionOp::Release => cmd_simple("release"),
        SessionOp::Edge { sign_in, status, delete_profile, confirm, urls, timeout_min, browser } => {
            crate::edge::cmd_session_edge(crate::edge::SessionEdgeOptions {
                sign_in,
                status,
                delete_profile,
                confirm,
                urls,
                timeout_min,
                browser,
            })
        }
    }
}

fn cmd_setup(enable_rdp: bool) -> Result<i32> {
    let host = host_exe()?;
    let dir = crate::util::ensure_data_dir();
    let _ = std::fs::remove_file(setup_path());
    let enabled = wts_enabled()?;

    // Always run the helper: even when Child Sessions are already enabled it
    // re-checks Remote Desktop, RDP Wrapper and the sign-in state. Only the
    // first enable needs elevation.
    if enabled && !enable_rdp {
        let status = Command::new(&host)
            .arg("--setup")
            .arg("--data-dir")
            .arg(&dir)
            .status()
            .with_context(|| format!("run {}", host.display()))?;
        if !status.success() && !setup_path().exists() {
            bail!("setup helper exited with {status}");
        }
    } else {
        let mut args = format!("--setup --data-dir \"{}\"", dir.display());
        if enable_rdp {
            args.push_str(" --enable-rdp");
        }
        if let Err(e) = runas_wait(&host, &args, &dir) {
            print_value(&json!({
                "ok": false,
                "schema": "cua-session-setup-v1",
                "error": e.to_string()
            }));
            return Ok(1);
        }
    }
    match load_json(&setup_path()) {
        Some(v) => {
            let ok = v["ok"].as_bool().unwrap_or(false);
            print_value(&v);
            Ok(if ok { 0 } else { 1 })
        }
        None => {
            print_value(&json!({
                "ok": false,
                "schema": "cua-session-setup-v1",
                "error": "setup helper produced no result; run cua session setup from an elevated terminal"
            }));
            Ok(1)
        }
    }
}

fn cmd_credentials(login: bool) -> Result<i32> {
    let host = host_exe()?;
    let dir = crate::util::ensure_data_dir();
    let mut cmd = Command::new(&host);
    cmd.arg(if login { "--login" } else { "--logout" })
        .arg("--data-dir")
        .arg(&dir);
    let status = cmd
        .status()
        .with_context(|| format!("run {}", host.display()))?;
    if !status.success() {
        print_value(&json!({
            "ok": false,
            "schema": "cua-session-credentials-v1",
            "error": if login { "credential save was cancelled or failed" } else { "credential deletion failed" }
        }));
        return Ok(1);
    }
    print_value(&json!({
        "ok": true,
        "schema": "cua-session-credentials-v1",
        "saved": login,
        "cleared": !login
    }));
    Ok(0)
}

fn cmd_start(
    view: bool,
    interactive: bool,
    keep_startup: bool,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<i32> {
    if let Some(st) = load_state() {
        if host_alive(&st) {
            if view || interactive {
                let v = host_call(&st, "view", json!({ "view_only": !interactive }))?;
                print_value(&v);
            } else {
                let v = host_call(&st, "status", json!({}))?;
                print_value(&v);
            }
            return Ok(0);
        }
        remove_stale_files();
    }

    if !wts_enabled()? {
        bail!(
            "Child Sessions are disabled. Run `cua session setup`, approve the administrator prompt, then sign out of Windows and sign back in."
        );
    }

    let host = host_exe()?;
    let dir = crate::util::ensure_data_dir();
    let _ = std::fs::remove_file(error_path());
    let _ = std::fs::remove_file(state_path());
    let cua = std::env::current_exe().context("current_exe")?;

    let mut cmd = Command::new(&host);
    cmd.arg("--start")
        .arg("--cua-exe")
        .arg(&cua)
        .arg("--data-dir")
        .arg(&dir);
    if let Some(width) = width {
        cmd.arg("--width").arg(width.clamp(640, 8192).to_string());
    }
    if let Some(height) = height {
        cmd.arg("--height").arg(height.clamp(480, 8192).to_string());
    }
    if keep_startup {
        cmd.arg("--keep-startup");
    }
    if interactive {
        cmd.arg("--interactive");
    } else if view {
        cmd.arg("--view");
    }
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    cmd.spawn()
        .with_context(|| format!("start {}", host.display()))?;

    let deadline = Instant::now() + Duration::from_secs(45);
    let mut latest = None;
    loop {
        if let Some(err) = read_error() {
            bail!("{err}");
        }
        if let Some(st) = load_state() {
            let ready = st.connected && st.child_session_id > 0;
            latest = Some(st.clone());
            if ready && st.agent_ready {
                print_value(&serde_json::to_value(&st)?);
                return Ok(0);
            }
        }
        if Instant::now() > deadline {
            if let Some(st) = latest {
                print_value(&serde_json::to_value(&st)?);
                return Ok(if st.connected { 0 } else { 1 });
            }
            bail!(
                "child-session host did not become ready within 45s. Check `cua session status` and {}",
                error_path().display()
            );
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn cmd_status() -> Result<i32> {
    let Some(st) = load_state() else {
        print_value(&json!({
            "ok": false,
            "schema": "cua-session-status-v1",
            "error": "no child session; run `cua session start`"
        }));
        return Ok(1);
    };
    match host_call(&st, "status", json!({})) {
        Ok(v) => {
            print_value(&v);
            Ok(if v["ok"].as_bool().unwrap_or(false) { 0 } else { 1 })
        }
        Err(e) => {
            print_value(&json!({
                "ok": false,
                "schema": "cua-session-status-v1",
                "host_alive": false,
                "child_session_id": st.child_session_id,
                "agent_pipe": st.agent_pipe,
                "error": e.to_string()
            }));
            Ok(1)
        }
    }
}

fn cmd_view(interactive: bool) -> Result<i32> {
    let Some(st) = load_state() else {
        bail!("no child session; run `cua session start --view`");
    };
    let v = host_call(&st, "view", json!({ "view_only": !interactive }))?;
    print_value(&v);
    Ok(0)
}

fn cmd_simple(op: &str) -> Result<i32> {
    let Some(st) = load_state() else {
        bail!("no child session; run `cua session start`");
    };
    let v = host_call(&st, op, json!({}))?;
    print_value(&v);
    Ok(if v["ok"].as_bool().unwrap_or(false) { 0 } else { 1 })
}

fn cmd_stop() -> Result<i32> {
    let Some(st) = load_state() else {
        print_value(&json!({ "ok": true, "message": "no child session" }));
        return Ok(0);
    };
    let result = host_call(&st, "stop", json!({}));
    match result {
        Ok(v) => {
            print_value(&v);
            let deadline = Instant::now() + Duration::from_secs(5);
            while state_path().exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = std::fs::remove_file(state_path());
            Ok(0)
        }
        Err(e) => {
            let _ = std::fs::remove_file(state_path());
            print_value(&json!({
                "ok": true,
                "host_alive": false,
                "message": format!("host was not reachable; stale state removed ({e})")
            }));
            Ok(0)
        }
    }
}

/// `cua child <args...>` and `cua --child <args...>` both route through here.
/// Returns None when this is not a child invocation.
pub fn maybe_child(raw: &[String]) -> Option<i32> {
    let mut rest: Vec<String> = raw.iter().skip(1).cloned().collect();
    let mut child = false;
    if rest.first().map(|s| s == "child").unwrap_or(false) {
        rest.remove(0);
        child = true;
    }
    if let Some(pos) = rest.iter().position(|s| s == "--child") {
        rest.remove(pos);
        child = true;
    }
    if !child {
        return None;
    }
    if rest.is_empty() {
        eprintln!("usage: cua child <cua command> [args...]");
        return Some(1);
    }
    match exec_remote(&rest) {
        Ok(code) => Some(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            Some(1)
        }
    }
}

fn exec_remote(args: &[String]) -> Result<i32> {
    let st = load_state().ok_or_else(|| anyhow!("no child session; run `cua session start`"))?;
    if st.agent_pipe.is_empty() || (!st.agent_ready && st.child_session_id == 0) {
        bail!("child agent is not ready; run `cua session status`");
    }
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let req = json!({
        "op": "exec",
        "argv": args,
        "cwd": cwd,
        "timeout_ms": 180000
    });
    let text = call_pipe(&st.agent_pipe, &serde_json::to_string(&req)?, 190_000)
        .with_context(|| format!("child agent pipe '{}'", st.agent_pipe))?;
    let v: Value = serde_json::from_str(&text).with_context(|| format!("bad agent response: {text}"))?;
    if let Some(stdout) = v["stdout"].as_str() {
        print!("{stdout}");
    }
    if let Some(stderr) = v["stderr"].as_str() {
        eprint!("{stderr}");
    }
    if let Some(err) = v["error"].as_str() {
        eprintln!("error: {err}");
    }
    Ok(v["code"].as_i64().unwrap_or(1) as i32)
}
