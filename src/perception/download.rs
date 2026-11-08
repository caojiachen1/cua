// perception/download.rs — install the runtime assets from the project's
// public ModelScope repo. No model bytes are embedded in the CLI.
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use super::models;

const LICENSE_NOTICE: &str = "This installs derived runtime assets: OmniParser YOLOv9-E and Florence-2 weights (MIT in their subdirectories), PP-OCRv4 (Apache-2.0), EasyOCR CRAFT/english_g2 (Apache-2.0), and this project's converted ONNX/Burnpack files. The OmniParser repository's root code is CC-BY-4.0. Review the notices in the ModelScope repo before enabling perception. Re-run with --accept-license if you agree.";

pub const LARGE_DOWNLOAD_BYTES: u64 = 1_500_000_000;

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
    let tmp = dest.with_extension("part");
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

/// Download/verify the runtime assets. The function is idempotent: files with
/// the expected size and SHA are skipped unless `force` is set.
pub fn install(accept_license: bool, force: bool) -> Result<Value> {
    if !accept_license {
        bail!("{LICENSE_NOTICE}");
    }
    let root = models::root();
    std::fs::create_dir_all(&root)?;
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut bytes = 0u64;
    for asset in models::manifest() {
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
        "repos": [models::REPO_YOLO, models::REPO_FLORENCE, models::REPO_PPOCR, models::REPO_EASYOCR],
        "ok": failed.is_empty(),
        "installed": installed,
        "skipped": skipped,
        "failed": failed,
        "bytes_downloaded": bytes,
        "large_download_bytes": LARGE_DOWNLOAD_BYTES,
    }))
}
