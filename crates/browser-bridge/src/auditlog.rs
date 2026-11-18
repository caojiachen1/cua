//! Persistent audit trail: periodically drains every connected slot's in-memory
//! audit ring into `%LOCALAPPDATA%\cua\audit\<date>.jsonl` (one JSON object per
//! line, `{ts, slot, seq, ...entry}`), so the history survives browser and
//! service-worker restarts. The ring itself is 500 entries and dies with the
//! extension; this file is the durable record the old Node `audit tail
//! --follow` used to keep.
//!
//! Cursor per slot: `audit.tail {since_seq}` is incremental, so a reconnect
//! catches up instead of re-reading. A service-worker restart resets the ring's
//! seq — the reply reports `last_seq < since_seq`, and the cursor restarts from
//! there (the tool's note field names this case explicitly).

use crate::config;
use crate::AppState;
use serde_json::json;
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

const POLL_INTERVAL_MS: u64 = 30_000;
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

fn file_for(day: &str) -> std::path::PathBuf {
    let dir = config::data_dir().join("audit");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{day}.jsonl"))
}

/// Spawned once by `run_daemon`; stops when the daemon stops (process exit).
/// `audit_persist: false` in browser.json pauses it.
pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || {
        let mut cursors: HashMap<String, u64> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            if config::read().get("audit_persist").and_then(|v| v.as_bool()) == Some(false) {
                continue;
            }
            let day = &crate::iso_now()[..10];
            let mut file = None;
            for slot in state.browser_names() {
                let since = cursors.get(&slot).copied().unwrap_or(0);
                let result = match crate::call_extension(
                    &state,
                    Some(&slot),
                    "audit.tail",
                    json!({ "since_seq": since, "limit": 500 }),
                    CALL_TIMEOUT,
                ) {
                    Ok(v) => v,
                    Err(_) => continue, // slot gone or busy — try again next tick
                };
                let entries = result
                    .get("entries")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if entries.is_empty() {
                    continue;
                }
                let mut last = result.get("last_seq").and_then(|v| v.as_u64()).unwrap_or(since);
                if last < since {
                    // the ring's seq restarted (service worker reload): resume
                    // from the new, lower seq instead of never draining again
                    last = result
                        .get("entries")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.last())
                        .and_then(|e| e.get("seq"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                }
                if file.is_none() {
                    file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(file_for(day))
                        .ok();
                }
                if let Some(f) = file.as_mut() {
                    for e in &entries {
                        let mut line = json!({ "ts": crate::iso_now(), "slot": slot.clone() });
                        if let (Some(dst), Some(src)) = (line.as_object_mut(), e.as_object()) {
                            for (k, v) in src {
                                dst.insert(k.clone(), v.clone());
                            }
                        }
                        let _ = writeln!(f, "{line}");
                    }
                }
                cursors.insert(slot, last);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn day_slice_is_yyyy_mm_dd() {
        let t = crate::iso_now();
        let day = &t[..10];
        assert_eq!(day.len(), 10, "{t}");
        assert_eq!(day.as_bytes()[4], b'-');
        assert_eq!(day.as_bytes()[7], b'-');
    }
}
