//! `cutWords`。文字起こしの単語を消し、その音をシーケンスのカットから抜いて詰める。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use mm_doc::{Cut, Project, SequenceId, WordId, FOOTAGE_KIND, FOOTAGE_PARAM};
use mm_types::{AssetId, Id, RationalTime, Result};

use crate::op::{Apply, Op};
use crate::ops::bins::update_edit;
use crate::ops::{cut_referrers, cut_refs_reach, invalid};
use crate::touches::Touches;

/// 素材の時刻の範囲 `[in, out)`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRange {
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    #[serde(rename = "out")]
    pub out_point: RationalTime,
}

/// シーケンスの `index` から `count` 個のカットを `cuts` で置き換える。`cutWords` の逆操作が持つ形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutSplice {
    pub index: usize,
    pub count: usize,
    pub cuts: Vec<Cut>,
}

/// 素材 `asset` の単語 `words` を消し（`restore` で戻し）、素材の時刻の範囲 `ranges` を、シーケンス `sequence` のうち
/// その素材を映すカットから抜いて後ろを詰める。範囲がカットの中ほどなら割って間を抜き、カットの端なら端を詰め、
/// カットを覆えばカットを抜く。どのカットにも掛からない範囲は単語を消すだけ。後ろの半分のカットの ID は `<元の ID>-<番号>`
/// の空いているもの。削除のフラグは素材に付くので全部のシーケンスの文字起こしに効くが、カットは `sequence` だけを切る
/// （本編から分けたショートを削らない）。1 つの操作なので、1 回の取り消しで単語とカットが戻る。
///
/// `splices` を渡すと `ranges` の代わりにその置き換えを順に行う（逆操作の形。人は省く）。
/// 既にその状態の単語は変えずに通し、逆操作には変えた単語だけを載せる。何も変わらない操作は拒否する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutWords {
    pub asset: AssetId,
    pub sequence: SequenceId,
    #[serde(default)]
    pub words: Vec<WordId>,
    #[serde(default)]
    pub ranges: Vec<SourceRange>,
    #[serde(default)]
    pub restore: bool,
    #[serde(default)]
    pub splices: Vec<CutSplice>,
}

impl CutWords {
    /// 行うカットの置き換えの列。後ろのカットから並べ、前の置き換えが後の位置をずらさない。
    fn plan(&self, project: &Project) -> Result<Vec<CutSplice>> {
        let seq = project.sequence(&self.sequence)?;
        if !self.splices.is_empty() {
            if !self.ranges.is_empty() {
                return Err(invalid("ranges と splices は一緒に渡さない"));
            }
            return Ok(self.splices.clone());
        }
        if self.restore && !self.ranges.is_empty() {
            return Err(invalid("戻すときは ranges を渡さない（取り消しで戻す）"));
        }
        let ranges = normalize(&self.ranges)?;
        let mut taken: HashSet<String> = all_cut_ids(project);
        let mut out = Vec::new();
        for (index, cut) in seq.cuts.iter().enumerate().rev() {
            if !shows_asset(cut, &self.asset) {
                continue;
            }
            if let Some(cuts) = pieces(cut, &ranges, &mut taken) {
                out.push(CutSplice {
                    index,
                    count: 1,
                    cuts,
                });
            }
        }
        Ok(out)
    }

    /// 単語の削除のフラグを変え、変えた単語を返す。
    fn set_words(&self, project: &mut Project) -> Result<Vec<WordId>> {
        project.asset(&self.asset)?;
        let want = !self.restore;
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for word in &self.words {
            if !seen.insert(word) {
                return Err(invalid(format!("単語 {word} を 2 回挙げている")));
            }
            let old = update_edit(project, &self.asset, word, |e| e.deleted = want)?;
            if old.deleted != want {
                changed.push(word.clone());
            }
        }
        Ok(changed)
    }
}

impl Apply for CutWords {
    fn touches(&self, project: &Project) -> Result<Touches> {
        let mut t = Touches::of(&self.asset);
        t.add(&self.sequence);
        for w in &self.words {
            t.add(w);
        }
        if let (Ok(splices), Ok(seq)) = (self.plan(project), project.sequence(&self.sequence)) {
            splice_touches(project, seq.cuts.clone(), &splices, &mut t);
        }
        Ok(t)
    }

    fn apply(&self, project: &mut Project) -> Result<Op> {
        let splices = self.plan(project)?;
        let mut inverse = Vec::with_capacity(splices.len());
        for s in &splices {
            inverse.push(apply_splice(project, &self.sequence, s)?);
        }
        inverse.reverse();
        let words = self.set_words(project)?;
        if words.is_empty() && inverse.is_empty() {
            return Err(invalid("変わる単語もカットも無い"));
        }
        Ok(CutWords {
            asset: self.asset.clone(),
            sequence: self.sequence.clone(),
            words,
            ranges: Vec::new(),
            restore: !self.restore,
            splices: inverse,
        }
        .into())
    }
}

/// 範囲を始まりの順に並べ、重なるか接するものを 1 つにする。`in < out` でない範囲は拒否する。
fn normalize(ranges: &[SourceRange]) -> Result<Vec<SourceRange>> {
    let mut sorted = ranges.to_vec();
    if let Some(bad) = sorted.iter().find(|r| r.in_point >= r.out_point) {
        return Err(invalid(format!(
            "範囲 [{}, {}) が in < out でない",
            bad.in_point, bad.out_point
        )));
    }
    sorted.sort_by_key(|r| r.in_point);
    let mut out: Vec<SourceRange> = Vec::with_capacity(sorted.len());
    for r in sorted {
        match out.last_mut() {
            Some(last) if r.in_point <= last.out_point => {
                last.out_point = last.out_point.max(r.out_point);
            }
            _ => out.push(r),
        }
    }
    Ok(out)
}

/// カットが素材 `asset` の撮影素材のノードか。
fn shows_asset(cut: &Cut, asset: &AssetId) -> bool {
    cut.source.kind == FOOTAGE_KIND
        && cut
            .source
            .params
            .get(FOOTAGE_PARAM)
            .and_then(|p| p.value.as_asset())
            .is_some_and(|a| a == asset)
}

/// カットの素材の範囲から `ranges` を除いた残りの区間。範囲が掛からなければ `None`。
fn remaining(cut: &Cut, ranges: &[SourceRange]) -> Option<Vec<(RationalTime, RationalTime)>> {
    let mut cur = cut.in_point;
    let mut out = Vec::new();
    for r in ranges {
        if r.out_point <= cur {
            continue;
        }
        if r.in_point >= cut.out_point {
            break;
        }
        if r.in_point > cur {
            out.push((cur, r.in_point));
        }
        cur = cur.max(r.out_point);
    }
    if cur < cut.out_point {
        out.push((cur, cut.out_point));
    }
    let untouched = out == [(cut.in_point, cut.out_point)];
    (!untouched).then_some(out)
}

/// カットを残りの区間ごとのカットにする。最初の区間は元の ID とトランジションを持ち、残りは新しい ID を取る。
fn pieces(cut: &Cut, ranges: &[SourceRange], taken: &mut HashSet<String>) -> Option<Vec<Cut>> {
    let parts = remaining(cut, ranges)?;
    let mut cuts = Vec::with_capacity(parts.len());
    for (k, (from, to)) in parts.into_iter().enumerate() {
        let mut piece = cut.clone();
        piece.in_point = from;
        piece.out_point = to;
        if k > 0 {
            piece.id = Id::new(fresh_id(cut.id.as_str(), taken));
            piece.transition_in = None;
        }
        cuts.push(piece);
    }
    Some(cuts)
}

/// `<base>-<番号>` のうち、まだ使っていない最初のもの。
fn fresh_id(base: &str, taken: &mut HashSet<String>) -> String {
    let id = (1u32..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains(id))
        .expect("番号は尽きない");
    taken.insert(id.clone());
    id
}

fn all_cut_ids(project: &Project) -> HashSet<String> {
    project
        .sequences
        .iter()
        .flat_map(|s| s.cuts.iter())
        .map(|c| c.id.to_string())
        .collect()
}

/// 置き換えを 1 つ行い、それを打ち消す置き換えを返す。
fn apply_splice(project: &mut Project, sequence: &SequenceId, s: &CutSplice) -> Result<CutSplice> {
    let seq = project.sequence_mut(sequence)?;
    let len = seq.cuts.len();
    let end = s.index.checked_add(s.count).filter(|e| *e <= len);
    let Some(end) = end else {
        return Err(invalid(format!(
            "カットの範囲 {}+{} がシーケンス {sequence} の {len} 個の中に無い",
            s.index, s.count
        )));
    };
    let removed = seq
        .cuts
        .splice(s.index..end, s.cuts.iter().cloned())
        .collect();
    Ok(CutSplice {
        index: s.index,
        count: s.cuts.len(),
        cuts: removed,
    })
}

/// 置き換えが読む・書く所。シーケンスのカットのコピー `cuts` に順に適用し、抜くカット、足すカット、その前後、
/// 接続したレイヤーを集める。
fn splice_touches(project: &Project, mut cuts: Vec<Cut>, splices: &[CutSplice], t: &mut Touches) {
    for s in splices {
        let end = (s.index + s.count).min(cuts.len());
        let start = s.index.min(end);
        let around = start.checked_sub(1).into_iter().chain([end]);
        for c in around.filter_map(|i| cuts.get(i)) {
            t.add(&c.id);
        }
        for c in cuts[start..end].iter().chain(&s.cuts) {
            t.add(&c.id);
            cut_refs_reach(project, c, t);
            cut_referrers(project, &c.id, t);
        }
        cuts.splice(start..end, s.cuts.iter().cloned());
    }
}
