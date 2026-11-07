//! `mm ingest`: 素材を取り込み、素材を足す操作（`addAsset`）をプロジェクトに適用する。
//! 取り込み（`mm-ingest`）は `cache/` に出力を作るだけで、プロジェクトのデータを変えるのはここ。
//! 文字起こしの付属ファイルがあれば、本文を DB の表に入れる（`transcript_cmd`）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mm_doc::Asset;
use mm_ingest::{ArtifactReport, ArtifactState, CacheLayout, IngestOptions, IngestReport};
use mm_ingest::{PreviewEncoder, Tools};
use mm_ops::{AddAsset, Op, Transaction};
use mm_types::{AssetId, MmError, Result};

use crate::commands::{actor, cli_context, finish, now_ms, op_error, open_target, Target};
use crate::remote;
use crate::transcript_cmd::put_transcripts;

/// `mm ingest`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// 素材を取り込む。`cache/` にプレビュー動画・波形・サムネイルを作り、素材を足す操作を適用する。
    Ingest {
        dir: PathBuf,
        /// 取り込む素材のパス（1 つ以上）。
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// プレビュー動画のエンコーダー（auto、nvenc、x264）。
        #[arg(long, default_value = "auto", value_parser = parse_encoder)]
        encoder: PreviewEncoder,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Ingest {
                dir,
                inputs,
                encoder,
            } => ingest(&IngestArgs {
                dir,
                inputs,
                encoder,
            }),
        }
    }
}

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
/// 別のプロセス（アプリ）が書き手なら、素材の追加は `/ops`、文字起こしは `/transcripts` で転送する。
pub fn ingest(args: &IngestArgs) -> Result<()> {
    if args.inputs.is_empty() {
        return Err(MmError::InvalidArgument("素材のパスを 1 つ以上渡す".into()));
    }
    let target = open_target(&args.dir)?;
    let opts = IngestOptions {
        cache: CacheLayout::new(args.dir.join(CACHE_DIR)),
        tools: Tools::default(),
        encoder: args.encoder,
    };
    let known = match &target {
        Target::Local(store) => asset_pairs(store.project().assets.iter()),
        Target::Remote(client) => remote::assets(client)?,
    };
    let (ops, hashes) = collect_ops(&opts, &args.inputs, &known)?;
    match target {
        Target::Local(mut store) => {
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
            let assets = asset_pairs(store.project().assets.iter());
            let put = |a: &_, h: &str, t: &_| store.put_transcript(a, h, t);
            put_transcripts(&opts.cache, &hashes, &assets, put)?;
            finish(*store)
        }
        Target::Remote(client) => {
            if !ops.is_empty() {
                remote::op(&client, &Transaction::new(TXN_NAME, ops))?;
            }
            let assets = remote::assets(&client)?;
            let put = |a: &_, h: &str, t: &_| remote::put_transcript(&client, a, h, t);
            put_transcripts(&opts.cache, &hashes, &assets, put)
        }
    }
}

/// 素材の（ID、コンテンツハッシュ）。
fn asset_pairs<'a>(assets: impl Iterator<Item = &'a Asset>) -> Vec<(AssetId, String)> {
    assets
        .map(|a| (a.id.clone(), a.content_hash.clone()))
        .collect()
}

/// 入力を取り込み、プロジェクトにまだ無い素材を足す操作と、全入力のコンテンツハッシュを返す。
fn collect_ops(
    opts: &IngestOptions,
    inputs: &[PathBuf],
    known: &[(AssetId, String)],
) -> Result<(Vec<Op>, Vec<String>)> {
    let mut seen: HashSet<String> = known.iter().map(|(_, h)| h.clone()).collect();
    let mut ops: Vec<Op> = Vec::new();
    let mut hashes = Vec::new();
    for input in inputs {
        let report = mm_ingest::ingest(opts, input)?;
        print_report(input, &report);
        hashes.push(report.asset.content_hash.clone());
        if seen.insert(report.asset.content_hash.clone()) {
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
    Ok((ops, hashes))
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
