// perception/models.rs — model locations and the pinned asset manifest.
// Runtime inference is paddle-ocr-rs/ORT (default OCR), Burn/WGPU and tract;
// no Python, OpenCV or LibTorch is loaded by the CLI.
use std::path::PathBuf;

pub const OMNI_REVISION: &str = "f55d0750e5b94db2125ef0b45b0fa4a85ddc59b4";

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

pub fn yolo_bpk() -> PathBuf {
    root().join("models").join("yolov9e_640.bpk")
}

/// Pinned file facts. Sizes/hashes come from the official Hugging Face API and
/// the ModelScope RapidOCR v3.6.0 manifest.
pub struct Asset {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub fn manifest() -> Vec<Asset> {
    vec![
        Asset {
            path: omni_dir().join("icon_detect_v3-model.pt"),
            bytes: 281_182_680,
            sha256: "11c6cbb77f22569fab22d86c76407a83ec81ab89dbfe28279854822d6e3fb00c",
        },
        Asset {
            path: omni_dir().join("icon_caption-model.safetensors"),
            bytes: 1_083_916_964,
            sha256: "01b934b0fe2d07b181e2d07752f16ae27c9d0ea88ddffe13a9a003aa9680f233",
        },
        Asset {
            path: ppocr("ch_PP-OCRv4_det_infer.onnx"),
            bytes: 4_745_517,
            sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
        },
        Asset {
            path: ppocr("ch_ppocr_mobile_v2.0_cls_infer.onnx"),
            bytes: 585_532,
            sha256: "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
        },
        Asset {
            path: ppocr("ch_PP-OCRv4_rec_infer.onnx"),
            bytes: 10_857_958,
            sha256: "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
        },
        Asset {
            path: ppocr("ppocr_keys_v1.txt"),
            bytes: 26_249,
            sha256: "28b2362ad4ab2dc38769aa72feb535e3a9ddb3fd2a7585a05920e6393b1dc7f7",
        },
        Asset {
            path: ppocr("det640.onnx"),
            bytes: 4_745_381,
            sha256: "7e291ee86bf31c1628fd9e17b28e231fd1ebe54f4ef01c732a30ad5edc021827",
        },
        Asset {
            path: ppocr("rec320.onnx"),
            bytes: 10_857_866,
            sha256: "5630301f9f66828f01ea3641bf155bf0b8e6ad6b326f0535b722ed50da11fd76",
        },
        Asset {
            path: ppocr("cls192.onnx"),
            bytes: 585_522,
            sha256: "eba51c07102d74a05c6903f8750cb8bdc6dcc72324fef7e834feb848609a2f55",
        },
        Asset {
            path: yolo_bpk(),
            bytes: 232_074_304,
            sha256: "a09935ab2768045bbbc38acf262c00e46b9b44615cd0fa497f903c69e5c4d710",
        },
        Asset {
            path: omni_onnx("fl_encoder.onnx"),
            bytes: 697_201_749,
            sha256: "e30604d351ade239382253ce55946394c23f36ed52047b47734112c81c8fec15",
        },
        Asset {
            path: omni_onnx("fl_decoder.onnx"),
            bytes: 545_456_547,
            sha256: "3c7a3d9530e98c30876a34cc3580f9d1ad1bbd6b2f245ed13f1c272658f97a48",
        },
        Asset {
            path: florence_tokenizer(),
            bytes: 1_355_863,
            sha256: "847bbeab6174d66a88898f729d52fa8d355fafe1bea101cf960dd404581df70e",
        },
    ]
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
