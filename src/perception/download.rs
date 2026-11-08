// perception/download.rs — install the runtime assets from the project's
// public ModelScope repos. No model bytes are embedded in the CLI.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use super::models;

const LICENSE_NOTICE: &str = "This installs derived runtime assets: OmniParser YOLOv9-E and Florence-2 weights (MIT in their subdirectories), PP-OCRv4 (Apache-2.0), EasyOCR CRAFT/english_g2 (Apache-2.0), and this project's converted ONNX/Burnpack files. The OmniParser repository's root code is CC-BY-4.0. The hosted repos carry weights only; the upstream projects hold the full license texts. Re-run with --accept-license if you agree.";

pub const LARGE_DOWNLOAD_BYTES: u64 = 1_500_000_000;

/// A lock older than this is considered abandoned (e.g. a crashed install).
const STALE_LOCK_AFTER: Duration = Duration::from_secs(10 * 60);

/// Exclusive install lock; the file is removed on drop.
#[derive(Debug)]
struct InstallLock {
    path: PathBuf,
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn try_create_lock(path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let started = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(file, "pid={}\nstarted_unix={started}", std::process::id());
    Ok(())
}

fn lock_is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
        .map(|age| age > STALE_LOCK_AFTER)
        .unwrap_or(false)
}

fn acquire_lock(root: &Path) -> Result<InstallLock> {
    let path = root.join(".install.lock");
    match try_create_lock(&path) {
        Ok(()) => Ok(InstallLock { path }),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if lock_is_stale(&path) {
                eprintln!("removing stale install lock {}", path.display());
                let _ = std::fs::remove_file(&path);
                if try_create_lock(&path).is_ok() {
                    return Ok(InstallLock { path });
                }
            }
            bail!(
                "another 'perception models install' is running (lock {}); remove it if that process is gone",
                path.display()
            )
        }
        Err(e) => Err(e).with_context(|| format!("create lock {}", path.display())),
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn download_one(url: &str, dest: &Path, expected_bytes: u64, expected_sha: &str) -> Result<u64> {
    use sha2::Digest;
    let parent = dest.parent().ok_or_else(|| anyhow!("asset has no parent"))?;
    std::fs::create_dir_all(parent)?;
    // Unique per-process temp name so even a stale lock cannot make two
    // installs overwrite each other's partial download.
    let tmp = dest.with_extension(format!("part.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    eprintln!("downloading {url}");
    let started = Instant::now();
    let req = ureq::get(url).timeout(Duration::from_secs(1800));
    let resp = req.call().map_err(|e| anyhow!("GET {url}: {e}"))?;
    let declared = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(size) = declared {
        if size != expected_bytes {
            bail!(
                "{url}: server size {size} != expected {expected_bytes}; refusing to install"
            );
        }
    }
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&tmp)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    let mut total = 0u64;
    let mut last_report = 0u64;
    loop {
        let n = reader.read(&mut buf).with_context(|| format!("read {url}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        total += n as u64;
        if total - last_report >= 64 * 1024 * 1024 {
            eprintln!("  {:>6.1} MiB", total as f64 / 1_048_576.0);
            last_report = total;
        }
    }
    file.sync_all()?;
    drop(file);
    if total != expected_bytes {
        let _ = std::fs::remove_file(&tmp);
        bail!("{url}: downloaded {total} bytes, expected {expected_bytes}");
    }
    let got = hex::encode(hasher.finalize());
    if got != expected_sha {
        let _ = std::fs::remove_file(&tmp);
        bail!("{url}: sha256 {got} != expected {expected_sha}; refusing to install");
    }
    std::fs::rename(&tmp, dest)?;
    eprintln!(
        "installed {} ({:.1} MiB, {:.1}s)",
        dest.display(),
        total as f64 / 1_048_576.0,
        started.elapsed().as_secs_f64()
    );
    Ok(total)
}

/// Normalize requested groups to the set that actually takes effect: empty or
/// `all` collapses to `["all"]`, known names are kept sorted and deduped, and
/// unknown names are dropped.
fn effective_groups(groups: &[String]) -> Vec<String> {
    let wanted: Vec<String> = groups
        .iter()
        .map(|g| g.trim().to_ascii_lowercase())
        .filter(|g| !g.is_empty())
        .collect();
    if wanted.is_empty() || wanted.iter().any(|g| g == models::GROUP_ALL) {
        return vec![models::GROUP_ALL.to_string()];
    }
    let mut out: Vec<String> = wanted
        .into_iter()
        .filter(|g| models::known_groups().contains(&g.as_str()))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Download/verify the runtime assets. The function is idempotent: files with
/// the expected size and SHA are skipped unless `force` is set. Only assets in
/// the requested groups are considered (`["all"]` or empty means everything).
pub fn install(accept_license: bool, force: bool, groups: &[String]) -> Result<Value> {
    if !accept_license {
        bail!("{LICENSE_NOTICE}");
    }
    let root = models::root();
    std::fs::create_dir_all(&root)?;
    let _lock = acquire_lock(&root)?;
    let effective = effective_groups(groups);
    // Unknown-only group lists select nothing (manifest_for(&[]) would mean all).
    let assets = if effective.is_empty() {
        Vec::new()
    } else {
        models::manifest_for(&effective)
    };
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut bytes = 0u64;
    for asset in assets {
        let url = models::repo_url(asset.repo, asset.repo_path);
        let rel = asset
            .path
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
            .unwrap_or_else(|_| asset.path.to_string_lossy().to_string());
        if !force && asset.path.is_file() {
            let size = std::fs::metadata(&asset.path).map(|m| m.len()).unwrap_or(0);
            if size == asset.bytes {
                match sha256_file(&asset.path) {
                    Ok(hash) if hash == asset.sha256 => {
                        skipped.push(json!({"path": rel, "bytes": size}));
                        continue;
                    }
                    _ => {}
                }
            }
        }
        match download_one(&url, &asset.path, asset.bytes, asset.sha256) {
            Ok(n) => {
                bytes += n;
                installed.push(json!({"path": rel, "bytes": n, "url": url}));
            }
            Err(e) => failed.push(json!({"path": rel, "error": format!("{e:#}")})),
        }
    }
    Ok(json!({
        "schema": "cua-perception-models-install-v1",
        "root": root.to_string_lossy(),
        "groups": effective,
        "repos": [models::REPO_YOLO, models::REPO_FLORENCE, models::REPO_PPOCR, models::REPO_EASYOCR],
        "ok": failed.is_empty(),
        "installed": installed,
        "skipped": skipped,
        "failed": failed,
        "bytes_downloaded": bytes,
        "large_download_bytes": LARGE_DOWNLOAD_BYTES,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_root(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "cua-perception-lock-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_lock_is_exclusive_and_reusable() {
        let root = unique_root("exclusive");
        let lock_path = root.join(".install.lock");

        let lock = acquire_lock(&root).expect("first acquire should succeed");
        assert!(lock_path.is_file());
        let body = std::fs::read_to_string(&lock_path).unwrap();
        assert!(body.contains(&format!("pid={}", std::process::id())), "{body}");

        let err = acquire_lock(&root).expect_err("second acquire must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("another 'perception models install' is running"),
            "{msg}"
        );
        assert!(msg.contains(".install.lock"), "{msg}");
        assert!(lock_path.is_file(), "failed acquire must not delete the lock");

        drop(lock);
        assert!(!lock_path.exists(), "drop must remove the lock file");

        let lock2 = acquire_lock(&root).expect("acquire after drop should succeed");
        drop(lock2);
        assert!(!lock_path.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn effective_groups_collapse_and_filter() {
        assert_eq!(effective_groups(&[]), vec!["all".to_string()]);
        assert_eq!(
            effective_groups(&["ALL".to_string()]),
            vec!["all".to_string()]
        );
        assert_eq!(
            effective_groups(&[
                "reference".to_string(),
                "core".to_string(),
                "core".to_string(),
            ]),
            vec!["core".to_string(), "reference".to_string()]
        );
        assert!(effective_groups(&["nope".to_string()]).is_empty());
    }
}
