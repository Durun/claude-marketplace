//! デコードの実装ごとの書き出しの速さを測る。`auto` を既定にするかの判断に使う。
//!
//! `cargo run -p mm-export --release --example export_bench --features mm-runtime/ffmpeg,mm-runtime/d3d12va,mm-runtime/cuvid -- <プロジェクト> <decode> [<幅>x<高さ>] [回数] [GPU の名前の一部]`

use std::path::PathBuf;

use mm_export::{export_project, ExportOptions};
use mm_runtime::{assemble, Backend, RuntimeConfig};
use mm_types::{MmError, Result};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let project = PathBuf::from(
        args.next()
            .ok_or_else(|| MmError::InvalidArgument("プロジェクトのパスが要る".into()))?,
    );
    let decode: Backend = args.next().unwrap_or_else(|| "auto".into()).parse()?;
    let size = args
        .next()
        .map(|s| {
            let (w, h) = s
                .split_once('x')
                .ok_or_else(|| MmError::InvalidArgument(format!("{s} は WxH でない")))?;
            Ok::<_, MmError>((
                w.parse::<u32>()
                    .map_err(|e| MmError::InvalidArgument(e.to_string()))?,
                h.parse::<u32>()
                    .map_err(|e| MmError::InvalidArgument(e.to_string()))?,
            ))
        })
        .transpose()?;
    let repeat: usize = args.next().and_then(|r| r.parse().ok()).unwrap_or(3);
    // 名前の一部で GPU を選ぶ（`Basic Render` で WARP。GPU の無い CI の確かめ用）。
    let gpu = args.next().and_then(|name| {
        mm_gpu::enumerate_adapters()
            .into_iter()
            .find(|a| a.name.contains(&name))
    });

    let text = std::fs::read_to_string(&project)?;
    let mut doc: mm_doc::Project = serde_json::from_str(&text)?;
    if let Some((w, h)) = size {
        for comp in &mut doc.compositions {
            (comp.width, comp.height) = (w, h);
        }
        for target in &mut doc.render_targets {
            (target.width, target.height) = (w, h);
        }
    }
    let config = RuntimeConfig {
        decode,
        gpu,
        ..RuntimeConfig::real()
    };
    let mut engine = assemble(&config)?;
    let out = std::env::temp_dir().join(format!("mm_export_bench_{}.mp4", std::process::id()));
    let mut fps = Vec::new();
    for _ in 0..repeat {
        let summary = export_project(
            &mut engine,
            &doc,
            &ExportOptions {
                output: Some(out.clone()),
                target: None,
                ..ExportOptions::default()
            },
        )?;
        fps.push(summary.fps);
    }
    let _ = std::fs::remove_file(&out);
    fps.sort_by(f64::total_cmp);
    println!(
        "decode={} adapter={} frames/run: fps median {:.1} (runs {:?})",
        decode.as_str(),
        engine.gpu_adapter().map(|a| a.name.as_str()).unwrap_or("-"),
        fps[fps.len() / 2],
        fps.iter()
            .map(|f| (f * 10.0).round() / 10.0)
            .collect::<Vec<_>>()
    );
    Ok(())
}
