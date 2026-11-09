// perception/candle_florence.rs — Florence-2 BART decoder on candle (CPU).
//
// Optional (`--features candle`). Built to answer one question with evidence:
// is a pure-Rust tensor runtime faster than ONNX Runtime for the caption
// decoder? The forward pass mirrors the exported `fl_decoder.onnx` graph, and a
// unit test compares its logits against the golden tensor captured from the
// PyTorch/TorchScript original.
use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use candle_core::{DType, Device, Tensor, D};

pub const HIDDEN: usize = 768;
pub const HEADS: usize = 12;
pub const LAYERS: usize = 6;
const EPS: f64 = 1e-5;

struct Layer {
    sq: (Tensor, Tensor),
    sk: (Tensor, Tensor),
    sv: (Tensor, Tensor),
    so: (Tensor, Tensor),
    sn: (Tensor, Tensor),
    cq: (Tensor, Tensor),
    ck: (Tensor, Tensor),
    cv: (Tensor, Tensor),
    co: (Tensor, Tensor),
    cn: (Tensor, Tensor),
    f1: (Tensor, Tensor),
    f2: (Tensor, Tensor),
    fn_: (Tensor, Tensor),
}

/// Incremental decoder state: self-attention K/V for every token already fed,
/// plus the cross-attention K/V of the encoder (computed once).
pub struct Session {
    self_k: Vec<Option<Tensor>>,
    self_v: Vec<Option<Tensor>>,
    cross_k: Vec<Tensor>,
    cross_v: Vec<Tensor>,
    len: usize,
}

pub struct CandleFlorence {
    dev: Device,
    shared: Tensor,
    lm_head: Tensor,
    pos: Tensor,
    ln_emb: (Tensor, Tensor),
    layers: Vec<Layer>,
}

fn take(map: &HashMap<String, Tensor>, name: &str) -> Result<Tensor> {
    map.get(name)
        .ok_or_else(|| anyhow!("missing tensor {name}"))
        .and_then(|t| t.to_dtype(DType::F32).map_err(Into::into))
}

fn pair(map: &HashMap<String, Tensor>, w: &str, b: &str) -> Result<(Tensor, Tensor)> {
    Ok((take(map, w)?, take(map, b)?))
}

fn lin(x: &Tensor, (w, b): &(Tensor, Tensor)) -> Result<Tensor> {
    Ok(x.matmul(&w.t()?)?.broadcast_add(b)?)
}

fn layer_norm(x: &Tensor, (w, b): &(Tensor, Tensor)) -> Result<Tensor> {
    let mean = x.mean_keepdim(D::Minus1)?;
    let centered = x.broadcast_sub(&mean)?;
    let var = centered.sqr()?.mean_keepdim(D::Minus1)?;
    let denom = var.affine(1.0, EPS)?.sqrt()?;
    Ok(centered.broadcast_div(&denom)?.broadcast_mul(w)?.broadcast_add(b)?)
}

fn softmax_last(x: &Tensor) -> Result<Tensor> {
    let max = x.max_keepdim(D::Minus1)?;
    let e = x.broadcast_sub(&max)?.exp()?;
    let s = e.sum_keepdim(D::Minus1)?;
    Ok(e.broadcast_div(&s)?)
}

fn attend(q: &Tensor, k: &Tensor, v: &Tensor, mask: Option<&Tensor>) -> Result<Tensor> {
    let s = q.dim(0)?;
    let kv = k.dim(0)?;
    let d = q.dim(1)?;
    let hd = d / HEADS;
    let q = q.reshape((s, HEADS, hd))?.transpose(0, 1)?.contiguous()?;
    let k = k.reshape((kv, HEADS, hd))?.transpose(0, 1)?.contiguous()?;
    let v = v.reshape((kv, HEADS, hd))?.transpose(0, 1)?.contiguous()?;
    let mut scores = (q.matmul(&k.transpose(1, 2)?.contiguous()?)? / (hd as f64).sqrt())?;
    if let Some(m) = mask {
        scores = scores.broadcast_add(m)?;
    }
    let probs = softmax_last(&scores)?;
    let out = probs.matmul(&v)?;
    Ok(out.transpose(0, 1)?.contiguous()?.reshape((s, d))?)
}

fn causal_mask(seq: usize, dev: &Device) -> Result<Tensor> {
    let mut data = vec![0f32; seq * seq];
    for i in 0..seq {
        for j in 0..seq {
            if j > i {
                data[i * seq + j] = -1e9;
            }
        }
    }
    Ok(Tensor::from_vec(data, (1, seq, seq), dev)?)
}

impl CandleFlorence {
    /// Prefer the decoder-only safetensors (smaller, installed by
    /// `perception models install`); fall back to the full checkpoint.
    pub fn load_preferring_decoder(model_dir: &Path) -> Result<Self> {
        let decoder = model_dir.join("decoder.safetensors");
        if decoder.is_file() {
            return Self::load_file(&decoder);
        }
        Self::load(model_dir)
    }

    pub fn load(model_dir: &Path) -> Result<Self> {
        let path = model_dir.join("model.safetensors");
        Self::load_file(&path)
    }

    fn load_file(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Err(anyhow!(
                "Florence safetensors missing: {} (the candle backend reads the original weights)",
                path.display()
            ));
        }
        let dev = Device::Cpu;
        let map = candle_core::safetensors::load(path, &dev)
            .with_context(|| format!("load {}", path.display()))?;
        let base = "language_model.model.decoder";
        let mut layers = Vec::with_capacity(LAYERS);
        for i in 0..LAYERS {
            let p = |s: &str| format!("{base}.layers.{i}.{s}");
            layers.push(Layer {
                sq: pair(&map, &p("self_attn.q_proj.weight"), &p("self_attn.q_proj.bias"))?,
                sk: pair(&map, &p("self_attn.k_proj.weight"), &p("self_attn.k_proj.bias"))?,
                sv: pair(&map, &p("self_attn.v_proj.weight"), &p("self_attn.v_proj.bias"))?,
                so: pair(&map, &p("self_attn.out_proj.weight"), &p("self_attn.out_proj.bias"))?,
                sn: pair(&map, &p("self_attn_layer_norm.weight"), &p("self_attn_layer_norm.bias"))?,
                cq: pair(&map, &p("encoder_attn.q_proj.weight"), &p("encoder_attn.q_proj.bias"))?,
                ck: pair(&map, &p("encoder_attn.k_proj.weight"), &p("encoder_attn.k_proj.bias"))?,
                cv: pair(&map, &p("encoder_attn.v_proj.weight"), &p("encoder_attn.v_proj.bias"))?,
                co: pair(&map, &p("encoder_attn.out_proj.weight"), &p("encoder_attn.out_proj.bias"))?,
                cn: pair(&map, &p("encoder_attn_layer_norm.weight"), &p("encoder_attn_layer_norm.bias"))?,
                f1: pair(&map, &p("fc1.weight"), &p("fc1.bias"))?,
                f2: pair(&map, &p("fc2.weight"), &p("fc2.bias"))?,
                fn_: pair(&map, &p("final_layer_norm.weight"), &p("final_layer_norm.bias"))?,
            });
        }
        Ok(Self {
            dev,
            shared: take(&map, "language_model.model.shared.weight")?,
            // The exported graph ties the LM head to `shared`; the checkpoint's
            // separate `lm_head.weight` is a stale copy (max |diff| 1.1e-2,
            // which shifts logits by ~8.7).
            lm_head: take(&map, "language_model.model.shared.weight")?,
            pos: take(&map, &format!("{base}.embed_positions.weight"))?,
            ln_emb: pair(
                &map,
                &format!("{base}.layernorm_embedding.weight"),
                &format!("{base}.layernorm_embedding.bias"),
            )?,
            layers,
        })
    }

    /// Build the [13, 768] encoder tensor for one caption row.
    pub fn encode_hidden(&self, enc: &[f32]) -> Result<Tensor> {
        if enc.len() != 13 * HIDDEN {
            bail!("encoder hidden has {} floats, expected {}", enc.len(), 13 * HIDDEN);
        }
        Ok(Tensor::from_vec(enc.to_vec(), (13, HIDDEN), &self.dev)?)
    }

    pub fn empty_session(&self, enc: &Tensor) -> Result<Session> {
        let mut cross_k = Vec::with_capacity(self.layers.len());
        let mut cross_v = Vec::with_capacity(self.layers.len());
        for l in &self.layers {
            cross_k.push(lin(enc, &l.ck)?);
            cross_v.push(lin(enc, &l.cv)?);
        }
        Ok(Session {
            self_k: (0..self.layers.len()).map(|_| None).collect(),
            self_v: (0..self.layers.len()).map(|_| None).collect(),
            cross_k,
            cross_v,
            len: 0,
        })
    }

    /// One token at position `session.len`. Causal attention means this is
    /// numerically identical to re-running the full sequence and reading the
    /// last position.
    pub fn step(&self, id: u32, session: &mut Session) -> Result<Tensor> {
        let pos = session.len;
        let ids_t = Tensor::from_vec(vec![id as i64], 1, &self.dev)?;
        let mut x = self.shared.index_select(&ids_t, 0)?;
        x = x.broadcast_add(&self.pos.narrow(0, pos + 2, 1)?)?;
        let mut x = layer_norm(&x, &self.ln_emb)?;
        for (i, l) in self.layers.iter().enumerate() {
            let q = lin(&x, &l.sq)?;
            let k = lin(&x, &l.sk)?;
            let v = lin(&x, &l.sv)?;
            let ck = match &session.self_k[i] {
                Some(prev) => Tensor::cat(&[prev, &k], 0)?,
                None => k.clone(),
            };
            let cv = match &session.self_v[i] {
                Some(prev) => Tensor::cat(&[prev, &v], 0)?,
                None => v.clone(),
            };
            session.self_k[i] = Some(ck.clone());
            session.self_v[i] = Some(cv.clone());
            let attn = attend(&q, &ck, &cv, None)?;
            x = layer_norm(&(&x + &lin(&attn, &l.so)?)?, &l.sn)?;
            let attn = attend(&lin(&x, &l.cq)?, &session.cross_k[i], &session.cross_v[i], None)?;
            x = layer_norm(&(&x + &lin(&attn, &l.co)?)?, &l.cn)?;
            let h = lin(&x, &l.f1)?.gelu_erf()?;
            let h = lin(&h, &l.f2)?;
            x = layer_norm(&(&x + &h)?, &l.fn_)?;
        }
        session.len += 1;
        Ok(x.matmul(&self.lm_head.t()?)?)
    }

    /// Full-sequence decoder pass. `enc` is [13, 768]; returns logits [seq, V].
    pub fn forward(&self, ids: &[u32], enc: &Tensor) -> Result<Tensor> {
        let seq = ids.len();
        if seq == 0 {
            return Err(anyhow!("empty decoder input"));
        }
        let ids_t = Tensor::from_vec(
            ids.iter().map(|v| *v as i64).collect::<Vec<_>>(),
            seq,
            &self.dev,
        )?;
        let mut x = self.shared.index_select(&ids_t, 0)?;
        // BartLearnedPositionalEmbedding offsets every position by 2
        // (padding_idx + 1); the graph was exported with that offset.
        x = x.broadcast_add(&self.pos.narrow(0, 2, seq)?)?;
        let mut x = layer_norm(&x, &self.ln_emb)?;
        let mask = causal_mask(seq, &self.dev)?;
        for l in &self.layers {
            let attn = attend(
                &lin(&x, &l.sq)?,
                &lin(&x, &l.sk)?,
                &lin(&x, &l.sv)?,
                Some(&mask),
            )?;
            x = layer_norm(&(&x + &lin(&attn, &l.so)?)?, &l.sn)?;

            let cq = lin(&x, &l.cq)?;
            let ck = lin(enc, &l.ck)?;
            let cv = lin(enc, &l.cv)?;
            let attn = attend(&cq, &ck, &cv, None)?;
            x = layer_norm(&(&x + &lin(&attn, &l.co)?)?, &l.cn)?;

            let h = lin(&x, &l.f1)?.gelu_erf()?;
            let h = lin(&h, &l.f2)?;
            x = layer_norm(&(&x + &h)?, &l.fn_)?;
        }
        // The decoder-level final layer norm is absent from this checkpoint, so
        // the graph's last op is the tied lm_head.
        Ok(x.matmul(&self.lm_head.t()?)?)
    }
}

/// Read a little-endian f32 dump as a candle tensor.
pub fn tensor_from_f32(path: &Path, shape: (usize, usize), dev: &Device) -> Result<Tensor> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.len() % 4 != 0 {
        return Err(anyhow!("{} is not f32 data", path.display()));
    }
    let mut data = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        data.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    let want = shape.0 * shape.1;
    if data.len() < want {
        return Err(anyhow!(
            "{} has {} floats, expected at least {want}",
            path.display(),
            data.len()
        ));
    }
    data.truncate(want);
    Ok(Tensor::from_vec(data, shape, dev)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_logits_match_the_exported_graph() {
        let root = crate::perception::models::root();
        let onnx = crate::perception::models::omni_dir().join("onnx");
        let weights = root.join("florence2-caption");
        if !weights.join("model.safetensors").is_file()
            || !onnx.join("golden_fl_logits.bin").is_file()
            || !onnx.join("golden_fl_ids.bin").is_file()
            || !onnx.join("golden_fl_encoder.bin").is_file()
        {
            eprintln!("skip: florence weights/golden tensors not installed");
            return;
        }
        let dev = Device::Cpu;
        let model = CandleFlorence::load(&weights).expect("load weights");
        let id_bytes = std::fs::read(onnx.join("golden_fl_ids.bin")).expect("ids");
        let ids: Vec<u32> = id_bytes
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]) as u32)
            .collect();
        let enc = tensor_from_f32(&onnx.join("golden_fl_encoder.bin"), (13, 768), &dev)
            .expect("encoder hidden");
        let logits = model.forward(&ids, &enc).expect("candle forward");
        let got = logits.flatten_all().expect("flatten").to_vec1::<f32>().expect("floats");
        let golden = tensor_from_f32(&onnx.join("golden_fl_logits.bin"), (ids.len(), 51289), &dev)
            .expect("golden")
            .flatten_all()
            .expect("flatten")
            .to_vec1::<f32>()
            .expect("floats");
        assert_eq!(got.len(), golden.len());
        let mut max = 0f32;
        let mut argmax_mismatch = 0usize;
        let vocab = 51289;
        for (i, (a, b)) in got.iter().zip(golden.iter()).enumerate() {
            max = max.max((a - b).abs());
            if i % vocab == vocab - 1 {
                let base = i + 1 - vocab;
                let ga = (0..vocab).max_by(|x, y| got[base + x].partial_cmp(&got[base + y]).unwrap());
                let gb = (0..vocab).max_by(|x, y| golden[base + x].partial_cmp(&golden[base + y]).unwrap());
                if ga != gb {
                    argmax_mismatch += 1;
                }
            }
        }
        eprintln!("candle vs golden: max_abs={max:e} argmax_mismatch={argmax_mismatch}/{}", ids.len());
        assert!(max < 1e-3, "logits diverged: {max:e}");
        assert_eq!(argmax_mismatch, 0, "argmax differs on {argmax_mismatch} positions");
    }
}
