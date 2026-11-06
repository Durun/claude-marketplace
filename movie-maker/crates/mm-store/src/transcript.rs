//! 文字起こしの本文（`transcripts`、`words`、`words_fts`）。
//!
//! 本文は取り込みのツールが作り直せる結果なので、操作の記録に載せずに表へ直接書く。
//! 人の直し（消した単語、直した文字）はプロジェクトのデータの `transcript_edits` にあり、
//! 操作を書くたびに同じトランザクションで `words` の `fixed`・`deleted`・`run` に写す。

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use mm_doc::{Project, WordEdit, WordId};
use mm_types::{AssetId, Id, MmError, RationalTime, Result};

use crate::store::Store;
use crate::SqlExt;

/// `words.run` に、単語の文字の後ろへ続ける文字数の上限。検索できる語の長さの上限でもある。
pub const RUN_EXTRA_CHARS: usize = 32;
/// 単語の ID に使うコンテンツハッシュの先頭の文字数。
const WORD_ID_HASH_CHARS: usize = 12;

/// 取り込む文字起こし。`mm-ingest-api` の付属ファイルから呼び出し側が組み立てる。
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptInput {
    /// 作ったツールの名前とバージョン。
    pub producer: String,
    /// ISO 639-1 の言語コード。
    pub language: String,
    /// 時刻の順の単語。単語の時刻を出さないツールは区間を 1 つの単語として渡す。
    pub words: Vec<WordInput>,
}

/// 取り込む 1 単語。
#[derive(Debug, Clone, PartialEq)]
pub struct WordInput {
    /// 区間（文）の番号。0 から。
    pub segment: u32,
    pub start: RationalTime,
    pub end: RationalTime,
    pub text: String,
}

/// 表に入った 1 単語と、プロジェクトのデータの直し。
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptWord {
    pub word: WordId,
    pub segment: u32,
    pub start: RationalTime,
    pub end: RationalTime,
    /// 取り込んだままの文字。
    pub text: String,
    /// 直した文字（`fixTranscript`）。
    pub fixed: Option<String>,
    /// 消した単語（`deleteWords`）。
    pub deleted: bool,
}

impl TranscriptWord {
    /// 表示する文字。消した単語は `None`。
    pub fn shown(&self) -> Option<&str> {
        (!self.deleted).then(|| self.fixed.as_deref().unwrap_or(&self.text))
    }
}

/// 単語の ID。「素材のコンテンツハッシュ + 開始の時刻」から決まり、同じ素材を取り込み直しても変わらない。
/// 同じ開始の時刻の単語が続くときは、2 つ目から `_2`、`_3` を付ける。
pub fn word_id(content_hash: &str, start: RationalTime, nth: usize) -> WordId {
    let short: String = content_hash.chars().take(WORD_ID_HASH_CHARS).collect();
    let base = format!("w_{short}_{}_{}", start.num(), start.den());
    match nth {
        0 | 1 => Id::new(base),
        n => Id::new(format!("{base}_{n}")),
    }
}

/// 表示する文字の列から `run` の列を作る。消した単語（`None`）は空で、後ろの単語の `run` にも入らない。
pub(crate) fn runs(shown: &[Option<&str>]) -> Vec<String> {
    let mut out = vec![String::new(); shown.len()];
    for (i, own) in shown.iter().enumerate() {
        let Some(own) = own else { continue };
        let mut run = (*own).to_owned();
        let mut extra = 0;
        for next in shown[i + 1..].iter().flatten() {
            if extra >= RUN_EXTRA_CHARS {
                break;
            }
            let take: String = next.chars().take(RUN_EXTRA_CHARS - extra).collect();
            extra += take.chars().count();
            run.push_str(&take);
        }
        out[i] = run;
    }
    out
}

impl Store {
    /// 素材 `asset` の文字起こしを入れる（前の本文は置き換える）。入れた単語の数を返す。
    /// 単語の時刻は素材の時間軸で、`end < start` の単語は拒否する。
    pub fn put_transcript(
        &mut self,
        asset: &AssetId,
        content_hash: &str,
        input: &TranscriptInput,
    ) -> Result<usize> {
        self.require_writer()?;
        if let Some(w) = input.words.iter().find(|w| w.end < w.start) {
            return Err(MmError::InvalidArgument(format!(
                "単語 {} の終わりが始まりより前",
                w.text
            )));
        }
        let edits = edits_of(&self.project, asset);
        let tx = self.conn.transaction().sql()?;
        tx.execute("DELETE FROM words WHERE asset_id = ?1", [asset.as_str()])
            .sql()?;
        tx.execute(
            "INSERT INTO transcripts(asset_id, content_hash, producer, language) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(asset_id) DO UPDATE SET content_hash = excluded.content_hash,
               producer = excluded.producer, language = excluded.language",
            params![asset.as_str(), content_hash, input.producer, input.language],
        )
        .sql()?;
        insert_words(&tx, asset, content_hash, &input.words, &edits)?;
        tx.commit().sql()?;
        Ok(input.words.len())
    }

    /// 素材の文字起こしがあるか。
    pub fn has_transcript(&self, asset: &AssetId) -> Result<bool> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM transcripts WHERE asset_id = ?1",
                [asset.as_str()],
                |r| r.get(0),
            )
            .optional()
            .sql()?;
        Ok(found.is_some())
    }

    /// 素材の文字起こしの単語を時刻の順に。直しを含む。無ければ空。
    pub fn transcript_words(&self, asset: &AssetId) -> Result<Vec<TranscriptWord>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT word_id, segment, start_num, start_den, end_num, end_den, text, fixed, deleted
                 FROM words WHERE asset_id = ?1 ORDER BY ord",
            )
            .sql()?;
        let rows = st
            .query_map([asset.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    (r.get::<_, i64>(2)?, r.get::<_, u32>(3)?),
                    (r.get::<_, i64>(4)?, r.get::<_, u32>(5)?),
                    r.get::<_, String>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, bool>(8)?,
                ))
            })
            .sql()?
            .collect::<rusqlite::Result<Vec<_>>>()
            .sql()?;
        rows.into_iter()
            .map(|(word, segment, s, e, text, fixed, deleted)| {
                Ok(TranscriptWord {
                    word: Id::new(word),
                    segment,
                    start: time(s)?,
                    end: time(e)?,
                    text,
                    fixed,
                    deleted,
                })
            })
            .collect()
    }
}

/// 表の (分子, 分母) を時刻に。
pub(crate) fn time((num, den): (i64, u32)) -> Result<RationalTime> {
    RationalTime::new(i128::from(num), den)
}

fn i64_of(t: RationalTime) -> Result<i64> {
    i64::try_from(t.num())
        .map_err(|_| MmError::InvalidArgument(format!("時刻 {t} が表に収まらない")))
}

fn edits_of<'a>(project: &'a Project, asset: &AssetId) -> Vec<&'a WordEdit> {
    project
        .transcript_edits
        .iter()
        .filter(|e| &e.asset == asset)
        .collect()
}

/// 単語の ID の列。同じ開始の時刻の単語には `_2`、`_3` を付けて重ならないようにする。
fn assign_ids(content_hash: &str, words: &[WordInput]) -> Vec<WordId> {
    let mut seen = HashSet::new();
    words
        .iter()
        .map(|w| {
            let mut nth = 1;
            while !seen.insert(word_id(content_hash, w.start, nth)) {
                nth += 1;
            }
            word_id(content_hash, w.start, nth)
        })
        .collect()
}

fn insert_words(
    tx: &Connection,
    asset: &AssetId,
    content_hash: &str,
    words: &[WordInput],
    edits: &[&WordEdit],
) -> Result<()> {
    let ids = assign_ids(content_hash, words);
    let by_word: HashMap<&WordId, &WordEdit> = edits.iter().map(|e| (&e.word, *e)).collect();
    let find = |id: &WordId| by_word.get(id).copied();
    let shown: Vec<Option<&str>> = words
        .iter()
        .zip(&ids)
        .map(|(w, id)| match find(id) {
            Some(e) if e.deleted => None,
            Some(e) => Some(e.text.as_deref().unwrap_or(&w.text)),
            None => Some(w.text.as_str()),
        })
        .collect();
    let runs = runs(&shown);
    let mut st = tx
        .prepare(
            "INSERT INTO words(asset_id, word_id, ord, segment, start_num, start_den, end_num, end_den,
                               text, fixed, deleted, run)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .sql()?;
    for (ord, ((w, id), run)) in words.iter().zip(&ids).zip(&runs).enumerate() {
        let edit = find(id);
        st.execute(params![
            asset.as_str(),
            id.as_str(),
            ord as i64,
            w.segment,
            i64_of(w.start)?,
            w.start.den(),
            i64_of(w.end)?,
            w.end.den(),
            w.text,
            edit.and_then(|e| e.text.clone()),
            edit.is_some_and(|e| e.deleted),
            run
        ])
        .sql()?;
    }
    Ok(())
}

impl Store {
    /// 新しく足した・変えた直しの単語が、素材の文字起こしに在るか確かめる。無い単語への直しは、
    /// 表に写らず検索にも現れないまま `transcript_edits` に残るので、書く前に拒否する。
    pub(crate) fn check_edited_words(&self, new: &Project) -> Result<()> {
        let old: HashMap<(&AssetId, &WordId), &WordEdit> = self
            .project
            .transcript_edits
            .iter()
            .map(|e| ((&e.asset, &e.word), e))
            .collect();
        let mut known: HashMap<&AssetId, HashSet<String>> = HashMap::new();
        for edit in &new.transcript_edits {
            if old.get(&(&edit.asset, &edit.word)) == Some(&edit) {
                continue;
            }
            if !known.contains_key(&edit.asset) {
                known.insert(&edit.asset, self.word_ids_of(&edit.asset)?);
            }
            if !known[&edit.asset].contains(edit.word.as_str()) {
                return Err(MmError::InvalidArgument(format!(
                    "単語 {} は素材 {} の文字起こしに無い",
                    edit.word, edit.asset
                )));
            }
        }
        Ok(())
    }

    fn word_ids_of(&self, asset: &AssetId) -> Result<HashSet<String>> {
        let mut st = self
            .conn
            .prepare("SELECT word_id FROM words WHERE asset_id = ?1")
            .sql()?;
        let ids = st
            .query_map([asset.as_str()], |r| r.get::<_, String>(0))
            .sql()?
            .collect::<rusqlite::Result<HashSet<String>>>()
            .sql()?;
        Ok(ids)
    }
}

/// 直しが変わった素材の `words` を、新しいプロジェクトのデータの直しに合わせる。
pub(crate) fn sync_edits(tx: &Connection, old: &Project, new: &Project) -> Result<()> {
    let mut assets: Vec<&AssetId> = old
        .transcript_edits
        .iter()
        .chain(&new.transcript_edits)
        .map(|e| &e.asset)
        .collect();
    assets.sort();
    assets.dedup();
    for asset in assets {
        if edits_of(old, asset) != edits_of(new, asset) {
            refresh_asset(tx, asset, &edits_of(new, asset))?;
        }
    }
    Ok(())
}

/// 素材の全部の単語の `fixed`・`deleted`・`run` を直しから作り直し、変わった行だけ書く。
fn refresh_asset(tx: &Connection, asset: &AssetId, edits: &[&WordEdit]) -> Result<()> {
    type Row = (i64, String, String, Option<String>, bool, String);
    let mut st = tx
        .prepare("SELECT id, word_id, text, fixed, deleted, run FROM words WHERE asset_id = ?1 ORDER BY ord")
        .sql()?;
    let rows: Vec<Row> = st
        .query_map([asset.as_str()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .sql()?
        .collect::<rusqlite::Result<_>>()
        .sql()?;
    let by_word: HashMap<&str, &WordEdit> = edits.iter().map(|e| (e.word.as_str(), *e)).collect();
    let edit_of = |word: &str| by_word.get(word).copied();
    let wanted: Vec<(Option<String>, bool)> = rows
        .iter()
        .map(|(_, word, ..)| edit_of(word).map_or((None, false), |e| (e.text.clone(), e.deleted)))
        .collect();
    let shown: Vec<Option<&str>> = rows
        .iter()
        .zip(&wanted)
        .map(|((_, _, text, ..), (fixed, deleted))| {
            (!deleted).then(|| fixed.as_deref().unwrap_or(text))
        })
        .collect();
    let runs = runs(&shown);
    let mut up = tx
        .prepare("UPDATE words SET fixed = ?2, deleted = ?3, run = ?4 WHERE id = ?1")
        .sql()?;
    for ((row, (fixed, deleted)), run) in rows.iter().zip(&wanted).zip(&runs) {
        if (&row.3, row.4, &row.5) != (fixed, *deleted, run) {
            up.execute(params![row.0, fixed, deleted, run]).sql()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_は後ろの単語を続け_消した単語を飛ばす() {
        let r = runs(&[Some("動画"), Some("編"), None, Some("集")]);
        assert_eq!(r, ["動画編集", "編集", "", "集"]);
    }

    #[test]
    fn run_の続きは上限の文字数で切る() {
        let long = "あ".repeat(RUN_EXTRA_CHARS + 5);
        let r = runs(&[Some("x"), Some(&long)]);
        assert_eq!(r[0].chars().count(), 1 + RUN_EXTRA_CHARS);
    }

    #[test]
    fn 単語の_id_はハッシュと開始の時刻から決まる() {
        let t = RationalTime::new(111, 50).unwrap();
        assert_eq!(
            word_id("abcdef0123456789", t, 1).as_str(),
            "w_abcdef012345_111_50"
        );
        assert_eq!(
            word_id("abcdef0123456789", t, 2).as_str(),
            "w_abcdef012345_111_50_2"
        );
    }
}
