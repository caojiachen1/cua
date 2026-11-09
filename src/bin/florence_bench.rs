// Florence decoder backend benchmark: ONNX Runtime CPU vs candle CPU.
//
// Both runtimes are fed the same 21-token decoder input and the same encoder
// hidden state that produced `golden_fl_logits.bin`; the golden dump is the
// oracle. Build with:
//   cargo run --release --features candle --bin florence_bench
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use candle_core::{Device, Tensor};
use cua::perception::candle_florence::CandleFlorence;
use cua::perception::models;

fn read_ids(path: &std::path::Path) -> Result<Vec<i64>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(bytes
        .chunks_exact(8)
        .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect())
}

fn read_f32(path: &std::path::Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn max_abs(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0f32, f32::max)
}

fn argmax_mismatch(a: &[f32], b: &[f32], vocab: usize) -> usize {
    let mut n = 0;
    for row in 0..(a.len() / vocab) {
        let base = row * vocab;
        let ga = (0..vocab)
            .max_by(|x, y| a[base + x].partial_cmp(&a[base + y]).unwrap())
            .unwrap();
        let gb = (0..vocab)
            .max_by(|x, y| b[base + x].partial_cmp(&b[base + y]).unwrap())
            .unwrap();
        if ga != gb {
            n += 1;
        }
    }
    n
}

fn main() -> Result<()> {
    let root = models::root();
    let onnx = models::omni_dir().join("onnx");
    let weights = root.join("florence2-caption");
    let dec = onnx.join("fl_decoder.onnx");
    if !dec.is_file() || !weights.join("model.safetensors").is_file() {
        return Err(anyhow!(
            "models missing: {} / {}",
            dec.display(),
            weights.display()
        ));
    }
    let ids = read_ids(&onnx.join("golden_fl_ids.bin"))?;
    let hidden = read_f32(&onnx.join("golden_fl_encoder.bin"))?;
    let golden = read_f32(&onnx.join("golden_fl_logits.bin"))?;
    let seq = ids.len();
    let vocab = 51289usize;
    if golden.len() != seq * vocab {
        return Err(anyhow!("golden logits size {} != {}", golden.len(), seq * vocab));
    }

    // ---- ONNX Runtime CPU ----
    let mut sess = ort::session::Session::builder()?
        .commit_from_file(&dec)
        .map_err(|e| anyhow!("ort load: {e}"))?;
    let run_ort = |sess: &mut ort::session::Session| -> Result<Vec<f32>> {
        let id_t = ort::value::Tensor::from_array(([1usize, seq], ids.clone()))
            .map_err(|e| anyhow!("ids tensor: {e}"))?;
        let h_t = ort::value::Tensor::from_array(([1usize, 13, 768], hidden.clone()))
            .map_err(|e| anyhow!("hidden tensor: {e}"))?;
        let out = sess
            .run(ort::inputs![id_t, h_t])
            .map_err(|e| anyhow!("ort run: {e}"))?;
        let (_, data) = out[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow!("ort output: {e}"))?;
        Ok(data.to_vec())
    };
    let ort_logits = run_ort(&mut sess)?;
    let mut ort_ms = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let _ = run_ort(&mut sess)?;
        ort_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    // ---- candle CPU ----
    let dev = Device::Cpu;
    let model = CandleFlorence::load(&weights)?;
    let enc = Tensor::from_vec(hidden.clone(), (13, 768), &dev)?;
    let logits = model.forward(&ids.iter().map(|v| *v as u32).collect::<Vec<_>>(), &enc)?;
    let cand_logits = logits
        .flatten_all()?
        .to_vec1::<f32>()?;
    let mut candle_ms = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let out = model.forward(&ids.iter().map(|v| *v as u32).collect::<Vec<_>>(), &enc)?;
        let _ = out.flatten_all()?.to_vec1::<f32>()?;
        candle_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    // ---- candle incremental (KV cache) ----
    let mut sess_kv = model.empty_session(&enc)?;
    let mut kv_logits: Vec<f32> = Vec::new();
    let mut kv_ms = Vec::new();
    for (k, id) in ids.iter().enumerate() {
        let t = Instant::now();
        let out = model.step(*id as u32, &mut sess_kv)?;
        kv_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let row = out.flatten_all()?.to_vec1::<f32>()?;
        if k < 5 {
            kv_logits.extend_from_slice(&row);
        }
    }
    let kv_p50 = {
        let mut v = kv_ms.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let kv_max_abs = {
        let golden_k: Vec<f32> = golden[..5 * vocab].to_vec();
        max_abs(&kv_logits, &golden_k)
    };

    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let ort_p50 = median(&mut ort_ms);
    let candle_p50 = median(&mut candle_ms);
    let summary = serde_json::json!({
        "schema": "cua-florence-backend-bench-v1",
        "seq": seq,
        "vocab": vocab,
        "ort_cpu_p50_ms": ort_p50,
        "candle_cpu_p50_ms": candle_p50,
        "candle_kv_step_p50_ms": kv_p50,
        "speedup_candle_vs_ort": ort_p50 / candle_p50,
        "speedup_candle_kv_vs_ort": ort_p50 / kv_p50,
        "oracle": {
            "golden": "golden_fl_logits.bin (PyTorch/TorchScript)",
            "ort_max_abs": max_abs(&ort_logits, &golden),
            "candle_max_abs": max_abs(&cand_logits, &golden),
            "ort_candle_max_abs": max_abs(&ort_logits, &cand_logits),
            "ort_argmax_mismatch": argmax_mismatch(&ort_logits, &golden, vocab),
            "candle_argmax_mismatch": argmax_mismatch(&cand_logits, &golden, vocab),
            "candle_kv_max_abs_first5": kv_max_abs,
        },
    });
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
