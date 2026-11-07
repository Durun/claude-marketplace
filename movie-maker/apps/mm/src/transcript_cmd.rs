//! 文字起こしの取り込み（`mm ingest` の付属ファイルから DB へ）と `mm search`。

use std::path::{Path, PathBuf};

use mm_ingest::CacheLayout;
use mm_ingest_api::Transcript;
use mm_store::{Store, TranscriptInput, WordInput};
use mm_types::{AssetId, MmError, Result};

/// `mm search`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// 文字起こしから語を引く。1 行に 1 件（素材、始まりと終わりの秒、単語の ID、続きの文字）。
    Search {
        dir: PathBuf,
        query: String,
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: usize,
        /// 1 行に 1 件の JSON（時刻は有理数）で出す。
        #[arg(long)]
        json: bool,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Search {
                dir,
                query,
                limit,
                json,
            } => search(&dir, &query, limit, json),
        }
    }
}

/// `mm search` が出す件数の既定。
pub const DEFAULT_LIMIT: usize = 50;

/// 付属ファイルの文字起こしを DB の入力にする。単語の時刻の無い区間は区間を 1 つの単語にする。
pub fn to_input(producer: &str, t: &Transcript) -> TranscriptInput {
    let mut words = Vec::new();
    for (i, seg) in t.segments.iter().enumerate() {
        let segment = i as u32;
        if seg.words.is_empty() {
            words.push(WordInput {
                segment,
                start: seg.start,
                end: seg.end,
                text: seg.text.clone(),
            });
        }
        words.extend(seg.words.iter().map(|w| WordInput {
            segment,
            start: w.start,
            end: w.end,
            text: w.text.clone(),
        }));
    }
    TranscriptInput {
        producer: producer.to_owned(),
        language: t.language.clone(),
        words,
    }
}

/// `hashes` の素材の文字起こしの付属ファイル（`cache/ingest/<hash>.transcript.json`）を `put` で DB に入れる。
/// `assets` は（素材の ID、コンテンツハッシュ）。1 つ失敗しても残りを入れ、最後にまとめて誤りにする
/// （素材は足し済みなので、同じ `mm ingest` をもう一度走らせれば文字起こしだけ入れ直せる）。
pub fn put_transcripts(
    cache: &CacheLayout,
    hashes: &[String],
    assets: &[(AssetId, String)],
    mut put: impl FnMut(&AssetId, &str, &TranscriptInput) -> Result<usize>,
) -> Result<()> {
    let mut failures = Vec::new();
    for hash in hashes {
        let Some((asset, _)) = assets.iter().find(|(_, h)| h == hash) else {
            continue;
        };
        match read_transcript(cache, hash) {
            Ok(None) => {}
            Ok(Some(input)) => match put(asset, hash, &input) {
                Ok(n) => println!("{asset}: 文字起こし {n} 語を DB に入れた"),
                Err(e) => failures.push(format!("{asset}: {e}")),
            },
            Err(e) => failures.push(format!("{asset}: {e}")),
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    Err(MmError::InvalidArgument(format!(
        "素材は足したが、文字起こしを入れられなかった。同じ mm ingest をもう一度実行すると入れ直す\n  {}",
        failures.join("\n  ")
    )))
}

/// 付属ファイルの文字起こし。ファイルが無い、または文字起こしでなければ `None`。
fn read_transcript(cache: &CacheLayout, hash: &str) -> Result<Option<TranscriptInput>> {
    let path = cache.sidecar(hash, "transcript");
    if !path.is_file() {
        return Ok(None);
    }
    let sidecar = mm_ingest::sidecar::read_sidecar(&path, hash)?;
    let mm_ingest_api::SidecarBody::Transcript(t) = &sidecar.body else {
        return Ok(None);
    };
    Ok(Some(to_input(&sidecar.producer, t)))
}

/// `mm search <dir> <語>`。文字起こしから語を引き、素材と時刻を出す。`json` なら 1 行に 1 件の JSON。
pub fn search(dir: &Path, query: &str, limit: usize, json: bool) -> Result<()> {
    // 書き手のロックを取らずに読む（アプリが編集している最中でも引ける）。
    let store = Store::open_read_only(dir)?;
    let hits = store.search_words(query, limit)?;
    for h in &hits {
        if json {
            let line = serde_json::json!({
                "asset": h.asset, "word": h.word, "start": h.start, "end": h.end, "context": h.context,
            });
            println!("{line}");
        } else {
            println!(
                "{}\t{:.3}\t{:.3}\t{}\t{}",
                h.asset,
                h.start.to_f64(),
                h.end.to_f64(),
                h.word,
                h.context
            );
        }
    }
    if !json && hits.is_empty() {
        println!("見つからない: {query}");
    }
    Ok(())
}
