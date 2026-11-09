// Detector backend benchmark: YOLOv9-E through Burn on WGPU or CUDA.
//
//   cargo run --release --bin det_bench                              # synthetic 1920x1080 UI, WGPU
//   cargo run --release --features=cuda --bin det_bench              # CUDA, ~1 s start, slow conv
//   cargo run --release --features=cuda,autotune --bin det_bench     # CUDA, fast kernels, slow start
//   cargo run --release --bin det_bench -- shot.png                  # real screenshot
use std::time::Instant;

use anyhow::Result;
use cua::perception::types::Detail;
use cua::perception::vision::{det_tile_budget, Detector};

fn synthetic(w: u32, h: u32) -> Vec<u8> {
    // A flat desktop with white cards, rows of 16px icon blocks and a few
    // text-like bars. Deterministic and non-uniform, so tiling cannot skip it.
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let base = if (x / 160 + y / 120) % 3 == 0 {
                245
            } else {
                232
            };
            buf[i] = base;
            buf[i + 1] = base;
            buf[i + 2] = base;
            buf[i + 3] = 255;
        }
    }
    let mut seed = 0x1234_5678u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as u32
    };
    for row in 0..(h / 40) {
        for col in 0..(w / 48) {
            let x0 = 16 + col * 48 + rnd() % 6;
            let y0 = 12 + row * 40 + rnd() % 6;
            let (iw, ih) = (12 + rnd() % 8, 12 + rnd() % 8);
            let c = [40 + rnd() % 180, 40 + rnd() % 180, 40 + rnd() % 180];
            for y in y0..(y0 + ih).min(h) {
                for x in x0..(x0 + iw).min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    buf[i] = c[0] as u8;
                    buf[i + 1] = c[1] as u8;
                    buf[i + 2] = c[2] as u8;
                }
            }
            // text bar next to the icon
            for y in (y0 + 4)..(y0 + 8).min(h) {
                for x in (x0 + 20)..(x0 + 60).min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    buf[i] = 90;
                    buf[i + 1] = 90;
                    buf[i + 2] = 90;
                }
            }
        }
    }
    buf
}

fn bench(det: &Detector, w: u32, h: u32, rgba: &[u8], budget: usize, runs: usize) -> Result<()> {
    let t = Instant::now();
    let n = det.detect_tiled(w, h, rgba, budget)?.len();
    println!(
        "budget={budget:<2} first={:>6.1} ms items={n}",
        t.elapsed().as_secs_f64() * 1e3
    );
    let mut best = f64::MAX;
    let mut sum = 0.0;
    for _ in 0..runs {
        let t = Instant::now();
        let n = det.detect_tiled(w, h, rgba, budget)?.len();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        best = best.min(ms);
        sum += ms;
        print!(" [{n} @ {ms:.1}]");
    }
    println!("  best={best:.1} ms avg={:.1} ms", sum / runs as f64);
    Ok(())
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1);
    let (w, h, rgba) = match path {
        Some(p) => {
            let img = image::open(&p)?.to_rgba8();
            let (w, h) = img.dimensions();
            (w, h, img.into_raw())
        }
        None => (1920, 1080, synthetic(1920, 1080)),
    };
    let det = Detector::load()?;
    println!("backend={} frame={w}x{h}", det.backend());
    bench(&det, w, h, &rgba, 0, 5)?;
    bench(&det, w, h, &rgba, det_tile_budget(Detail::Balanced), 3)?;
    bench(&det, w, h, &rgba, det_tile_budget(Detail::Full), 2)?;
    Ok(())
}
