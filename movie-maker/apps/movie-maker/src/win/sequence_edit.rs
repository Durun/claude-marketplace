//! Timeline と Source の編集。ドラッグとキーを `mm-ops` の操作に直す。
//!
//! 操作を作る関数は、開いているプロジェクトのデータと選択、再生位置だけを見る（適用はしない）ので、
//! 適用の前に何が起きるかを単体で試せる。適用は `edit::submit`（再生スレッドが持つデータに対して）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use mm_doc::{
    Composition, Cut, CutId, Layer, Node, Project, Sequence, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_eval::{place_cuts, PlacedCut};
use mm_ops::{Edge, Insert, Lift, Op, Overwrite, Ripple, Roll, Slide, Slip, Split, TrimLayer};
use mm_types::{AssetId, Id, RationalTime};
use mm_ui_layout::{EditKind, Side};

use crate::win::timebase;

/// 編集の対象にするカットの選択と、カットの ID の採番。
#[derive(Default)]
pub struct EditState {
    selected_cut: Mutex<Option<String>>,
    /// 選択を置き直すたびに進む。
    pub cut_serial: AtomicU64,
    next_id: AtomicU64,
    /// 直近の編集の結果の文言（失敗の理由、取り消したもの）。何も伝えることが無ければ空。
    message: Mutex<String>,
    /// 文言を置くたびに進む。同じ文言が続いても、置き直したことを WebView に伝える。
    pub message_serial: AtomicU64,
}

impl EditState {
    /// 直近の編集の結果の文言（失敗の理由、取り消したもの）。空なら伝えることが無い。
    pub fn message(&self) -> String {
        self.message.lock().expect("edit message").clone()
    }

    /// 直近の編集の結果を置く。空でなければ失敗の理由としてログにも残す。
    pub fn set_message(&self, text: String) {
        if !text.is_empty() {
            tracing::warn!(message = %text, "編集できなかった");
        }
        self.notify(text);
    }

    /// 失敗でない知らせ（取り消したものなど）を文言に置く。
    pub fn notify(&self, text: String) {
        *self.message.lock().expect("edit message") = text;
        self.message_serial.fetch_add(1, Ordering::SeqCst);
    }

    /// 選んだカットを置く。外すときは `None`。
    pub fn select_cut(&self, id: Option<String>) {
        let mut cur = self.selected_cut.lock().expect("selected cut");
        if *cur != id {
            *cur = id;
            self.cut_serial.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// 選んでいるカットの ID。
    pub fn selected_cut(&self) -> Option<String> {
        self.selected_cut.lock().expect("selected cut").clone()
    }

    /// プロジェクトに無いカットの ID。やり直しで同じ ID に戻るよう、呼び出し側が操作に入れる。
    pub fn fresh_cut_id(&self, project: &Project) -> CutId {
        loop {
            let n = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
            let id: CutId = Id::new(format!("k-{n}"));
            if project.cut(&id).is_err() {
                return id;
            }
        }
    }
}

/// 編集の入力。編集のキーを押した時点の状態。
pub struct Ctx<'a> {
    pub project: &'a Project,
    pub comp: &'a Composition,
    /// 選んでいるレイヤーの ID。
    pub layer: Option<&'a str>,
    /// 選んでいるカットの ID。
    pub cut: Option<&'a str>,
    /// 再生位置（秒）。
    pub playhead: f64,
}

/// Source で選んだ素材の範囲。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRange {
    pub asset: AssetId,
    pub in_point: RationalTime,
    pub out_point: RationalTime,
}

/// 計画した操作と、適用の後に選ぶカット。
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub op: Op,
    pub select: Option<CutId>,
}

/// 素材 `asset` のイン点・アウト点から範囲を決める。付いていない側は素材の端（イン点は 0、アウト点は素材の長さ）。
pub fn source_range(project: &Project, asset: &AssetId) -> Result<SourceRange, String> {
    let a = project.asset(asset).map_err(|e| e.to_string())?;
    let marks = project.bins.marks.iter().find(|m| &m.asset == asset);
    let in_point = marks.and_then(|m| m.in_point).unwrap_or(RationalTime::ZERO);
    let out_point = marks.and_then(|m| m.out_point).unwrap_or(a.duration);
    if in_point >= out_point {
        return Err(format!("イン点がアウト点以降: {in_point} >= {out_point}"));
    }
    Ok(SourceRange {
        asset: asset.clone(),
        in_point,
        out_point,
    })
}

fn is_sequence(layer: &Layer) -> bool {
    layer.source.kind == SEQUENCE_KIND
}

/// 編集の対象のシーケンスのレイヤー。選んだレイヤーがシーケンスならそれ、でなければ一番上のシーケンスのレイヤー。
fn target_layer<'a>(ctx: &Ctx<'a>) -> Option<&'a Layer> {
    let picked = ctx
        .layer
        .and_then(|id| ctx.comp.layers.iter().find(|l| l.id.as_str() == id))
        .filter(|l| is_sequence(l));
    picked.or_else(|| ctx.comp.layers.iter().rev().find(|l| is_sequence(l)))
}

struct Target<'a> {
    seq: &'a Sequence,
    placed: Vec<PlacedCut>,
    /// 再生位置のシーケンスの時刻。
    at: RationalTime,
}

fn target<'a>(ctx: &Ctx<'a>) -> Result<Target<'a>, String> {
    let layer = target_layer(ctx).ok_or("シーケンスのレイヤーが無い")?;
    let id = Id::new(layer.source.text_param(SEQUENCE_PARAM).unwrap_or(""));
    let seq = ctx.project.sequence(&id).map_err(|e| e.to_string())?;
    let placed = place_cuts(seq).map_err(|e| e.to_string())?;
    let here = timebase::frame_start(ctx.playhead, ctx.comp.fps)?;
    let at = timebase::layer_inner_time(layer, here)?;
    Ok(Target { seq, placed, at })
}

impl Target<'_> {
    /// 再生位置にあるカット。
    fn cut_at_playhead(&self) -> Option<&PlacedCut> {
        self.placed
            .iter()
            .find(|p| p.range.start <= self.at && self.at < p.range.end)
    }

    /// 再生位置に一番近いカットの編集点の、`seq.cuts` の中の位置。カットの前半にいれば前、後半にいれば後ろ。
    fn boundary_index(&self) -> usize {
        match self.cut_at_playhead() {
            Some(p) => {
                let len = p.range.end.to_f64() - p.range.start.to_f64();
                if self.at.to_f64() - p.range.start.to_f64() <= len / 2.0 {
                    p.index
                } else {
                    p.index + 1
                }
            }
            None if self.placed.iter().all(|p| p.range.start > self.at) => 0,
            None => self.seq.cuts.len(),
        }
    }

    /// 選んだカット、なければ再生位置のカット。このシーケンスのものだけ。
    fn working_cut(&self, selected: Option<&str>) -> Option<&Cut> {
        let picked = selected.and_then(|id| self.seq.cuts.iter().find(|c| c.id.as_str() == id));
        picked.or_else(|| {
            let p = self.cut_at_playhead()?;
            self.seq.cuts.get(p.index)
        })
    }
}

fn footage_cut(id: CutId, range: &SourceRange) -> Cut {
    Cut::new(
        id.to_string(),
        Node::footage(&range.asset),
        range.in_point,
        range.out_point,
    )
}

/// Source の範囲を、再生位置に一番近いカットの編集点へ差し込む（`insert`）。後ろのカットはずれる。
pub fn plan_insert(ctx: &Ctx<'_>, range: &SourceRange, id: CutId) -> Result<Planned, String> {
    let t = target(ctx)?;
    Ok(Planned {
        op: Insert {
            sequence: t.seq.id.clone(),
            index: t.boundary_index(),
            cut: footage_cut(id.clone(), range),
        }
        .into(),
        select: Some(id),
    })
}

/// Source の範囲で、選んだカット（なければ再生位置のカット）を置き換える（`overwrite`）。
pub fn plan_overwrite(ctx: &Ctx<'_>, range: &SourceRange, id: CutId) -> Result<Planned, String> {
    let t = target(ctx)?;
    let old = t
        .working_cut(ctx.cut)
        .ok_or("置き換えるカットが無い")?
        .id
        .clone();
    Ok(Planned {
        op: Overwrite {
            cut: old,
            replacement: footage_cut(id.clone(), range),
        }
        .into(),
        select: Some(id),
    })
}

/// 再生位置でカットを割る（`split`）。後ろの半分が `id`。カットの端では割れない。
pub fn plan_split(ctx: &Ctx<'_>, id: CutId) -> Result<Planned, String> {
    let t = target(ctx)?;
    let p = t.cut_at_playhead().ok_or("再生位置にカットが無い")?;
    let at = t.at.checked_sub(p.range.start).map_err(|e| e.to_string())?;
    if at <= RationalTime::ZERO {
        return Err("カットの先頭では割れない".into());
    }
    Ok(Planned {
        op: Split {
            cut: p.id.clone(),
            at,
            new_cut: id.clone(),
        }
        .into(),
        select: Some(id),
    })
}

/// 選んだカットを抜いて隙間を残す（`lift`）。
pub fn plan_lift(ctx: &Ctx<'_>) -> Result<Planned, String> {
    let cut = selected_cut_id(ctx)?;
    Ok(Planned {
        op: Lift { cut: cut.clone() }.into(),
        select: Some(cut),
    })
}

/// 選んだカットを抜いて後ろを詰める（`extract`）。
pub fn plan_extract(ctx: &Ctx<'_>) -> Result<Planned, String> {
    let cut = selected_cut_id(ctx)?;
    Ok(Planned {
        op: mm_ops::Extract { cut }.into(),
        select: None,
    })
}

fn selected_cut_id(ctx: &Ctx<'_>) -> Result<CutId, String> {
    let id = ctx.cut.ok_or("カットを選んでいない")?;
    let cut: CutId = Id::new(id);
    ctx.project.cut(&cut).map_err(|e| e.to_string())?;
    Ok(cut)
}

/// Timeline の添字と ID の対応。`DocSync` が作る。
pub struct TimelineIds<'a> {
    pub layers: &'a [String],
    pub cuts: &'a [Vec<String>],
}

fn edge_of(side: Side) -> Edge {
    match side {
        Side::In => Edge::In,
        Side::Out => Edge::Out,
    }
}

/// ドラッグの編集を操作にする。`by`（コンポジションの秒）は comp のフレームに丸め、0 フレームなら操作にしない。
/// trimLayer はコンポジションの時間軸のまま、カットの操作はレイヤーの `speed` を掛けたシーケンスの時間軸で渡す。
pub fn gesture_op(
    kind: EditKind,
    by: f64,
    ids: &TimelineIds<'_>,
    comp: &Composition,
) -> Option<Op> {
    let frames = (by * comp.fps.to_f64()).round() as i64;
    if frames == 0 {
        return None;
    }
    let by = RationalTime::from_frames(frames, comp.fps).ok()?;
    let cut_id = |item: usize, cut: usize| -> Option<CutId> {
        Some(Id::new(ids.cuts.get(item)?.get(cut)?.as_str()))
    };
    let item = match kind {
        EditKind::TrimLayer { item, .. }
        | EditKind::Ripple { item, .. }
        | EditKind::Roll { item, .. }
        | EditKind::Slip { item, .. }
        | EditKind::Slide { item, .. } => item,
    };
    let layer_id = ids.layers.get(item)?;
    let layer = comp.layers.iter().find(|l| l.id.as_str() == layer_id)?;
    if let EditKind::TrimLayer { side, .. } = kind {
        let edge = edge_of(side);
        return Some(
            TrimLayer {
                layer: layer.id.clone(),
                edge,
                by,
            }
            .into(),
        );
    }
    let by = timebase::layer_inner_delta(layer, by).ok()?;
    Some(match kind {
        EditKind::TrimLayer { .. } => return None,
        EditKind::Ripple { item, cut, side } => Ripple {
            cut: cut_id(item, cut)?,
            edge: edge_of(side),
            by,
        }
        .into(),
        EditKind::Roll { item, cut } => Roll {
            cut: cut_id(item, cut)?,
            by,
        }
        .into(),
        EditKind::Slip { item, cut } => Slip {
            cut: cut_id(item, cut)?,
            by,
        }
        .into(),
        EditKind::Slide { item, cut } => Slide {
            cut: cut_id(item, cut)?,
            by,
        }
        .into(),
    })
}

#[cfg(test)]
pub(crate) mod tests;
