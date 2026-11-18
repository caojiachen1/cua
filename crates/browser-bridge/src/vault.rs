//! The local credential vault and the forward-time `{{secret:NAME}}`
//! substitution — ported from the Node implementation so the merged daemon
//! keeps the whole credential story: values live in a plaintext file readable
//! only by the current user, agents reference them by NAME, and a plaintext
//! value reaches the extension exactly on the methods whose target field the
//! extension can classify before a keystroke (`fill` / `fill_form` via
//! placeholders, `secret.fill` by name, `replay` vars).
//!
//! Wire surface (extension ⇄ daemon reverse RPC, answered on the extension
//! socket only — a reply must never reach the agent routes):
//!   secret.resolve {name}            → {value}
//!   secret.list {}                   → {names, vault_path, vault_updated_at}
//!   secret.set {name, value}         → {stored, bytes}
//!   secret.delete {name}             → {deleted, existed}
//!   secret.bindings {names?}         → {mode, bindings:[{name, exists, origins}]}
//!
//! Storage: `<home>/.cua/secrets.json` (`CUA_SECRETS_FILE` overrides it, tests
//! must never touch the real vault). Two record shapes live side by side: a
//! bare string (no origin binding) and `{ "name": { "value", "origins" } }`.
//! `secret_origin_mode` ("off" default | "strict") in browser.json gates where
//! an unbound secret may be typed; the extension — the party that knows the
//! tab origin — compares the stamped bindings against the page host.

use serde_json::{json, Map, Value};
use std::path::PathBuf;

const MIN_RESTORE_VALUE_LEN: usize = 6;
pub const MARKER_FILL: &str = "__cua_secret_fill";
pub const MARKER_VARS: &str = "__cua_secret_vars";
pub const MARKER_BINDING: &str = "__cua_secret_binding";
pub const MARKER_NAMES: &str = "__cua_secret_names";
pub const MARKER_GATE: &str = "__cua_secret_gate";

type VaultError = (&'static str, String);

fn fail(code: &'static str, message: impl Into<String>) -> VaultError {
    (code, message.into())
}

/// `[A-Za-z0-9_.-]{1,64}` without pulling in a regex dependency.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

pub fn secrets_file() -> PathBuf {
    if let Ok(p) = std::env::var("CUA_SECRETS_FILE") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    crate::home_dir()
        .unwrap_or_default()
        .join(".cua")
        .join("secrets.json")
}

fn origin_mode() -> String {
    if crate::config::read().get("secret_origin_mode").and_then(|v| v.as_str()) == Some("strict") {
        "strict".to_string()
    } else {
        "off".to_string()
    }
}

fn bad_name(name: &str) -> VaultError {
    fail("BAD_SECRET_NAME", format!("\"{}\" is not a valid secret name (allowed: letters, digits, _ . - up to 64 chars)", name.chars().take(64).collect::<String>()))
}

fn read_store() -> Result<Map<String, Value>, VaultError> {
    let raw = match std::fs::read_to_string(secrets_file()) {
        Ok(s) => s,
        Err(_) => return Ok(Map::new()),
    };
    let v: Value = serde_json::from_str(&raw).map_err(|_| {
        fail("VAULT_CORRUPT", format!("cannot parse {} — fix or delete that file (no values are reported here)", secrets_file().display()))
    })?;
    match v {
        Value::Object(m) => Ok(m),
        _ => Err(fail("VAULT_CORRUPT", format!("{} must contain a JSON object of {{\"name\": \"value\"}}", secrets_file().display()))),
    }
}

fn write_store(store: &Map<String, Value>) -> Result<(), VaultError> {
    let path = secrets_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, serde_json::to_string_pretty(store).unwrap_or_default() + "\n")
        .map_err(|e| fail("VAULT_WRITE_FAILED", format!("cannot write {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// `{name: "value"}` and `{name:{value,origins}}` are one record; never throws
/// on shape. Returns `(value, origins)`.
fn read_record(store: &Map<String, Value>, name: &str) -> Option<(String, Vec<String>)> {
    match store.get(name) {
        Some(Value::String(s)) if !s.is_empty() => Some((s.clone(), Vec::new())),
        Some(Value::Object(o)) => {
            let value = o.get("value").and_then(|v| v.as_str())?;
            if value.is_empty() {
                return None;
            }
            let origins = o
                .get("origins")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some((value.to_string(), origins))
        }
        _ => None,
    }
}

/// upstream `normalizeOriginHost`: strip scheme/path/port, allow single-label
/// hosts (localhost dev pages); the suffix defense lives in the extension's
/// exact-or-subdomain match, not here.
fn normalize_origin_host(raw: &str) -> Result<String, VaultError> {
    let mut h = raw.trim().to_lowercase();
    if let Some(idx) = h.find("://") {
        let scheme = &h[..idx];
        if scheme.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        {
            h = h[idx + 3..].to_string();
        }
    }
    h = h.split('/').next().unwrap_or("").split(':').next().unwrap_or("").to_string();
    let trimmed = h.trim_matches('.').to_string();
    if trimmed.is_empty()
        || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || trimmed.contains("..")
    {
        return Err(fail("BAD_ORIGIN_HOST", format!("\"{}\" is not a bindable host (expected e.g. github.com, localhost or 127.0.0.1, no path/port)", raw.chars().take(120).collect::<String>())));
    }
    Ok(trimmed)
}

fn mtime_iso(path: &std::path::Path) -> Option<String> {
    let secs = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = secs.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(crate::iso_from_secs(secs))
}

/// Answers the extension's reverse RPC. The reply goes back on the extension
/// socket ONLY — a vault value must never reach the agent routes.
pub fn handle_reverse(method: &str, params: &Value) -> Result<Value, VaultError> {
    match method {
        "secret.resolve" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !valid_name(name) {
                return Err(bad_name(name));
            }
            let store = read_store()?;
            match read_record(&store, name) {
                Some((value, _)) => Ok(json!({ "value": value })),
                None => Err(fail("SECRET_UNKNOWN", format!("no secret named \"{name}\" — store one with the secret.set tool, then reference it from secret.fill or a {{{{secret:{name}}}}} placeholder"))),
            }
        }
        "secret.list" => {
            let store = read_store()?;
            let mut names: Vec<String> = store.keys().cloned().collect();
            names.sort();
            let path = secrets_file();
            Ok(json!({
                "names": names,
                "vault_path": path.display().to_string(),
                "vault_updated_at": mtime_iso(&path),
            }))
        }
        "secret.set" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !valid_name(name) {
                return Err(bad_name(name));
            }
            let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if value.is_empty() {
                return Err(fail("BAD_SECRET_VALUE", format!("secret \"{name}\" needs a non-empty string value (the value itself is never echoed back)")));
            }
            let mut store = read_store()?;
            // no origins param → keep an existing binding; an explicit array rebinds
            let origins: Vec<String> = match params.get("origins") {
                Some(Value::Array(a)) => {
                    let mut out = Vec::new();
                    for o in a {
                        let s = o.as_str().unwrap_or("");
                        if !s.is_empty() {
                            out.push(normalize_origin_host(s)?);
                        }
                    }
                    out.sort();
                    out.dedup();
                    out
                }
                _ => read_record(&store, name).map(|(_, o)| o).unwrap_or_default(),
            };
            let bytes = value.len();
            if origins.is_empty() {
                store.insert(name.to_string(), json!(value));
            } else {
                store.insert(name.to_string(), json!({ "value": value, "origins": origins }));
            }
            write_store(&store)?;
            Ok(json!({ "stored": name, "bytes": bytes }))
        }
        "secret.delete" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !valid_name(name) {
                return Err(bad_name(name));
            }
            let mut store = read_store()?;
            let existed = store.remove(name).is_some();
            if existed {
                write_store(&store)?;
            }
            Ok(json!({ "deleted": name, "existed": existed }))
        }
        "secret.bindings" => {
            let store = read_store()?;
            let names: Vec<String> = params
                .get("names")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().take(32).filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let bindings: Vec<Value> = names
                .iter()
                .map(|raw| {
                    let name = raw.chars().take(64).collect::<String>();
                    match read_record(&store, &name) {
                        Some((_, origins)) if valid_name(&name) => json!({ "name": name, "exists": true, "origins": origins }),
                        _ => json!({ "name": name, "exists": false, "origins": [] }),
                    }
                })
                .collect();
            Ok(json!({ "mode": origin_mode(), "bindings": bindings }))
        }
        _ => Err(fail(
            "RPC_METHOD_NOT_ALLOWED",
            format!("the daemon answers only diag.browsers and the secret.* vault methods on the extension channel (got \"{method}\")"),
        )),
    }
}

// ------------------------------------------------------------------ forward hook

/// Agent-supplied markers are stripped everywhere; only the daemon re-stamps
/// them, so a phishing page cannot self-declare "no binding".
fn strip_markers(p: &Value) -> Value {
    match p {
        Value::Object(o) => {
            let mut out = o.clone();
            for k in [MARKER_FILL, MARKER_VARS, MARKER_BINDING, MARKER_NAMES, MARKER_GATE] {
                out.remove(k);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn scan_out_of_scope(v: &Value, path: &str, allowed: &[String], out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            if s.contains("{{secret:") && !allowed.iter().any(|a| a == path) && out.len() < 8 {
                out.push(if path.is_empty() { "(root)".to_string() } else { path.to_string() });
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                scan_out_of_scope(x, &format!("{path}.{i}"), allowed, out);
            }
        }
        Value::Object(o) => {
            for (k, x) in o {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                scan_out_of_scope(x, &p, allowed, out);
            }
        }
        _ => {}
    }
}

struct Subst<'a> {
    store: &'a Map<String, Value>,
    allowed: &'a [String],
    missing: Vec<String>,
    invalid: Vec<String>,
    replaced: u32,
    used: Vec<(String, Vec<String>)>,
}

impl<'a> Subst<'a> {
    fn substitute_string(&mut self, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("{{secret:") {
            out.push_str(&rest[..start]);
            let tail = &rest[start + 9..];
            let Some(end) = tail.find("}}") else {
                out.push_str(&rest[start..]);
                rest = "";
                break;
            };
            let raw = &tail[..end];
            let consumed = &rest[start..start + 9 + end + 2];
            if !valid_name(raw) {
                self.invalid.push(raw.chars().take(64).collect());
                out.push_str(consumed);
            } else {
                match read_record(self.store, raw) {
                    Some((value, origins)) => {
                        self.replaced += 1;
                        self.used.push((raw.to_string(), origins));
                        out.push_str(&value);
                    }
                    None => {
                        self.missing.push(raw.to_string());
                        out.push_str(consumed);
                    }
                }
            }
            rest = &rest[start + 9 + end + 2..];
        }
        out.push_str(rest);
        out
    }

    fn walk(&mut self, v: &Value, path: &str) -> Value {
        match v {
            Value::String(text) => {
                if !self.allowed.iter().any(|a| a == path) || !text.contains("{{secret:") {
                    return Value::String(text.clone());
                }
                Value::String(self.substitute_string(text))
            }
            Value::Array(a) => Value::Array(
                a.iter()
                    .enumerate()
                    .map(|(i, x)| {
                        let p = format!("{path}.{i}");
                        self.walk(x, &p)
                    })
                    .collect(),
            ),
            Value::Object(o) => {
                let mut out = Map::new();
                for (k, x) in o {
                    let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    out.insert(k.clone(), self.walk(x, &p));
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }
}

fn gate_value(names: &[(String, Vec<String>)]) -> Value {
    json!({
        "mode": origin_mode(),
        "bindings": names
            .iter()
            .map(|(n, o)| json!({ "name": n, "exists": true, "origins": o }))
            .collect::<Vec<_>>(),
    })
}

fn denied_names(used: &[(String, Vec<String>)]) -> Vec<String> {
    if origin_mode() != "strict" {
        return Vec::new();
    }
    used.iter().filter(|(_, o)| o.is_empty()).map(|(n, _)| n.clone()).collect()
}

/// The forward-time hook the agent path runs before a tool call reaches the
/// extension. Mirrors the Node `prepareForward` decision table; returns the
/// full decision object (`tool`, `params`, `missing`, `invalid`, `replaced`,
/// `denied`, `out_of_scope`, ...) so the caller can refuse before forwarding.
pub fn prepare_forward(method: &str, params: &Value) -> Result<Value, VaultError> {
    let tool = method.strip_prefix("tool.").unwrap_or(method);
    let base = strip_markers(params);
    let mut result = json!({ "tool": tool, "missing": [], "invalid": [], "replaced": 0, "denied": [], "out_of_scope": [], "params": base });

    let merge = |result: &mut Value, fwd: Value| {
        if let (Some(dst), Some(src)) = (result.as_object_mut(), fwd.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
    };

    match tool {
        "fill" | "fill_form" => {
            let input = base.clone();
            let mut allowed: Vec<String> = Vec::new();
            // only the schema-declared value leaves may carry a secret: a
            // placeholder in ref/selector would otherwise be replaced too, and
            // a failure message could echo the plaintext back to the caller
            if tool == "fill_form" {
                if let Some(Value::Object(values)) = input.get("values") {
                    for (k, v) in values {
                        if let Value::String(s) = v {
                            if s.contains("{{secret:") {
                                allowed.push(format!("values.{k}"));
                            }
                        }
                    }
                }
            } else if let Some(Value::String(s)) = input.get("value") {
                if s.contains("{{secret:") {
                    allowed.push("value".to_string());
                }
            }
            let mut out_scope = Vec::new();
            scan_out_of_scope(&input, "", &allowed, &mut out_scope);
            if !allowed.is_empty() {
                let store = read_store()?;
                let mut s = Subst {
                    store: &store,
                    allowed: &allowed,
                    missing: Vec::new(),
                    invalid: Vec::new(),
                    replaced: 0,
                    used: Vec::new(),
                };
                let walked = s.walk(&input, "");
                let out = strip_markers(&walked);
                s.used.sort();
                s.used.dedup();
                let denied = denied_names(&s.used);
                if s.replaced > 0 && denied.is_empty() {
                    if let Some(o) = out.as_object() {
                        let mut with_markers = o.clone();
                        with_markers.insert(MARKER_FILL.to_string(), json!(true));
                        with_markers.insert(MARKER_NAMES.to_string(), json!(s.used.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()));
                        with_markers.insert(MARKER_GATE.to_string(), gate_value(&s.used));
                        merge(
                            &mut result,
                            json!({
                                "params": Value::Object(with_markers),
                                "missing": s.missing, "invalid": s.invalid,
                                "replaced": s.replaced, "denied": denied,
                                "out_of_scope": out_scope,
                            }),
                        );
                        return Ok(result);
                    }
                }
                merge(
                    &mut result,
                    json!({
                        "params": out,
                        "missing": s.missing, "invalid": s.invalid,
                        "replaced": s.replaced, "denied": denied,
                        "out_of_scope": out_scope,
                    }),
                );
            } else {
                result["out_of_scope"] = json!(out_scope);
            }
            Ok(result)
        }
        "replay" | "replay.run" => {
            // substitute ONLY inside params.vars values — trace/events/name stay
            // untouched, so no plaintext can ride a structural field back into a
            // saved script
            let mut p = base.clone();
            let store = read_store()?;
            let mut resolved = Map::new();
            let mut used: Vec<(String, Vec<String>)> = Vec::new();
            let mut missing: Vec<String> = Vec::new();
            let mut invalid: Vec<String> = Vec::new();
            let mut replaced = 0u32;
            if let Some(Value::Object(vars)) = p.get("vars").cloned() {
                let mut out_vars = Map::new();
                for (k, v) in vars {
                    if let Value::String(text) = &v {
                        let mut s = Subst {
                            store: &store,
                            allowed: &["__all__".to_string()],
                            missing: Vec::new(),
                            invalid: Vec::new(),
                            replaced: 0,
                            used: Vec::new(),
                        };
                        let nv = s.walk(&Value::String(text.clone()), "__all__");
                        replaced += s.replaced;
                        missing.extend(s.missing);
                        invalid.extend(s.invalid);
                        if !s.used.is_empty() {
                            let mut names: Vec<String> = s.used.iter().map(|(n, _)| n.clone()).collect();
                            names.sort();
                            names.dedup();
                            resolved.insert(k.clone(), json!(names));
                            used.extend(s.used);
                        }
                        out_vars.insert(k, nv);
                    } else {
                        out_vars.insert(k, v);
                    }
                }
                if let Some(o) = p.as_object_mut() {
                    o.insert("vars".to_string(), Value::Object(out_vars));
                }
            }
            used.sort();
            used.dedup();
            let denied = denied_names(&used);
            if replaced > 0 && denied.is_empty() {
                if let Some(o) = p.as_object_mut() {
                    o.insert(MARKER_FILL.to_string(), json!(true));
                    o.insert(MARKER_VARS.to_string(), Value::Object(resolved));
                    o.insert(MARKER_NAMES.to_string(), json!(used.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()));
                    o.insert(MARKER_GATE.to_string(), gate_value(&used));
                }
            }
            merge(
                &mut result,
                json!({ "params": p, "missing": missing, "invalid": invalid, "replaced": replaced, "denied": denied }),
            );
            Ok(result)
        }
        "replay.save" => {
            // a vault plaintext must never persist into chrome.storage.session,
            // so every occurrence of a stored value is restored to its
            // placeholder (longest first; too-short values would mangle text)
            let store = read_store()?;
            let mut pairs: Vec<(String, String)> = store
                .iter()
                .filter_map(|(name, _)| read_record(&store, name).map(|(value, _)| (name.clone(), value)))
                .filter(|(_, v)| v.len() >= MIN_RESTORE_VALUE_LEN)
                .collect();
            pairs.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
            let mut hits = 0u32;
            let mut names: Vec<String> = Vec::new();
            let restored = restore_walk(&base, &pairs, &mut hits, &mut names);
            merge(&mut result, json!({ "params": restored, "replaced": hits }));
            Ok(result)
        }
        "secret.fill" => {
            // authoritative binding stamp; priority params.enforce > config > off
            let mut p = base.clone();
            let store = read_store()?;
            let name = p.get("secret").and_then(|v| v.as_str()).unwrap_or("");
            let origins = if valid_name(name) {
                read_record(&store, name).map(|(_, o)| o).unwrap_or_default()
            } else {
                Vec::new()
            };
            let enforce = p.get("enforce").and_then(|v| v.as_str()).unwrap_or("");
            let mode = if enforce == "strict" || enforce == "off" {
                enforce.to_string()
            } else {
                origin_mode()
            };
            if let Some(o) = p.as_object_mut() {
                o.insert(MARKER_BINDING.to_string(), json!({ "mode": mode, "origins": origins }));
            }
            merge(&mut result, json!({ "params": p }));
            Ok(result)
        }
        _ => {
            // batch steps dispatch INSIDE the extension — strip any forged
            // marker there too, or it would release the password guard
            let mut p = base.clone();
            if let Some(Value::Array(actions)) = p.get("actions").cloned() {
                let cleaned: Vec<Value> = actions.iter().map(strip_markers).collect();
                if let Some(o) = p.as_object_mut() {
                    o.insert("actions".to_string(), Value::Array(cleaned));
                }
            }
            merge(&mut result, json!({ "params": p }));
            Ok(result)
        }
    }
}

fn restore_walk(v: &Value, pairs: &[(String, String)], hits: &mut u32, names: &mut Vec<String>) -> Value {
    match v {
        Value::String(s) => {
            let mut out = s.clone();
            for (name, value) in pairs {
                if out.contains(value.as_str()) {
                    *hits += out.matches(value.as_str()).count() as u32;
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                    out = out.replace(value.as_str(), &format!("{{{{secret:{name}}}}}"));
                }
            }
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| restore_walk(x, pairs, hits, names)).collect()),
        Value::Object(o) => {
            let mut out = Map::new();
            for (k, x) in o {
                out.insert(k.clone(), restore_walk(x, pairs, hits, names));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    // the vault path is process env, so the tests that touch it must not run
    // concurrently with each other
    static VAULT_LOCK: Mutex<()> = Mutex::new(());

    fn temp_vault(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "cua-vault-test-{tag}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        std::env::set_var("CUA_SECRETS_FILE", &p);
        p
    }

    #[test]
    fn name_validation_matches_upstream() {
        assert!(valid_name("github_pw"));
        assert!(valid_name("a.b-c_d"));
        assert!(!valid_name(""));
        assert!(!valid_name("has space"));
        assert!(!valid_name("unicode✓"));
        assert!(!valid_name(&"x".repeat(65)));
    }

    #[test]
    fn set_list_resolve_delete_roundtrip() {
        let _g = VAULT_LOCK.lock().unwrap();
        let path = temp_vault("roundtrip");
        let r = handle_reverse("secret.set", &json!({"name": "gh_pw", "value": "s3cret"})).unwrap();
        assert_eq!(r["stored"], "gh_pw");
        assert_eq!(r["bytes"], 6);
        let list = handle_reverse("secret.list", &json!({})).unwrap();
        assert_eq!(list["names"], json!(["gh_pw"]));
        assert_eq!(list["vault_path"], json!(path.display().to_string()));
        let res = handle_reverse("secret.resolve", &json!({"name": "gh_pw"})).unwrap();
        assert_eq!(res["value"], "s3cret");
        let del = handle_reverse("secret.delete", &json!({"name": "gh_pw"})).unwrap();
        assert_eq!(del["existed"], true);
        let err = handle_reverse("secret.resolve", &json!({"name": "gh_pw"})).unwrap_err();
        assert_eq!(err.0, "SECRET_UNKNOWN");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bare_and_bound_records_coexist() {
        let _g = VAULT_LOCK.lock().unwrap();
        temp_vault("records");
        handle_reverse("secret.set", &json!({"name": "plain", "value": "v1"})).unwrap();
        handle_reverse("secret.set", &json!({"name": "bound", "value": "v2", "origins": ["https://GitHub.com/"] })).unwrap();
        let store = read_store().unwrap();
        assert_eq!(store["plain"], json!("v1"));
        assert_eq!(store["bound"]["origins"], json!(["github.com"]));
        // re-set without origins keeps the binding
        handle_reverse("secret.set", &json!({"name": "bound", "value": "v3"})).unwrap();
        let store = read_store().unwrap();
        assert_eq!(store["bound"]["origins"], json!(["github.com"]));
        let b = handle_reverse("secret.bindings", &json!({"names": ["plain", "bound", "ghost"] })).unwrap();
        assert_eq!(b["mode"], "off");
        assert_eq!(b["bindings"][0]["exists"], true);
        assert_eq!(b["bindings"][0]["origins"], json!([]));
        assert_eq!(b["bindings"][1]["origins"], json!(["github.com"]));
        assert_eq!(b["bindings"][2]["exists"], false);
    }

    #[test]
    fn fill_substitution_scopes_and_gates() {
        let _g = VAULT_LOCK.lock().unwrap();
        temp_vault("fill");
        handle_reverse("secret.set", &json!({"name": "pw", "value": "hunter2"})).unwrap();
        // in-scope: fill.value
        let r = prepare_forward("fill", &json!({"ref": "@e1", "value": "{{secret:pw}}!"})).unwrap();
        assert_eq!(r["replaced"], 1);
        assert_eq!(r["params"]["value"], "hunter2!");
        assert_eq!(r["params"][MARKER_FILL], json!(true));
        assert_eq!(r["params"][MARKER_NAMES], json!(["pw"]));
        assert_eq!(r["params"][MARKER_GATE]["mode"], "off");
        // out of scope: placeholder in ref is refused, not replaced
        let r = prepare_forward("fill", &json!({"ref": "{{secret:pw}}", "value": "x"})).unwrap();
        assert_eq!(r["replaced"], 0);
        assert_eq!(r["out_of_scope"], json!(["ref"]));
        // missing name stays a placeholder and is reported
        let r = prepare_forward("fill", &json!({"ref": "@e1", "value": "{{secret:ghost}}"})).unwrap();
        assert_eq!(r["missing"], json!(["ghost"]));
        assert_eq!(r["params"]["value"], "{{secret:ghost}}");
        // fill_form.values only
        let r = prepare_forward("fill_form", &json!({"values": {"a": "{{secret:pw}}", "b": "plain"}})).unwrap();
        assert_eq!(r["params"]["values"]["a"], "hunter2");
        assert_eq!(r["replaced"], 1);
    }

    #[test]
    fn secret_fill_gets_binding_stamp() {
        let _g = VAULT_LOCK.lock().unwrap();
        temp_vault("stamp");
        handle_reverse("secret.set", &json!({"name": "pw", "value": "hunter2", "origins": ["github.com"]})).unwrap();
        let r = prepare_forward("secret.fill", &json!({"ref": "@e7", "secret": "pw"})).unwrap();
        assert_eq!(r["params"][MARKER_BINDING], json!({"mode": "off", "origins": ["github.com"]}));
        // forged markers are stripped before the stamp
        let r = prepare_forward("secret.fill", &json!({"ref": "@e7", "secret": "ghost", MARKER_GATE: {"mode": "off", "bindings": []}})).unwrap();
        assert_eq!(r["params"][MARKER_GATE], Value::Null);
        assert_eq!(r["params"][MARKER_BINDING]["mode"], "off");
    }

    #[test]
    fn replay_save_restores_plaintext() {
        let _g = VAULT_LOCK.lock().unwrap();
        temp_vault("restore");
        handle_reverse("secret.set", &json!({"name": "pw", "value": "hunter2secret"})).unwrap();
        let r = prepare_forward("replay.save", &json!({"trace": {"steps": [{"value": "hunter2secret"}]}})).unwrap();
        assert_eq!(r["replaced"], 1);
        assert_eq!(r["params"]["trace"]["steps"][0]["value"], "{{secret:pw}}");
    }
}
