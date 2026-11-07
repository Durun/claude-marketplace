//! 提案（Claude の編集を保留し、人が承認・却下する単位）のうち、プロジェクトのデータだけで決まる部分。
//! 保留と決定の記録は保存の層（`mm_store` の `proposals` の表）が持つ。

use std::collections::BTreeMap;

use serde::Serialize;

use mm_doc::{Project, Sequence};
use mm_types::MmError;

use crate::context::OpContext;
use crate::error::{OpError, OpResult};
use crate::op::Op;
use crate::ops::with_fitted_layers;
use crate::touches::Touches;
use crate::txn::{apply_transaction, TxnOutcome};

/// 提案の操作から承認する分を選ぶ。`None` は全部。並びは提案の順に揃え、重なり・範囲外・空は拒否する。
pub fn select_ops(ops: &[Op], selection: Option<&[usize]>) -> OpResult<Vec<Op>> {
    let Some(indices) = selection else {
        return Ok(ops.to_vec());
    };
    let mut sorted = indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != indices.len() {
        return Err(invalid("承認する操作の番号が重なっている"));
    }
    if sorted.is_empty() {
        return Err(invalid("承認する操作が 1 つも無い"));
    }
    sorted
        .iter()
        .map(|&i| {
            ops.get(i)
                .cloned()
                .ok_or_else(|| invalid(&format!("操作の番号 {i} は提案に無い（{} 個）", ops.len())))
        })
        .collect()
}

/// 提案の操作をプロジェクトのデータのコピーに適用する。プレビューと、提案を受け付ける前の検証に使う。
/// sequence の長さへの追従（`fitLayer`）も、承認で適用するときと同じく足す。
pub fn preview_ops(
    project: &Project,
    ops: &[Op],
    ctx: &OpContext,
) -> OpResult<(Project, TxnOutcome)> {
    let mut after = project.clone();
    let ops = with_fitted_layers(project, ops);
    let outcome = apply_transaction(&mut after, &ops, ctx)?;
    Ok((after, outcome))
}

fn invalid(message: &str) -> OpError {
    OpError::Doc(MmError::InvalidArgument(message.to_owned()))
}

/// 提案が触る範囲。Review パネルの「どの範囲に」と、プレビューで見せる位置に使う。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProposalScope {
    /// 触るカットを持つ sequence と、その時間軸の上の範囲。
    pub sequences: Vec<SequenceSpan>,
    /// 触るレイヤー。
    pub layers: Vec<LayerRef>,
    /// sequence とレイヤーに当たらない ID（素材、効果、マーカーなど）。
    pub other: Vec<String>,
}

/// sequence の上で触る範囲。秒は sequence の時間軸。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SequenceSpan {
    pub id: String,
    pub cuts: Vec<String>,
    pub start: f64,
    pub end: f64,
}

/// コンポジションの中のレイヤー。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerRef {
    pub comp: String,
    pub id: String,
}

/// カットの sequence の上の位置（始まりと終わりの秒）。外したカットは長さ 0 として前のカットの終わりに置く。
fn cut_spans(seq: &Sequence) -> Vec<(String, f64, f64)> {
    let mut head = 0.0;
    seq.cuts
        .iter()
        .map(|cut| {
            let length = match (cut.enabled, cut.duration()) {
                (true, Ok(d)) => d.to_f64(),
                _ => 0.0,
            };
            let start = head;
            head += length;
            (cut.id.as_str().to_owned(), start, head)
        })
        .collect()
}

/// sequence ごとの触ったカットと範囲を集める。先に見た側（操作の後）の位置を採る。
#[derive(Default)]
struct SpanAcc {
    by_seq: BTreeMap<String, (Vec<String>, f64, f64)>,
}

impl SpanAcc {
    fn add(&mut self, seq: &str, cut: &str, start: f64, end: f64) {
        let entry = self
            .by_seq
            .entry(seq.to_owned())
            .or_insert_with(|| (Vec::new(), start, end));
        if entry.0.iter().any(|c| c == cut) {
            return;
        }
        entry.0.push(cut.to_owned());
        entry.1 = entry.1.min(start);
        entry.2 = entry.2.max(end);
    }

    /// sequence そのものを触ったがカットを触っていなければ、全体を範囲にする。
    fn add_whole(&mut self, seq: &Sequence) {
        let total = cut_spans(seq).last().map_or(0.0, |c| c.2);
        self.by_seq
            .entry(seq.id.as_str().to_owned())
            .or_insert_with(|| (Vec::new(), 0.0, total));
    }
}

/// 触った ID を sequence、レイヤー、そのほかに分ける。消したものも見えるよう、操作の前と後の両方を引く。
pub fn proposal_scope(before: &Project, after: &Project, touches: &Touches) -> ProposalScope {
    let mut spans = SpanAcc::default();
    let mut layers = Vec::new();
    let mut other = Vec::new();
    for id in touches.iter() {
        let found_cut = [after, before].iter().any(|p| add_cut(p, id, &mut spans));
        if found_cut {
            continue;
        }
        if let Some(seq) = [after, before]
            .iter()
            .find_map(|p| p.sequences.iter().find(|s| s.id.as_str() == id))
        {
            spans.add_whole(seq);
            continue;
        }
        match [after, before].iter().find_map(|p| find_layer(p, id)) {
            Some(layer) => layers.push(layer),
            None => other.push(id.to_owned()),
        }
    }
    let sequences = spans
        .by_seq
        .into_iter()
        .map(|(id, (cuts, start, end))| SequenceSpan {
            id,
            cuts,
            start,
            end,
        })
        .collect();
    ProposalScope {
        sequences,
        layers,
        other,
    }
}

fn add_cut(project: &Project, id: &str, spans: &mut SpanAcc) -> bool {
    for seq in &project.sequences {
        if let Some((cut, start, end)) = cut_spans(seq).into_iter().find(|c| c.0 == id) {
            spans.add(seq.id.as_str(), &cut, start, end);
            return true;
        }
    }
    false
}

fn find_layer(project: &Project, id: &str) -> Option<LayerRef> {
    project.compositions.iter().find_map(|c| {
        c.layers
            .iter()
            .any(|l| l.id.as_str() == id)
            .then(|| LayerRef {
                comp: c.id.as_str().to_owned(),
                id: id.to_owned(),
            })
    })
}
