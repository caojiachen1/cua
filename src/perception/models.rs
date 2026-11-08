// perception/models.rs — model locations and the pinned asset manifest.
// Runtime inference is paddle-ocr-rs/ORT (default OCR), Burn/WGPU and tract;
// no Python, OpenCV or LibTorch is loaded by the CLI.
use std::path::PathBuf;

pub const OMNI_REVISION: &str = "f55d0750e5b94db2125ef0b45b0fa4a85ddc59b4";

/// Assets required for the default OCR/caption pipeline.
pub const GROUP_CORE: &str = "core";
/// Optional EasyOCR reference assets; not needed by the default pipeline.
pub const GROUP_REFERENCE: &str = "reference";
/// Pseudo-group selecting every asset.
pub const GROUP_ALL: &str = "all";

pub fn known_groups() -> &'static [&'static str] {
    &[GROUP_CORE, GROUP_REFERENCE, GROUP_ALL]
}

pub fn root() -> PathBuf {
    if let Ok(p) = std::env::var("CUA_MODEL_DIR") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    crate::util::data_dir().join("models")
}

pub fn omni_dir() -> PathBuf {
    root().join("omni-v2").join("f55d0750")
}

pub fn omni_onnx(name: &str) -> PathBuf {
    omni_dir().join("onnx").join(name)
}

pub fn ppocr_dir() -> PathBuf {
    root().join("ppocr-v4")
}

pub fn ppocr(name: &str) -> PathBuf {
    ppocr_dir().join(name)
}

pub fn florence_tokenizer() -> PathBuf {
    root().join("florence2-caption").join("tokenizer.json")
}

pub fn easyocr_dir() -> PathBuf {
    root().join("easyocr-en")
}

pub fn easyocr(name: &str) -> PathBuf {
    easyocr_dir().join(name)
}

pub fn yolo_bpk() -> PathBuf {
    root().join("models").join("yolov9e_640.bpk")
}

/// Pinned file facts. Sizes/hashes come from the official Hugging Face API and
/// the ModelScope RapidOCR v3.6.0 manifest.
pub struct Asset {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: &'static str,
    pub repo: &'static str,
    pub repo_path: &'static str,
    /// Install group this asset belongs to; see `GROUP_CORE`/`GROUP_REFERENCE`.
    pub group: &'static str,
}

pub const REPO_YOLO: &str = "cjc1887415157/yolov9e";
pub const REPO_FLORENCE: &str = "cjc1887415157/florence2-caption";
pub const REPO_PPOCR: &str = "cjc1887415157/ppocr-v4";
pub const REPO_EASYOCR: &str = "cjc1887415157/easyocr-en-reference";

/// Runtime assets only. Raw TorchScript/Florence checkpoints and the
/// single-image Florence graphs are development inputs and are not downloaded
/// by the CLI.
pub fn manifest() -> Vec<Asset> {
    vec![
        Asset {
            path: yolo_bpk(),
            bytes: 232_074_304,
            sha256: "a09935ab2768045bbbc38acf262c00e46b9b44615cd0fa497f903c69e5c4d710",
            repo: REPO_YOLO,
            repo_path: "yolov9e_640.bpk",
            group: GROUP_CORE,
        },
        Asset {
            path: omni_onnx("fl_encoder_b8.onnx"),
            bytes: 697_202_566,
            sha256: "e22f21669922744fa40ea2875c95e3cb76b3a49b4b1c2a4276a9dc0003693926",
            repo: REPO_FLORENCE,
            repo_path: "onnx/fl_encoder_b8.onnx",
            group: GROUP_CORE,
        },
        Asset {
            path: omni_onnx("fl_decoder_b8.onnx"),
            bytes: 545_456_547,
            sha256: "0ef8676bad7abe61088186f1057beb990af629cbfdb00ebbc782a119ab0c69dd",
            repo: REPO_FLORENCE,
            repo_path: "onnx/fl_decoder_b8.onnx",
            group: GROUP_CORE,
        },
        Asset {
            path: florence_tokenizer(),
            bytes: 1_355_863,
            sha256: "847bbeab6174d66a88898f729d52fa8d355fafe1bea101cf960dd404581df70e",
            repo: REPO_FLORENCE,
            repo_path: "tokenizer.json",
            group: GROUP_CORE,
        },
        Asset {
            path: ppocr("ch_PP-OCRv4_det_infer.onnx"),
            bytes: 4_745_517,
            sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
            repo: REPO_PPOCR,
            repo_path: "ch_PP-OCRv4_det_infer.onnx",
            group: GROUP_CORE,
        },
        Asset {
            path: ppocr("ch_ppocr_mobile_v2.0_cls_infer.onnx"),
            bytes: 585_532,
            sha256: "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
            repo: REPO_PPOCR,
            repo_path: "ch_ppocr_mobile_v2.0_cls_infer.onnx",
            group: GROUP_CORE,
        },
        Asset {
            path: ppocr("ch_PP-OCRv4_rec_infer.onnx"),
            bytes: 10_857_958,
            sha256: "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
            repo: REPO_PPOCR,
            repo_path: "ch_PP-OCRv4_rec_infer.onnx",
            group: GROUP_CORE,
        },
        Asset {
            path: ppocr("ppocr_keys_v1.txt"),
            bytes: 26_249,
            sha256: "28b2362ad4ab2dc38769aa72feb535e3a9ddb3fd2a7585a05920e6393b1dc7f7",
            repo: REPO_PPOCR,
            repo_path: "ppocr_keys_v1.txt",
            group: GROUP_CORE,
        },
        Asset {
            path: easyocr("craft_mlt_25k.onnx"),
            bytes: 83_059_843,
            sha256: "652b4a91ef0bb6919c5887cae79fa867079faa1459ff8acbdfc1e2e19b7e05b1",
            repo: REPO_EASYOCR,
            repo_path: "craft_mlt_25k.onnx",
            group: GROUP_REFERENCE,
        },
        Asset {
            path: easyocr("english_g2.onnx"),
            bytes: 15_144_535,
            sha256: "aae25c0ac7123ca56dab45b520be6d88478a7111cddc25af335ef03dbe4e5ab6",
            repo: REPO_EASYOCR,
            repo_path: "english_g2.onnx",
            group: GROUP_REFERENCE,
        },
        Asset {
            path: easyocr("characters.json"),
            bytes: 495,
            sha256: "7cf668de2547325422036e18d7d4fac8aabfa2c038dc7ae6e4601756a77ba6ed",
            repo: REPO_EASYOCR,
            repo_path: "characters.json",
            group: GROUP_REFERENCE,
        },
    ]
}

/// Select assets by install group. An empty list, or a list containing
/// `GROUP_ALL`, returns the full manifest; unknown group names are ignored.
pub fn manifest_for(groups: &[String]) -> Vec<Asset> {
    let all = manifest();
    let wanted: Vec<String> = groups
        .iter()
        .map(|g| g.trim().to_ascii_lowercase())
        .filter(|g| !g.is_empty())
        .collect();
    if wanted.is_empty() || wanted.iter().any(|g| g == GROUP_ALL) {
        return all;
    }
    all.into_iter()
        .filter(|a| wanted.iter().any(|g| g == a.group))
        .collect()
}

pub fn repo_url(repo: &str, path: &str) -> String {
    format!("https://modelscope.cn/models/{repo}/resolve/master/{path}")
}


pub fn verify_manifest(deep: bool) -> anyhow::Result<serde_json::Value> {
    use sha2::Digest;
    let mut entries = Vec::new();
    let mut ok = true;
    for a in manifest() {
        let exists = a.path.is_file();
        let size = std::fs::metadata(&a.path).map(|m| m.len()).unwrap_or(0);
        let size_ok = size == a.bytes;
        let mut hash_ok: Option<bool> = None;
        if deep && exists {
            let bytes = std::fs::read(&a.path)?;
            let got = hex::encode(sha2::Sha256::digest(&bytes));
            hash_ok = Some(got == a.sha256);
        }
        let entry_ok = exists && size_ok && hash_ok.unwrap_or(true);
        ok &= entry_ok;
        entries.push(serde_json::json!({
            "path": a.path.to_string_lossy(),
            "exists": exists,
            "bytes_expected": a.bytes,
            "bytes_actual": size,
            "size_ok": size_ok,
            "sha256_expected": a.sha256,
            "sha256_ok": hash_ok,
            "repo": a.repo,
            "repo_path": a.repo_path,
            "ok": entry_ok,
        }));
    }
    Ok(serde_json::json!({
        "schema": "cua-perception-models-v1",
        "revision": OMNI_REVISION,
        "root": root().to_string_lossy(),
        "deep": deep,
        "ok": ok,
        "assets": entries,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(name: &str) -> Vec<String> {
        vec![name.to_string()]
    }

    #[test]
    fn manifest_for_empty_returns_all() {
        assert_eq!(manifest_for(&[]).len(), 11);
        assert_eq!(manifest_for(&g(GROUP_ALL)).len(), 11);
    }

    #[test]
    fn manifest_for_core_excludes_easyocr() {
        let core = manifest_for(&g(GROUP_CORE));
        assert_eq!(core.len(), 8);
        assert!(core.iter().all(|a| a.group == GROUP_CORE));
        assert!(!core.iter().any(|a| a.repo == REPO_EASYOCR));
        assert!(!core.iter().any(|a| a.path.starts_with(easyocr_dir())));
    }

    #[test]
    fn manifest_for_reference_is_easyocr_only() {
        let reference = manifest_for(&g(GROUP_REFERENCE));
        assert_eq!(reference.len(), 3);
        assert!(reference.iter().all(|a| a.group == GROUP_REFERENCE));
        assert!(reference.iter().all(|a| a.path.starts_with(easyocr_dir())));
    }

    #[test]
    fn manifest_for_ignores_unknown_and_dedups() {
        assert!(manifest_for(&g("nope")).is_empty());
        let mixed = manifest_for(&[GROUP_REFERENCE.to_string(), "nope".to_string(), GROUP_REFERENCE.to_string()]);
        assert_eq!(mixed.len(), 3);
    }
}
