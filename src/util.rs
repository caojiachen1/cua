// util.rs — receipts, cache/state-ids, safety policy (fusion of ZCode receipt schema + Qoder confirmation policy + Trae approval)
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECEIPT_SCHEMA: &str = "cua-action-receipt-v1";

pub fn data_dir() -> std::path::PathBuf {
    // Tests and isolated agent sessions set this so they never touch the
    // user's config, recordings, sandbox state or cache.
    if let Ok(d) = std::env::var("CUA_DATA_DIR") {
        if !d.trim().is_empty() {
            return std::path::PathBuf::from(d);
        }
    }
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
    /// not_requested | passed | failed | unknown
    #[serde(default = "verification_default")]
    pub verification: String,
    /// complete | incomplete (e.g. the action ran but its record could not be written)
    #[serde(default = "evidence_default")]
    pub evidence: String,
    /// auto | a11y | event | background | background-a11y (when the action has a mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_mode: Option<String>,
}

fn verification_default() -> String {
    "not_requested".into()
}

fn evidence_default() -> String {
    "complete".into()
}

pub struct ActionMeta {
    pub cmd: String,
    pub argv: Vec<String>,
    pub record: Option<String>,
}

impl ActionMeta {
    pub fn new(cmd: &str, argv: Vec<String>, record: &Option<String>) -> Self {
        ActionMeta { cmd: cmd.to_string(), argv, record: record.clone() }
    }
}

/// Shared tail for normal and sandbox actions: persist the recording and make a
/// failure visible as incomplete evidence instead of pretending it was recorded.
pub fn finish_action(meta: &ActionMeta, mut r: Receipt, sig_after: Option<&str>) -> Receipt {
    if let Some(name) = &meta.record {
        match record_append(name, &meta.cmd, &serde_json::json!(meta.argv), &r, sig_after) {
            Ok(()) => {
                if r.status == "ok" {
                    r.detail = Some(format!(
                        "{}; recorded to '{}'",
                        r.detail.clone().unwrap_or_default(),
                        name
                    ));
                }
            }
            Err(e) => {
                r.evidence = "incomplete".into();
                r.detail = Some(format!(
                    "{}; action ran but recording to '{}' failed: {e:#}",
                    r.detail.clone().unwrap_or_default(),
                    name
                ));
            }
        }
    }
    r
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
            verification: "not_requested".into(),
            evidence: "complete".into(),
            input_mode: None,
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

/// Observation cache namespace. A second agent/session sets CUA_SESSION so its
/// last-state and element indices cannot overwrite another session's. The
/// default keeps the historical filenames for compatibility.
pub fn session() -> String {
    std::env::var("CUA_SESSION")
        .ok()
        .map(|s| sanitize(&s))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

fn scoped(name: &str, ext: &str) -> String {
    let s = session();
    if s == "default" {
        format!("{name}.{ext}")
    } else {
        format!("{name}-{s}.{ext}")
    }
}

pub fn save_last_state(state_id: &str, hwnd: isize, tree_sig: &str) {
    let p = ensure_data_dir().join("cache").join(scoped("last-state", "json"));
    let v = serde_json::json!({"state_id": state_id, "hwnd": hwnd, "sig": tree_sig, "session": session(), "ts": now_iso()});
    let _ = std::fs::write(p, serde_json::to_vec(&v).unwrap_or_default());
}

pub fn load_last_state() -> Option<serde_json::Value> {
    let p = ensure_data_dir().join("cache").join(scoped("last-state", "json"));
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn save_elements(hwnd: isize, elements: &serde_json::Value) -> std::path::PathBuf {
    let p = ensure_data_dir()
        .join("cache")
        .join(scoped(&format!("elements-{hwnd}"), "json"));
    let _ = std::fs::write(&p, serde_json::to_vec(elements).unwrap_or_default());
    p
}

pub fn load_elements(hwnd: isize) -> Option<serde_json::Value> {
    let p = ensure_data_dir()
        .join("cache")
        .join(scoped(&format!("elements-{hwnd}"), "json"));
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Runtime-id overrides recorded by successful attribute relocations.
/// Observations are immutable snapshots, so a relocated element is recorded
/// here (old rid -> live rid) instead of rewriting the observation; index
/// actions keep their original indices and resolve through this map.
pub fn save_rid_override(hwnd: isize, old: &[i32], new: &[i32]) {
    let p = ensure_data_dir()
        .join("cache")
        .join(scoped(&format!("rid-fix-{hwnd}"), "json"));
    let mut map: std::collections::HashMap<String, Vec<i32>> = std::fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let key = serde_json::to_string(old).unwrap_or_default();
    if !key.is_empty() {
        map.insert(key, new.to_vec());
        let _ = std::fs::write(&p, serde_json::to_vec(&map).unwrap_or_default());
    }
}

pub fn load_rid_override(hwnd: isize, old: &[i32]) -> Option<Vec<i32>> {
    let p = ensure_data_dir()
        .join("cache")
        .join(scoped(&format!("rid-fix-{hwnd}"), "json"));
    let map: std::collections::HashMap<String, Vec<i32>> = std::fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())?;
    let key = serde_json::to_string(old).ok()?;
    map.get(&key).cloned()
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "obs".into());
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// An observation is immutable: nodes, signature, truncation flag and screenshot
/// metadata are written once under its id. The last-observation pointer selects
/// which immutable snapshot index actions resolve against.
pub fn save_observation(
    id: &str,
    hwnd: isize,
    sig: &str,
    truncated: bool,
    nodes: &serde_json::Value,
    screenshot: Option<&serde_json::Value>,
) -> Result<std::path::PathBuf> {
    let p = ensure_data_dir().join("cache").join(format!("obs-{id}.json"));
    let v = serde_json::json!({
        "schema": "cua-observation-v1",
        "id": id,
        "session": session(),
        "hwnd": hwnd,
        "sig": sig,
        "truncated": truncated,
        "created": now_iso(),
        "nodes": nodes,
        "screenshot": screenshot,
    });
    write_atomic(&p, &serde_json::to_vec(&v)?)?;
    prune_observations_in(&ensure_data_dir().join("cache"), OBS_RETAIN);
    Ok(p)
}

pub fn save_last_observation(hwnd: isize, id: &str) {
    let p = ensure_data_dir()
        .join("cache")
        .join(scoped(&format!("last-observation-{hwnd}"), "json"));
    let v = serde_json::json!({"id": id, "hwnd": hwnd, "session": session(), "ts": now_iso()});
    let _ = write_atomic(&p, &serde_json::to_vec(&v).unwrap_or_default());
}

pub fn last_observation_id(hwnd: isize) -> Option<String> {
    let p = ensure_data_dir()
        .join("cache")
        .join(scoped(&format!("last-observation-{hwnd}"), "json"));
    std::fs::read(p)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
}

pub fn load_observation(hwnd: isize) -> Option<serde_json::Value> {
    let id = last_observation_id(hwnd)?;
    let p = ensure_data_dir().join("cache").join(format!("obs-{id}.json"));
    std::fs::read(p)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
}

fn pinned_observation_id() -> Option<String> {
    std::env::var("CUA_OBSERVATION_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Per-invocation observation binding (`--observation-id`), set by main before
/// dispatch. Takes precedence over the CUA_OBSERVATION_ID environment pin;
/// unlike the pointer fallback it never silently degrades to the latest
/// observation — an id that does not resolve for the target window fails the
/// action instead.
static OBS_OVERRIDE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_observation_override(id: &str) {
    let id = id.trim();
    if !id.is_empty() {
        if let Ok(mut g) = OBS_OVERRIDE.lock() {
            *g = Some(id.to_string());
        }
    }
}

pub fn observation_override() -> Option<String> {
    OBS_OVERRIDE.lock().ok().and_then(|g| g.clone())
}

fn load_observation_by_id(id: &str, hwnd: isize) -> Option<serde_json::Value> {
    let p = ensure_data_dir().join("cache").join(format!("obs-{id}.json"));
    std::fs::read(p)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .filter(|v| v["hwnd"].as_i64() == Some(hwnd as i64))
}

/// Nodes for index actions: the explicit `--observation-id` override wins (no
/// fallback — a wrong id must fail the action, not silently observe anew), then
/// the `CUA_OBSERVATION_ID` pin, then the latest observation pointer, with the
/// legacy elements cache as the final fallback.
pub fn load_observation_nodes(hwnd: isize) -> Option<serde_json::Value> {
    if let Some(id) = observation_override() {
        return load_observation_by_id(&id, hwnd).and_then(|v| v.get("nodes").cloned());
    }
    if let Some(id) = pinned_observation_id() {
        return load_observation_by_id(&id, hwnd).and_then(|v| v.get("nodes").cloned());
    }
    load_observation(hwnd)
        .and_then(|v| v.get("nodes").cloned())
        .or_else(|| load_elements(hwnd))
}

const OBS_RETAIN: usize = 500;

fn prune_observations_in(dir: &std::path::Path, keep: usize) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let pinned = pinned_observation_id().or_else(observation_override);
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with("obs-") && name.ends_with(".json")
                && pinned.as_ref().map(|id| name != format!("obs-{id}.json")).unwrap_or(true)
        })
        .filter_map(|e| {
            let p = e.path();
            let t = e.metadata().ok()?.modified().ok()?;
            Some((t, p))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    for (_, p) in files.iter().take(files.len() - keep) {
        let _ = std::fs::remove_file(p);
    }
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
        "input_mode": receipt.input_mode,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_safe_names_and_replaces_the_rest() {
        assert_eq!(sanitize("rec-01_a"), "rec-01_a");
        assert_eq!(sanitize("a/b:c*?"), "a_b_c__");
        assert!(!sanitize("../../x").contains(".."));
    }

    #[test]
    fn receipt_exit_codes_are_stable() {
        let ok = Receipt::new("x").ok(serde_json::json!({}), "done");
        let refused = Receipt::new("x").refused(serde_json::json!({}), "no");
        let failed = Receipt::new("x").fail(serde_json::json!({}), "err");
        assert_eq!(ok.exit_code(), 0);
        assert_eq!(refused.exit_code(), 2);
        assert_eq!(failed.exit_code(), 1);
        assert!(!refused.dispatched);
        assert!(ok.dispatched);
        assert_eq!(ok.verification, "not_requested");
        assert_eq!(ok.evidence, "complete");
    }
}

#[cfg(test)]
mod obs_tests {
    use super::*;

    #[test]
    fn observation_pruning_keeps_the_newest_files() {
        let dir = std::env::temp_dir().join(format!(
            "cua-obs-prune-{}-{}",
            std::process::id(),
            now_iso().replace([':', '.', '-', 'T', 'Z'], "")
        ));
        let _ = std::fs::create_dir_all(&dir);
        for i in 0..5 {
            std::fs::write(dir.join(format!("obs-{i}.json")), b"{}").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        prune_observations_in(&dir, 2);
        let left = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).count();
        assert_eq!(left, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
