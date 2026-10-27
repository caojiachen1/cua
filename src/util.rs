// util.rs — receipts, cache/state-ids, safety policy (fusion of ZCode receipt schema + Qoder confirmation policy + Trae approval)
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECEIPT_SCHEMA: &str = "cua-action-receipt-v1";

pub fn data_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into())
    });
    std::path::Path::new(&base).join("cua")
}

pub fn ensure_data_dir() -> std::path::PathBuf {
    let d = data_dir();
    let _ = std::fs::create_dir_all(d.join("recordings"));
    let _ = std::fs::create_dir_all(d.join("cache"));
    d
}

pub fn now_iso() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let ms = d.subsec_millis();
    // civil-from-days (Howard Hinnant algorithm)
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dd = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let sod = secs % 86400;
    let (hh, mm, ss) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    format!("{y:04}-{m:02}-{dd:02}T{hh:02}:{mm:02}:{ss:02}.{ms:03}Z")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Receipt {
    pub schema: String,
    pub tool: String,
    pub cmd: String,
    pub ts: String,
    pub target: serde_json::Value,
    pub dispatched: bool,
    pub status: String, // ok | refused | failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_id: Option<String>,
}

impl Receipt {
    pub fn new(cmd: &str) -> Self {
        Receipt {
            schema: RECEIPT_SCHEMA.to_string(),
            tool: "cua".into(),
            cmd: cmd.into(),
            ts: now_iso(),
            target: serde_json::Value::Null,
            dispatched: false,
            status: "failed".into(),
            detail: None,
            elapsed_ms: None,
            state_id: None,
        }
    }
    pub fn ok(mut self, target: serde_json::Value, detail: &str) -> Self {
        self.target = target;
        self.dispatched = true;
        self.status = "ok".into();
        self.detail = Some(detail.into());
        self
    }
    pub fn refused(mut self, target: serde_json::Value, detail: &str) -> Self {
        self.target = target;
        self.dispatched = false;
        self.status = "refused".into();
        self.detail = Some(detail.into());
        self
    }
    pub fn fail(mut self, target: serde_json::Value, detail: &str) -> Self {
        self.target = target;
        self.dispatched = false;
        self.status = "failed".into();
        self.detail = Some(detail.into());
        self
    }
    pub fn print(&self) {
        let mut out = std::io::stdout();
        let _ = serde_json::to_writer_pretty(&mut out, self);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
    pub fn exit_code(&self) -> i32 {
        match self.status.as_str() {
            "ok" => 0,
            "refused" => 2,
            _ => 1,
        }
    }
}

// ---------------- state-id / cache ----------------

pub fn new_state_id() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{}-{:?}", d.as_nanos(), std::process::id()).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("s-{:x}", h)
}

pub fn save_last_state(state_id: &str, hwnd: isize, tree_sig: &str) {
    let p = ensure_data_dir().join("cache").join("last-state.json");
    let v = serde_json::json!({"state_id": state_id, "hwnd": hwnd, "sig": tree_sig, "ts": now_iso()});
    let _ = std::fs::write(p, serde_json::to_vec(&v).unwrap_or_default());
}

pub fn load_last_state() -> Option<serde_json::Value> {
    let p = ensure_data_dir().join("cache").join("last-state.json");
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn save_elements(hwnd: isize, elements: &serde_json::Value) -> std::path::PathBuf {
    let p = ensure_data_dir().join("cache").join(format!("elements-{}.json", hwnd));
    let _ = std::fs::write(&p, serde_json::to_vec(elements).unwrap_or_default());
    p
}

pub fn load_elements(hwnd: isize) -> Option<serde_json::Value> {
    let p = ensure_data_dir().join("cache").join(format!("elements-{}.json", hwnd));
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

// ---------------- safety (Qoder no-negotiables + confirmation levels) ----------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SafetyConfig {
    pub deny_processes: Vec<String>,
    pub deny_key_modifiers: Vec<String>,
    pub require_shot_id_for_coords: bool,
    pub levels: std::collections::BTreeMap<String, String>, // action -> handoff|confirm|preapprove|none
}

impl Default for SafetyConfig {
    fn default() -> Self {
        // Qoder-style no-negotiables (Windows) + ZCode coordinate guard
        SafetyConfig {
            deny_processes: [
                "cmd", "powershell", "pwsh", "windowsterminal", "conhost", "wt",
                "taskmgr", "regedit", "mmc", "control",
                "1password", "keepassxc", "keepass", "bitwarden", "dashlane", "lastpass",
                "ms-settings", "systemsettings",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            deny_key_modifiers: ["win", "meta", "logo"].iter().map(|s| s.to_string()).collect(),
            require_shot_id_for_coords: true,
            levels: [
                ("close".to_string(), "confirm".to_string()),
                ("drag".to_string(), "preapprove".to_string()),
                ("click".to_string(), "preapprove".to_string()),
                ("set-value".to_string(), "preapprove".to_string()),
                ("clipboard".to_string(), "preapprove".to_string()),
                ("key".to_string(), "preapprove".to_string()),
                ("type".to_string(), "none".to_string()),
            ]
            .into_iter()
            .collect(),
        }
    }
}

pub fn load_config() -> SafetyConfig {
    let p = ensure_data_dir().join("config.json");
    if let Ok(b) = std::fs::read(&p) {
        let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&b);
        if let Ok(c) = serde_json::from_slice(b) {
            return c;
        }
    }
    SafetyConfig::default()
}

/// Raw config JSON, for keys SafetyConfig does not model (e.g. `ubb_path`).
pub fn load_config_json() -> serde_json::Value {
    let p = ensure_data_dir().join("config.json");
    std::fs::read(p)
        .ok()
        .and_then(|b| {
            let b = b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b).to_vec();
            serde_json::from_slice(&b).ok()
        })
        .unwrap_or_else(|| serde_json::json!({}))
}

pub fn process_image_name(pid: u32) -> Option<String> {
    crate::win::process_name(pid).map(|n| n.to_ascii_lowercase())
}

/// Env vars that silently break third-party apps when inherited from an automation
/// shell: an Electron app that sees ELECTRON_RUN_AS_NODE runs as bare Node and exits 0
/// without ever creating a window; a stray crashpad pipe name hijacks Chromium helpers.
const TOXIC_EXACT: &[&str] = &[
    "ELECTRON_RUN_AS_NODE",
    "CHROME_CRASHPAD_PIPE_NAME",
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_REPL_EXTERNAL_MODULE",
    "NODE_REPL_HISTORY",
];
const TOXIC_PREFIX: &[&str] = &["ELECTRON_"];

fn toxic(key_upper: &str) -> bool {
    TOXIC_EXACT.iter().any(|k| *k == key_upper) || TOXIC_PREFIX.iter().any(|p| key_upper.starts_with(p))
}

/// Wide environment block (CreateProcessW format) with the toxic vars stripped.
pub fn sanitized_env_block() -> Vec<u16> {
    let mut block: Vec<u16> = Vec::new();
    for (k, v) in std::env::vars() {
        if toxic(&k.to_ascii_uppercase()) {
            continue;
        }
        for unit in format!("{k}={v}").encode_utf16() {
            block.push(unit);
        }
        block.push(0);
    }
    block.push(0);
    block
}

pub fn stripped_env_keys() -> Vec<String> {
    std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| toxic(&k.to_ascii_uppercase()))
        .collect()
}

/// Which deny-list entry, if any, matches this launch/open target?
/// Covers bare names, file names, `.exe` stems and URI schemes (`ms-settings:display`),
/// which is how Settings and other protected surfaces are actually opened.
pub fn denied_token(cfg: &SafetyConfig, raw: &str) -> Option<String> {
    let low = raw.to_ascii_lowercase();
    let mut tokens: Vec<String> = Vec::new();
    if let Some(pos) = low.find(':') {
        let scheme = &low[..pos];
        if !scheme.is_empty()
            && scheme.len() <= 32
            && !scheme.contains('/')
            && !scheme.contains('\\')
        {
            tokens.push(scheme.to_string());
        }
    }
    let base = std::path::Path::new(&low)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| low.clone());
    tokens.push(base.clone());
    tokens.push(base.trim_end_matches(".exe").to_string());
    if low.contains(".cpl") {
        tokens.push("control".to_string());
    }
    cfg.deny_processes
        .iter()
        .find(|d| tokens.iter().any(|t| t == *d))
        .cloned()
}

/// Qoder no-negotiables: refuse automation of terminals / password managers / security settings.
pub fn check_target_allowed(cfg: &SafetyConfig, pid: u32, hwnd: isize) -> Result<(), String> {
    if let Some(name) = process_image_name(pid) {
        let stem = name.trim_end_matches(".exe");
        for d in &cfg.deny_processes {
            if name == *d || stem == d.as_str() {
                return Err(format!(
                    "refused by safety policy: process '{}' is on the no-negotiable deny list (terminals/password managers/security settings cannot be automated)", name
                ));
            }
        }
    }
    let _ = hwnd;
    Ok(())
}

// ---------------- recording ----------------

pub fn record_append(name: &str, cmd: &str, args: &serde_json::Value, receipt: &Receipt, sig_after: Option<&str>) -> Result<()> {
    let dir = ensure_data_dir().join("recordings");
    let p = dir.join(format!("{}.jsonl", sanitize(name)));
    let rec = serde_json::json!({
        "ts": receipt.ts,
        "cmd": cmd,
        "args": args,
        "status": receipt.status,
        "state_sig_after": sig_after,
    });
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
    f.write_all(serde_json::to_vec(&rec)?.as_slice())?;
    f.write_all(b"\n")?;
    Ok(())
}

pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

pub fn state_fingerprint(hwnd: isize) -> Result<String> {
    let h = windows::Win32::Foundation::HWND(hwnd as *mut _);
    let sig = crate::uia::tree_signature(h)?;
    Ok(format!("{:x}", fnv1a(sig.as_bytes())))
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
