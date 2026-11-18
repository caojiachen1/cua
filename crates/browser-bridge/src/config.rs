//! Where the bridge keeps its token, its port and the installed extension copy.
//!
//! Everything lives under cua's data directory (`%LOCALAPPDATA%\cua`, or
//! `$CUA_DATA_DIR` when a test or an isolated agent session overrides it), so a
//! single folder holds the config, the daemon log and the extension the browser
//! was pointed at.

use serde_json::{json, Value};
use std::path::PathBuf;

/// Port the daemon listens on when nothing overrides it.
pub const DEFAULT_PORT: u16 = 8765;

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CUA_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("cua")
}

/// `browser.json` — kept apart from cua's own `config.json` so neither writer
/// can clobber the other.
pub fn path() -> PathBuf {
    data_dir().join("browser.json")
}

/// The unpacked extension cua installs and the browser loads.
pub fn default_extension_dir() -> PathBuf {
    data_dir().join("browser-ext")
}

pub fn read() -> Value {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

pub fn write(map: &serde_json::Map<String, Value>) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, serde_json::to_string_pretty(map).unwrap_or_default());
}

pub fn port_of(v: &Value) -> u16 {
    v.get("port")
        .and_then(|x| x.as_u64())
        .filter(|n| *n > 0 && *n < 65536)
        .map(|n| n as u16)
        .unwrap_or(DEFAULT_PORT)
}

/// 64 hex chars from the OS CSPRNG. `None` means the platform RNG failed and
/// pairing must be refused rather than fall back to something guessable.
pub fn random_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).ok()?;
    let mut t = String::with_capacity(64);
    for b in bytes {
        t.push_str(&format!("{b:02x}"));
    }
    Some(t)
}

/// Token and port, generating and persisting a token on first use.
pub fn token_and_port() -> Result<(String, u16), String> {
    let mut cfg = read().as_object().cloned().unwrap_or_default();
    let token = match cfg.get("token").and_then(|v| v.as_str()) {
        Some(t) if t.len() >= 32 => t.to_string(),
        _ => {
            let t = random_token()
                .ok_or("OS CSPRNG unavailable — refusing to generate a predictable token")?;
            cfg.insert("token".into(), json!(t));
            write(&cfg);
            t
        }
    };
    let port = cfg
        .get("port")
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0 && *n < 65536)
        .map(|n| n as u16)
        .unwrap_or(DEFAULT_PORT);
    Ok((token, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_falls_back_and_rejects_zero() {
        assert_eq!(port_of(&json!({})), DEFAULT_PORT);
        assert_eq!(port_of(&json!({"port": 0})), DEFAULT_PORT);
        assert_eq!(port_of(&json!({"port": 9000})), 9000);
    }

    #[test]
    fn token_shape() {
        let a = random_token().expect("OS CSPRNG must work on the test host");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
