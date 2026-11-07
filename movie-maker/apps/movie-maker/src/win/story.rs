//! Story パネルのアプリ側の処理。素材の文字起こし（単語の列、話題、選別）、単語の範囲の時刻、全文検索、
//! 素材の時刻から再生位置への変換。Tauri のコマンドのハンドラーは `story_ipc.rs`、名前と引数の形は `ui/sdk/README.md` の「Story のコマンド」。
//!
//! 単語の本文は `project.mmdb` の `Store::transcript_words` があればそれを、無ければ取り込みの付属ファイル
//! （`cache/ingest/<hash>.transcript.json`）を読む。消した単語・直した文字は、いまのプロジェクトのデータの
//! `transcript_edits` を重ねる（アプリの操作は再生スレッドのデータに先に入るので、DB より新しい）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mm_doc::{Asset, Project, Selection, Topic};
use mm_ingest::CacheLayout;
use mm_ingest_api::Transcript;
use mm_store::{word_id, Store};
use mm_types::{AssetId, Id, RationalTime};
use serde::Serialize;

use crate::win::state::Shared;
use crate::win::timebase;

/// 全文検索の語の長さ（文字数）の上限。`mm search` と同じ。
pub const QUERY_MAX_CHARS: usize = mm_store::RUN_EXTRA_CHARS;
/// 検索結果の文脈に出す文字数。
const CONTEXT_CHARS: usize = 24;

/// 一覧に出す素材 1 つ。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StoryAsset {
    pub id: String,
    pub name: String,
    pub duration: f64,
    pub has_transcript: bool,
}

/// シーケンス 1 つの概要。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StorySequence {
    pub id: String,
    pub cuts: usize,
    pub duration: f64,
}

/// Story を開いたときに要る一覧。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Overview {
    pub assets: Vec<StoryAsset>,
    pub sequences: Vec<StorySequence>,
    /// いま開いているコンポジションの、いちばん下の sequence のレイヤーが置く sequence。無ければ `None`。
    /// Story の Delete（`cutWords`）が切る先。
    pub open_sequence: Option<String>,
}

/// 文字起こしの 1 単語。時刻は素材の時間軸。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StoryWord {
    pub id: String,
    pub segment: u32,
    pub start: RationalTime,
    pub end: RationalTime,
    /// 取り込んだままの文字。
    pub text: String,
    /// 直した文字。
    pub fixed: Option<String>,
    pub deleted: bool,
}

/// 素材の文字起こしと、その素材の話題・選別。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StoryTranscript {
    pub asset: String,
    pub words: Vec<StoryWord>,
    pub topics: Vec<Topic>,
    pub selections: Vec<Selection>,
}

/// 単語の範囲の時刻（素材の時間軸）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WordRange {
    pub start: RationalTime,
    pub end: RationalTime,
}

/// 検索の 1 件。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StoryHit {
    pub asset: String,
    /// 語が始まる単語。
    pub word: String,
    pub start: RationalTime,
    /// 語が終わる単語の終わり。
    pub end: RationalTime,
    pub context: String,
}

/// 取り込んだままの 1 単語（修正を重ねる前）。
struct BaseWord {
    id: String,
    segment: u32,
    start: RationalTime,
    end: RationalTime,
    text: String,
}

/// アプリ全体で 1 つの Story の状態。
pub struct StoryState {
    shared: Arc<Shared>,
    store_dir: Option<PathBuf>,
    cache: CacheLayout,
}

impl StoryState {
    /// `project` は開いたプロジェクトのパス、`cache_root` はその `cache/`。
    pub fn new(shared: Arc<Shared>, project: &Path, cache_root: PathBuf) -> Self {
        Self {
            shared,
            store_dir: mm_store::store_dir(project),
            cache: CacheLayout::new(cache_root),
        }
    }

    fn project(&self) -> Result<(Arc<Project>, mm_types::CompositionId), String> {
        let doc = self.shared.doc().ok_or("プロジェクトを開いていない")?;
        Ok((doc.project, doc.comp))
    }

    fn asset_of(project: &Project, id: &str) -> Result<Asset, String> {
        project
            .asset(&Id::new(id))
            .cloned()
            .map_err(|_| format!("素材が無い: {id}"))
    }

    /// 素材と、シーケンスの一覧。
    pub fn overview(&self) -> Result<Overview, String> {
        let (project, comp) = self.project()?;
        let open_sequence = project.composition(&comp).ok().and_then(|c| {
            c.layers
                .iter()
                .find_map(|l| timebase::layer_sequence(&project, l))
                .map(|s| s.id.to_string())
        });
        let assets = project
            .assets
            .iter()
            .map(|a| StoryAsset {
                id: a.id.to_string(),
                name: file_name(&a.path),
                duration: a.duration.to_f64(),
                has_transcript: self.has_transcript(a),
            })
            .collect();
        let sequences = project
            .sequences
            .iter()
            .map(|s| StorySequence {
                id: s.id.to_string(),
                cuts: s.cuts.len(),
                duration: mm_eval::sequence_duration(s).map_or(0.0, RationalTime::to_f64),
            })
            .collect();
        Ok(Overview {
            assets,
            sequences,
            open_sequence,
        })
    }

    fn has_transcript(&self, asset: &Asset) -> bool {
        self.cache
            .sidecar(&asset.content_hash, "transcript")
            .is_file()
            || self.store_has(&asset.id)
    }

    fn store_has(&self, asset: &AssetId) -> bool {
        self.open_store()
            .is_some_and(|s| s.has_transcript(asset).unwrap_or(false))
    }

    fn open_store(&self) -> Option<Store> {
        Store::open_read_only(self.store_dir.as_deref()?).ok()
    }

    /// 素材の文字起こしの単語に、いまの修正を重ねたもの。文字起こしが無ければ空。
    pub fn words(&self, asset_id: &str) -> Result<Vec<StoryWord>, String> {
        let (project, _) = self.project()?;
        let asset = Self::asset_of(&project, asset_id)?;
        let base = self.base_words(&asset)?;
        Ok(base
            .into_iter()
            .map(|w| {
                let edit = project.word_edit(&asset.id, &Id::new(&w.id));
                StoryWord {
                    id: w.id,
                    segment: w.segment,
                    start: w.start,
                    end: w.end,
                    text: w.text,
                    fixed: edit.and_then(|e| e.text.clone()),
                    deleted: edit.is_some_and(|e| e.deleted),
                }
            })
            .collect())
    }

    fn base_words(&self, asset: &Asset) -> Result<Vec<BaseWord>, String> {
        if let Some(store) = self.open_store() {
            if store.has_transcript(&asset.id).unwrap_or(false) {
                let words = store
                    .transcript_words(&asset.id)
                    .map_err(|e| e.to_string())?;
                return Ok(words
                    .into_iter()
                    .map(|w| BaseWord {
                        id: w.word.to_string(),
                        segment: w.segment,
                        start: w.start,
                        end: w.end,
                        text: w.text,
                    })
                    .collect());
            }
        }
        match mm_ingest::sidecar::load_transcript(&self.cache, &asset.content_hash) {
            Ok(Some(t)) => Ok(sidecar_words(&asset.content_hash, &t)),
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(format!("文字起こしを読めない: {e}")),
        }
    }

    /// 素材の文字起こしと、その素材の話題・選別。
    pub fn transcript(&self, asset_id: &str) -> Result<StoryTranscript, String> {
        let (project, _) = self.project()?;
        let asset = Self::asset_of(&project, asset_id)?;
        Ok(StoryTranscript {
            asset: asset_id.to_owned(),
            words: self.words(asset_id)?,
            topics: of_asset(&project.bins.topics, |t| &t.asset, &asset.id),
            selections: of_asset(&project.bins.selections, |s| &s.asset, &asset.id),
        })
    }

    /// 単語 `first` から `last`（どちらが先でもよい）までの時刻。終わりは、次の単語の始まりより後に出ない
    /// （単語の時刻は隣と重なることがあり、隣り合う話題を並べたときに同じ音が 2 回入らないようにする）。
    pub fn range(&self, asset: &str, first: &str, last: &str) -> Result<WordRange, String> {
        let words = self.words(asset)?;
        let find = |id: &str| {
            words
                .iter()
                .position(|w| w.id == id)
                .ok_or_else(|| format!("単語が無い: {id}"))
        };
        let (a, b) = (find(first)?, find(last)?);
        let (a, b) = (a.min(b), a.max(b));
        Ok(WordRange {
            start: words[a].start,
            end: range_end(&words, b),
        })
    }

    /// 全文検索。`mm search` と同じ規則（単語の境をまたいで探す、消した単語は外す、直した文字で探す）で、
    /// 保存前の修正も見えるよう、パネルに出す単語と同じ列を探す。
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<StoryHit>, String> {
        let q = query.trim();
        let chars = q.chars().count();
        if chars == 0 || chars > QUERY_MAX_CHARS {
            return Err(format!(
                "検索の語は 1 文字以上 {QUERY_MAX_CHARS} 文字まで: {query:?}"
            ));
        }
        let (project, _) = self.project()?;
        let mut hits = Vec::new();
        for asset in &project.assets {
            if hits.len() >= limit {
                break;
            }
            let words = self.words(asset.id.as_str())?;
            hits.extend(find_in(asset.id.as_str(), &words, q, limit - hits.len()));
        }
        Ok(hits)
    }

    /// 素材の時刻 `at`（秒）が、いま開いているコンポジションの撮影素材のレイヤーか sequence のカットに入っているときの
    /// 再生位置（秒）。どこにも入っていなければ `None`。
    pub fn timeline_time(&self, asset: &str, at: f64) -> Result<Option<f64>, String> {
        let (project, comp) = self.project()?;
        let composition = project.composition(&comp).map_err(|e| e.to_string())?;
        Ok(timebase::source_to_comp(&project, composition, asset, at))
    }

    /// カット `cut` の始まりの再生位置（秒）。いま開いているコンポジションに映っていなければ `None`。
    pub fn cut_time(&self, cut: &str) -> Result<Option<f64>, String> {
        let (project, comp) = self.project()?;
        let composition = project.composition(&comp).map_err(|e| e.to_string())?;
        Ok(timebase::cut_to_comp(&project, composition, cut))
    }
}

/// 単語 `b` で終わる範囲の終わり。単語の終わりを、次の単語の始まりまでに詰める。
fn range_end(words: &[StoryWord], b: usize) -> RationalTime {
    match words.get(b + 1) {
        Some(next) if next.start < words[b].end && next.start > words[b].start => next.start,
        _ => words[b].end,
    }
}

fn of_asset<T: Clone>(items: &[T], asset_of: impl Fn(&T) -> &AssetId, asset: &AssetId) -> Vec<T> {
    items
        .iter()
        .filter(|i| asset_of(i) == asset)
        .cloned()
        .collect()
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// 付属ファイルの文字起こしを単語の列にする。単語の時刻の無い区間は区間を 1 つの単語にする。
/// 単語の ID は `Store::put_transcript` と同じ規則（同じ開始の時刻には `_2`、`_3` を付ける）。
fn sidecar_words(content_hash: &str, t: &Transcript) -> Vec<BaseWord> {
    let mut raw = Vec::new();
    for (segment, seg) in t.segments.iter().enumerate() {
        if seg.words.is_empty() {
            raw.push((segment as u32, seg.start, seg.end, seg.text.clone()));
        }
        raw.extend(
            seg.words
                .iter()
                .map(|w| (segment as u32, w.start, w.end, w.text.clone())),
        );
    }
    let mut seen = HashSet::new();
    raw.into_iter()
        .map(|(segment, start, end, text)| {
            let mut nth = 1;
            while !seen.insert(word_id(content_hash, start, nth)) {
                nth += 1;
            }
            BaseWord {
                id: word_id(content_hash, start, nth).to_string(),
                segment,
                start,
                end,
                text,
            }
        })
        .collect()
}

/// 消していない単語の文字を並べた中から `q` を探す。語が始まる単語ごとに 1 件で、最大 `limit` 件。
fn find_in(asset: &str, words: &[StoryWord], q: &str, limit: usize) -> Vec<StoryHit> {
    let shown: Vec<(usize, Vec<char>)> = words
        .iter()
        .enumerate()
        .filter(|(_, w)| !w.deleted)
        .map(|(i, w)| (i, w.fixed.as_deref().unwrap_or(&w.text).chars().collect()))
        .collect();
    let flat: Vec<char> = shown.iter().flat_map(|(_, c)| c.iter().copied()).collect();
    // 平らな文字列の位置から、その文字がある単語の番号（shown の添字）へ。
    let owner: Vec<usize> = shown
        .iter()
        .enumerate()
        .flat_map(|(k, (_, c))| std::iter::repeat_n(k, c.len()))
        .collect();
    let needle: Vec<char> = q.chars().collect();
    let mut hits = Vec::new();
    let mut last_start_word = None;
    for at in 0..flat.len().saturating_sub(needle.len() - 1) {
        if hits.len() >= limit {
            break;
        }
        if flat[at..at + needle.len()] != needle[..] || last_start_word == Some(owner[at]) {
            continue;
        }
        last_start_word = Some(owner[at]);
        let first = &words[shown[owner[at]].0];
        let last = &words[shown[owner[at + needle.len() - 1]].0];
        hits.push(StoryHit {
            asset: asset.to_owned(),
            word: first.id.clone(),
            start: first.start,
            end: last.end,
            context: flat[at..].iter().take(CONTEXT_CHARS).collect(),
        });
    }
    hits
}

#[cfg(test)]
mod tests;
