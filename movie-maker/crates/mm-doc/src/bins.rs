//! 素材の整理（`bins`）と文字起こしの直し（`transcript_edits`）。
//!
//! 文字起こしの本文は `project.mmdb` の `words` の表に持ち、プロジェクトのデータは素材の ID と時刻の範囲、
//! 単語の ID で指すだけにする。本文はツールが作り直せる取り込みの結果で、人の判断（選別、話題、直し）だけを
//! 操作で変えて取り消せるようにするため。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use mm_types::{AssetId, Id, MmError, RationalTime, Result};

use crate::Project;

/// 選別の ID の型タグ。
#[derive(Debug)]
pub enum SelectionMarker {}
/// 選別の ID。
pub type SelectionId = Id<SelectionMarker>;
/// 話題の ID の型タグ。
#[derive(Debug)]
pub enum TopicMarker {}
/// 話題の ID。
pub type TopicId = Id<TopicMarker>;
/// 単語の ID の型タグ。
#[derive(Debug)]
pub enum WordMarker {}
/// 単語の ID。素材のコンテンツハッシュと開始の時刻から決まる（`mm_store::word_id`）。
pub type WordId = Id<WordMarker>;

/// 評価。FCP の Favorite / Reject と同じ 3 値。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    #[default]
    Unrated,
    Favorite,
    Rejected,
}

/// 素材の整理。Source の印、選別、話題。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Bins {
    /// 素材ごとのイン点とアウト点の印（Source で付ける）。素材の ID の順。
    #[serde(default)]
    pub marks: Vec<SourceMarks>,
    /// ID の順。
    #[serde(default)]
    pub selections: Vec<Selection>,
    /// ID の順。
    #[serde(default)]
    pub topics: Vec<Topic>,
}

/// 素材の印。どちらも無い印は持たない（操作が外す）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SourceMarks {
    pub asset: AssetId,
    /// 素材の時間軸でのイン点。
    #[serde(rename = "in")]
    pub in_point: Option<RationalTime>,
    /// 素材の時間軸でのアウト点（含まない）。
    #[serde(rename = "out")]
    pub out_point: Option<RationalTime>,
}

/// 選別。素材の中の範囲に評価とタグを付ける（FCP のキーワード付きの範囲）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Selection {
    pub id: SelectionId,
    pub asset: AssetId,
    /// 素材の時間軸でのイン点。
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    /// 素材の時間軸でのアウト点（含まない）。`in < out`。
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    #[serde(default)]
    pub rating: Rating,
    /// 辞書順で重複しない。
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 話題。文字起こしの区間（素材の中の範囲）に見出しを付けたもの。Story が並べ、`insertTopics` がカットにする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Topic {
    pub id: TopicId,
    pub asset: AssetId,
    /// 素材の時間軸でのイン点。
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    /// 素材の時間軸でのアウト点（含まない）。`in < out`。
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    pub title: String,
    #[serde(default)]
    pub rating: Rating,
    /// 辞書順で重複しない。
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 文字起こしの 1 単語の直し。本文は `words` の表にあり、ここは単語の ID で指す。
/// 何も変えない直し（`text` が無く `deleted` が偽）は持たない（操作が外す）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WordEdit {
    pub asset: AssetId,
    pub word: WordId,
    /// 直した文字。`None` は元の文字のまま。
    #[serde(default)]
    pub text: Option<String>,
    /// 消した単語（言い淀みなど）。検索と表示から外す。
    #[serde(default)]
    pub deleted: bool,
}

impl WordEdit {
    /// 何も変えていないか。
    pub fn is_noop(&self) -> bool {
        self.text.is_none() && !self.deleted
    }
}

impl Bins {
    /// ID で引く列を並べる。
    pub(crate) fn normalize(&mut self) {
        self.marks.sort_by(|a, b| a.asset.cmp(&b.asset));
        self.selections.sort_by(|a, b| a.id.cmp(&b.id));
        self.topics.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

/// 単語の直しを素材と単語の ID の順に並べる。
pub(crate) fn normalize_edits(edits: &mut [WordEdit]) {
    edits.sort_by(|a, b| (&a.asset, &a.word).cmp(&(&b.asset, &b.word)));
}

impl Project {
    /// 話題を ID で引く。
    pub fn topic(&self, id: &TopicId) -> Result<&Topic> {
        self.bins
            .topics
            .iter()
            .find(|t| &t.id == id)
            .ok_or_else(|| MmError::NotFound(format!("topic {id}")))
    }

    /// 選別を ID で引く。
    pub fn selection(&self, id: &SelectionId) -> Result<&Selection> {
        self.bins
            .selections
            .iter()
            .find(|s| &s.id == id)
            .ok_or_else(|| MmError::NotFound(format!("selection {id}")))
    }

    /// 素材の単語の直し。無ければ `None`。
    pub fn word_edit(&self, asset: &AssetId, word: &WordId) -> Option<&WordEdit> {
        self.transcript_edits
            .iter()
            .find(|e| &e.asset == asset && &e.word == word)
    }

    /// 整理と直しの不変条件。素材が在り、範囲が素材の中で `in < out`、ID が重複しない、タグが辞書順。
    pub(crate) fn validate_bins(&self) -> Result<()> {
        let mut assets = HashSet::new();
        for m in &self.bins.marks {
            self.require_asset("印", &m.asset)?;
            if !assets.insert(m.asset.as_str()) {
                return Err(invalid(format!("素材 {} の印が重複", m.asset)));
            }
            validate_marks(self, m)?;
        }
        let mut ids = HashSet::new();
        for s in &self.bins.selections {
            check_item(
                self,
                &mut ids,
                s.id.as_str(),
                &s.asset,
                s.in_point,
                s.out_point,
            )?;
            check_tags(s.id.as_str(), &s.tags)?;
        }
        for t in &self.bins.topics {
            check_item(
                self,
                &mut ids,
                t.id.as_str(),
                &t.asset,
                t.in_point,
                t.out_point,
            )?;
            check_tags(t.id.as_str(), &t.tags)?;
        }
        let mut words = HashSet::new();
        for e in &self.transcript_edits {
            self.require_asset("単語の直し", &e.asset)?;
            if !words.insert((e.asset.as_str(), e.word.as_str())) {
                return Err(invalid(format!("単語 {} の直しが重複", e.word)));
            }
            if e.is_noop() {
                return Err(invalid(format!("単語 {} の直しが何も変えない", e.word)));
            }
        }
        Ok(())
    }

    fn require_asset(&self, what: &str, asset: &AssetId) -> Result<()> {
        self.asset(asset)
            .map(|_| ())
            .map_err(|_| invalid(format!("{what}の素材 {asset} が無い")))
    }
}

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

fn validate_marks(project: &Project, m: &SourceMarks) -> Result<()> {
    if m.in_point.is_none() && m.out_point.is_none() {
        return Err(invalid(format!("素材 {} の印が空", m.asset)));
    }
    let duration = project.asset(&m.asset)?.duration;
    for t in [m.in_point, m.out_point].into_iter().flatten() {
        if t.is_negative() || t > duration {
            return Err(invalid(format!("素材 {} の印 {t} が素材の外", m.asset)));
        }
    }
    if let (Some(i), Some(o)) = (m.in_point, m.out_point) {
        if i >= o {
            return Err(invalid(format!("素材 {} の印が in < out でない", m.asset)));
        }
    }
    Ok(())
}

/// 選別と話題の共通の検査。ID は選別と話題をまたいで重複しない（`rate` と `tag` が ID だけで指すため）。
fn check_item<'a>(
    project: &Project,
    ids: &mut HashSet<&'a str>,
    id: &'a str,
    asset: &AssetId,
    in_point: RationalTime,
    out_point: RationalTime,
) -> Result<()> {
    if !ids.insert(id) {
        return Err(invalid(format!("選別と話題の ID {id} が重複")));
    }
    let duration = project
        .asset(asset)
        .map_err(|_| invalid(format!("{id} の素材 {asset} が無い")))?
        .duration;
    if in_point.is_negative() || out_point > duration || in_point >= out_point {
        return Err(invalid(format!(
            "{id} の範囲 [{in_point}, {out_point}) が素材の中で in < out でない"
        )));
    }
    Ok(())
}

fn check_tags(id: &str, tags: &[String]) -> Result<()> {
    if tags.iter().any(String::is_empty) {
        return Err(invalid(format!("{id} に空のタグ")));
    }
    if tags.windows(2).any(|w| w[0] >= w[1]) {
        return Err(invalid(format!(
            "{id} のタグが辞書順で重複しない並びでない"
        )));
    }
    Ok(())
}
