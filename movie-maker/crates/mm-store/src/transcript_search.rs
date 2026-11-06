//! 文字起こしの全文検索。語から素材と時刻を引く。

use rusqlite::{params, Connection};

use mm_doc::WordId;
use mm_types::{AssetId, Id, MmError, RationalTime, Result};

use crate::store::Store;
use crate::transcript::{time, RUN_EXTRA_CHARS};
use crate::SqlExt;

/// trigram の索引で引ける語の最短の文字数。短い語は `instr` で全部の行を見る。
const TRIGRAM_MIN_CHARS: usize = 3;
/// 語の終わりを求めるときに読む、後ろの単語の数の上限。
const FOLLOW_WORDS: i64 = 64;

/// 検索の 1 件。語が始まる単語と、語が終わる単語の終わりの時刻。
#[derive(Debug, Clone, PartialEq)]
pub struct WordHit {
    pub asset: AssetId,
    /// 語が始まる単語。
    pub word: WordId,
    /// 語が始まる単語の始まり（素材の時間軸）。
    pub start: RationalTime,
    /// 語が終わる単語の終わり。
    pub end: RationalTime,
    /// 語が始まる単語から後ろへ続けた文字（前後の文脈の表示に使う）。
    pub context: String,
}

/// 1 件の候補（語が始まる単語）。
struct Candidate {
    asset: String,
    word: String,
    ord: i64,
    start: (i64, u32),
    /// `run` の中で語が始まる文字の位置（0 から）。
    offset: usize,
    run: String,
}

const SELECT: &str = "SELECT w.asset_id, w.word_id, w.ord, w.start_num, w.start_den,
       instr(w.run, ?1) - 1, w.run FROM words w";
/// 語が始まるのがこの単語の文字の中であること（後ろの単語で始まるものは、その単語の行で引く）と、
/// 素材がプロジェクトにあること。
const WHERE: &str = "w.deleted = 0
   AND instr(w.run, ?1) BETWEEN 1 AND length(coalesce(w.fixed, w.text))
   AND w.asset_id IN (SELECT id FROM assets)
 ORDER BY w.asset_id, w.ord LIMIT ?3";

impl Store {
    /// 文字起こしから語 `query` を引く。単語の境目をまたぐ語も引ける。消した単語は外し、直した文字で引く。
    /// 素材の ID と時刻の順に、最大 `limit` 件。語の長さは [`RUN_EXTRA_CHARS`] 文字まで。
    pub fn search_words(&self, query: &str, limit: usize) -> Result<Vec<WordHit>> {
        let q = query.trim();
        let chars = q.chars().count();
        if chars == 0 || chars > RUN_EXTRA_CHARS {
            return Err(MmError::InvalidArgument(format!(
                "検索の語は 1 文字以上 {RUN_EXTRA_CHARS} 文字まで: {query:?}"
            )));
        }
        let sql = if chars >= TRIGRAM_MIN_CHARS {
            format!(
                "{SELECT} JOIN words_fts f ON f.rowid = w.id WHERE words_fts MATCH ?2 AND {WHERE}"
            )
        } else {
            format!("{SELECT} WHERE ?2 IS NOT NULL AND {WHERE}")
        };
        let phrase = format!("\"{}\"", q.replace('"', "\"\""));
        let mut st = self.conn.prepare(&sql).sql()?;
        let found = st
            .query_map(params![q, phrase, limit as i64], |r| {
                Ok(Candidate {
                    asset: r.get(0)?,
                    word: r.get(1)?,
                    ord: r.get(2)?,
                    start: (r.get(3)?, r.get(4)?),
                    offset: r.get::<_, i64>(5)? as usize,
                    run: r.get(6)?,
                })
            })
            .sql()?
            .collect::<rusqlite::Result<Vec<_>>>()
            .sql()?;
        found
            .into_iter()
            .map(|c| {
                let end = match_end(&self.conn, &c, chars)?;
                Ok(WordHit {
                    asset: Id::new(c.asset),
                    word: Id::new(c.word),
                    start: time(c.start)?,
                    end,
                    context: c.run,
                })
            })
            .collect()
    }
}

/// 語の最後の文字を含む単語の終わり。`run` と同じく消していない単語の表示する文字を数える。
fn match_end(conn: &Connection, c: &Candidate, chars: usize) -> Result<RationalTime> {
    let mut st = conn
        .prepare_cached(
            "SELECT length(coalesce(fixed, text)), end_num, end_den FROM words
             WHERE asset_id = ?1 AND ord >= ?2 AND deleted = 0 ORDER BY ord LIMIT ?3",
        )
        .sql()?;
    let rows = st
        .query_map(params![c.asset, c.ord, FOLLOW_WORDS], |r| {
            Ok((r.get::<_, i64>(0)? as usize, (r.get(1)?, r.get(2)?)))
        })
        .sql()?
        .collect::<rusqlite::Result<Vec<(usize, (i64, u32))>>>()
        .sql()?;
    let last = c.offset + chars;
    let mut seen = 0;
    for (len, end) in &rows {
        seen += len;
        if seen >= last {
            return time(*end);
        }
    }
    let (_, end) = rows
        .last()
        .ok_or_else(|| MmError::Internal(format!("単語 {} が引けない", c.word)))?;
    time(*end)
}
