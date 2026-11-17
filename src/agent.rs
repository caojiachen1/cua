// agent.rs — the process that runs inside the Windows child session.
//
// It exposes a named pipe (`cua-child-agent-<session-id>`) and executes normal
// cua commands in that session. The parent CLI stays in the user's session and
// proxies argv through this pipe, so every existing command is available as
// `cua child <command>` without duplicating the command surface.
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Console::FreeConsole;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_MESSAGE: usize = 32 * 1024 * 1024;

fn alog(m: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[t+{}s] {}
", ts, m);
    if let Ok(dir) = std::env::var("TEMP") {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::path::Path::new(&dir).join("cua-child-agent.log"))
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
    }
}

pub fn run(data_dir: Option<String>) -> Result<i32> {
    unsafe {
        let _ = FreeConsole();
    }
    alog("agent run start");

    let mut session_id = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }
        .map_err(|e| anyhow!("ProcessIdToSessionId: {e}"))?;
    if session_id == 0 {
        bail!("cannot determine the child session id");
    }

    let pipe_name = format!("cua-child-agent-{session_id}");
    let name = wide(&format!("\\\\.\\pipe\\{pipe_name}"));
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
            1,
            MAX_MESSAGE as u32,
            1024 * 1024,
            0,
            None,
        )
    };
    if handle.is_invalid() {
        bail!("CreateNamedPipeW failed (0x{:08x})", unsafe { GetLastError() }.0);
    }

    let session_tag = format!("child-{session_id}");
    let data_dir = data_dir
        .or_else(|| std::env::var("CUA_DATA_DIR").ok())
        .filter(|s| !s.trim().is_empty());
    let mut buffer = vec![0u8; MAX_MESSAGE];
    loop {
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        if connected.is_err() {
            let err = unsafe { GetLastError() };
            if err != ERROR_PIPE_CONNECTED {
                break;
            }
        }

        alog("loop: ConnectNamedPipe pending / reading");
        let response = match read_message(handle, &mut buffer) {
            Ok(0) => {
                alog("read: client closed (0 bytes)");
                None
            }
            Ok(n) => {
                alog(&format!("read: {} bytes", n));
                let text = String::from_utf8_lossy(&buffer[..n]).to_string();
                let request: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
                    json!({ "op": "error", "error": format!("bad JSON: {e}") })
                });
                Some(handle_request(&request, session_id, &session_tag, data_dir.as_deref()))
            }
            Err(e) => {
                alog(&format!("read error: {e}"));
                Some(json!({ "ok": false, "error": e.to_string(), "code": 1 }))
            }
        };

        if let Some(value) = response {
            if let Err(e) = write_message(handle, &serde_json::to_vec(&value)?) {
                let _ = e;
            }
        }

        unsafe {
            let _ = DisconnectNamedPipe(handle);
        }
        alog("loop: disconnected, next client");
    }

    unsafe {
        let _ = CloseHandle(handle);
    }
    Ok(0)
}

fn handle_request(request: &Value, session_id: u32, session_tag: &str, data_dir: Option<&str>) -> Value {
    let op = request["op"].as_str().unwrap_or("exec");
    match op {
        "ping" => json!({ "ok": true, "session_id": session_id, "agent": "cua" }),
        "stop" => json!({ "ok": true, "stopping": true }),
        "exec" => {
            let argv: Vec<String> = request["argv"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let cwd = request["cwd"].as_str().unwrap_or("").to_string();
            let timeout = request["timeout_ms"].as_u64().unwrap_or(180_000);
            match run_command(&argv, &cwd, timeout, session_id, session_tag, data_dir) {
                Ok(v) => v,
                Err(e) => json!({ "ok": false, "error": e.to_string(), "code": 1 }),
            }
        }
        other => json!({ "ok": false, "error": format!("unknown op: {other}"), "code": 1 }),
    }
}

fn run_command(
    argv: &[String],
    cwd: &str,
    timeout_ms: u64,
    session_id: u32,
    session_tag: &str,
    data_dir: Option<&str>,
) -> Result<Value> {
    if argv.is_empty() {
        bail!("empty argv");
    }
    match argv[0].as_str() {
        "session" | "__agent" | "child" => {
            bail!("child-session management commands cannot run inside the child session")
        }
        _ => {}
    }

    let exe = std::env::current_exe().context("current_exe")?;
    let mut cmd = Command::new(&exe);
    cmd.args(argv);
    if !cwd.is_empty() && std::path::Path::new(cwd).is_dir() {
        cmd.current_dir(cwd);
    }
    if let Some(dir) = data_dir {
        cmd.env("CUA_DATA_DIR", dir);
    }
    cmd.env("CUA_SESSION", session_tag)
        .env("CUA_CHILD_SESSION", "1")
        .env("CUA_CHILD_ID", session_id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    alog(&format!("exec: spawning {:?}", argv));
    let start = Instant::now();
    let mut child = cmd.spawn().with_context(|| format!("spawn {}", exe.display()))?;
    alog("exec: spawned");
    let mut out = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut err = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;
    // Collect output through a channel with a short post-exit deadline: a
    // grandchild process (e.g. a viewer spawned by `main view`) may inherit
    // the stdout write handle, so read_to_end would never see EOF and the
    // pipe loop would wedge forever waiting for join().
    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let err_tx = out_tx.clone();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = out_tx.send(buf);
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        let _ = err_tx.send(buf);
    });

    let deadline = start + Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            timed_out = true;
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    alog(&format!("exec: child exited (timed_out={timed_out})"));
    let collect_deadline = Duration::from_secs(2);
    let stdout = out_rx
        .recv_timeout(collect_deadline)
        .unwrap_or_else(|_| {
            alog("exec: stdout reader still open (inherited handle) — returning partial");
            Vec::new()
        });
    let stderr = out_rx
        .recv_timeout(collect_deadline)
        .unwrap_or_default();
    let mut stderr_text = String::from_utf8_lossy(&stderr).to_string();
    if timed_out {
        if !stderr_text.is_empty() && !stderr_text.ends_with('\n') {
            stderr_text.push('\n');
        }
        stderr_text.push_str(&format!("cua: child command timed out after {timeout_ms} ms\n"));
    }

    Ok(json!({
        "ok": !timed_out,
        "code": status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&stdout),
        "stderr": stderr_text,
        "elapsed_ms": start.elapsed().as_millis() as u64,
        "session_id": session_id
    }))
}

fn read_message(handle: HANDLE, buffer: &mut [u8]) -> Result<usize> {
    let mut read = 0u32;
    unsafe {
        ReadFile(handle, Some(buffer), Some(&mut read), None)
            .map_err(|e| anyhow!("ReadFile: {e}"))?;
    }
    Ok(read as usize)
}

fn write_message(handle: HANDLE, bytes: &[u8]) -> Result<()> {
    let mut written = 0u32;
    unsafe {
        WriteFile(handle, Some(bytes), Some(&mut written), None)
            .map_err(|e| anyhow!("WriteFile: {e}"))?;
        let _ = FlushFileBuffers(handle);
    }
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
