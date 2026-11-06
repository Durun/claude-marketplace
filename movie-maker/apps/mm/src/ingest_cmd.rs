//! `mm ingest`: 素材を取り込み、素材を足す操作（`addAsset`）をプロジェクトに適用する。
//! 取り込み（`mm-ingest`）は `cache/` に出力を作るだけで、プロジェクトのデータを変えるのはここ。
//! 文字起こしの付属ファイルがあれば、本文を DB の表に入れる（`transcript_cmd`）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mm_ingest::{ArtifactReport, ArtifactState, CacheLayout, IngestOptions, IngestReport};
use mm_ingest::{PreviewEncoder, Tools};
use mm_ops::{AddAsset, Op, Transaction};
use mm_types::{MmError, Result};

use crate::commands::{actor, cli_context, finish, now_ms, op_error, open_writer};

/// プロジェクトの中のキャッシュのフォルダー名。
const CACHE_DIR: &str = "cache";
/// トランザクションの名前。
const TXN_NAME: &str = "取り込み";

/// `mm ingest <dir> <素材のパス>...` の引数。
pub struct IngestArgs {
    pub dir: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub encoder: PreviewEncoder,
}

/// `--encoder` の値。
pub fn parse_encoder(text: &str) -> std::result::Result<PreviewEncoder, String> {
    match text {
        "auto" => Ok(PreviewEncoder::Auto),
        "nvenc" => Ok(PreviewEncoder::Nvenc),
        "x264" => Ok(PreviewEncoder::X264),
        other => Err(format!("auto、nvenc、x264 のどれか: {other}")),
    }
}

/// 素材を 1 つずつ取り込み、新しい素材をまとめて 1 つのトランザクションで足す。
pub fn ingest(args: &IngestArgs) -> Result<()> {
    if args.inputs.is_empty() {
        return Err(MmError::InvalidArgument("素材のパスを 1 つ以上渡す".into()));
    }
    let mut store = open_writer(&args.dir)?;
    let opts = IngestOptions {
        cache: CacheLayout::new(args.dir.join(CACHE_DIR)),
        tools: Tools::default(),
        encoder: args.encoder,
    };
    let mut known: HashSet<String> = store
        .project()
        .assets
        .iter()
        .map(|a| a.content_hash.clone())
        .collect();
    let mut ops: Vec<Op> = Vec::new();
    let mut hashes = Vec::new();
    for input in &args.inputs {
        let report = mm_ingest::ingest(&opts, input)?;
        print_report(input, &report);
        hashes.push(report.asset.content_hash.clone());
        if known.insert(report.asset.content_hash.clone()) {
            ops.push(
                AddAsset {
                    asset: report.asset,
                    index: None,
                }
                .into(),
            );
        } else {
            println!("  素材は既にプロジェクトにある（同じコンテンツハッシュ）");
        }
    }
    if !ops.is_empty() {
        let count = ops.len();
        let stored = store
            .apply_txn(
                Transaction::new(TXN_NAME, ops),
                &cli_context(),
                actor(),
                now_ms(),
                None,
            )
            .map_err(op_error)?;
        println!("バージョン {}: 素材を {count} 件足した", stored.doc_version);
    }
    crate::transcript_cmd::put_transcripts(&mut store, &opts.cache, &hashes)?;
    finish(store)
}

fn print_report(input: &Path, r: &IngestReport) {
    println!(
        "{}: {} （{:.2} 秒）",
        input.display(),
        r.asset.id,
        r.total_elapsed.as_secs_f64()
    );
    println!(
        "  ハッシュ {:.2} 秒、調べ {:.2} 秒",
        r.hash_elapsed.as_secs_f64(),
        r.probe_elapsed.as_secs_f64()
    );
    for (name, art) in [
        ("プレビュー動画", &r.preview),
        ("サムネイル", &r.thumbnail),
        ("波形", &r.waveform),
    ] {
        if let Some(a) = art {
            println!("  {name}: {}", describe(a));
        }
    }
    if let Some(n) = r.transcript_segments {
        println!("  文字起こし: {n} 区間");
    }
}

fn describe(a: &ArtifactReport) -> String {
    match a.state {
        ArtifactState::Cached => "スキップした（すでにある）".to_owned(),
        ArtifactState::Created => {
            let note = a
                .note
                .as_deref()
                .map_or(String::new(), |n| format!("、{n}"));
            format!("{:.2} 秒{note}", a.elapsed.as_secs_f64())
        }
    }
}
