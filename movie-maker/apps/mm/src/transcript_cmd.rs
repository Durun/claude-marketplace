//! 文字起こしの取り込み（`mm ingest` の付属ファイルから DB へ）と `mm search`。

use std::path::Path;

use mm_ingest::CacheLayout;
use mm_ingest_api::Transcript;
use mm_store::{Store, TranscriptInput, WordInput};
use mm_types::Result;

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

/// プロジェクトの素材のうち、`hashes` の素材の文字起こしの付属ファイル（`cache/ingest/<hash>.transcript.json`）を DB に入れる。
pub fn put_transcripts(store: &mut Store, cache: &CacheLayout, hashes: &[String]) -> Result<()> {
    for hash in hashes {
        let path = cache.sidecar(hash, "transcript");
        if !path.is_file() {
            continue;
        }
        let sidecar = mm_ingest::sidecar::read_sidecar(&path, hash)?;
        let mm_ingest_api::SidecarBody::Transcript(t) = &sidecar.body else {
            continue;
        };
        let Some(asset) = store
            .project()
            .assets
            .iter()
            .find(|a| &a.content_hash == hash)
            .map(|a| a.id.clone())
        else {
            continue;
        };
        let n = store.put_transcript(&asset, hash, &to_input(&sidecar.producer, t))?;
        println!("{asset}: 文字起こし {n} 語を DB に入れた");
    }
    Ok(())
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
