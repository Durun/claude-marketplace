//! シーケンスのカットの境のトランジションが、時刻のどこに掛かるか。
//!
//! トランジションは境を中心に前後へ半分ずつ掛け、境の前では前のカットを後ろへ、境の後では次のカットを前へ延ばす。
//! 半分の長さは次のどれも超えない。
//! - トランジションの長さの半分。
//! - 両側のカットの長さ。カットの両端にトランジションがあるときは、その半分（隣の境の掛かりと重ならない）。
//! - 延ばす側の素材の余白（ハンドル）。出ていくカットは出の後ろ、入ってくるカットはイン点の前の素材の長さ。

use mm_doc::{Cut, Project, Sequence, Transition, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_eval::{node_params_at, place_cuts, PlacedCut};
use mm_types::{Id, ParamValue, RationalTime, Result};

use crate::layers::{NESTED_KIND, NESTED_PARAM};

/// 出ていくカットを逆向きに描く倍率のパラメータの名前（`transition.*` の規約）。
const REWIND_PARAM: &str = "rewind";

/// 境のトランジションの掛かり方。番号は配置したカットの並びの中の位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Crossfade {
    pub from: usize,
    pub to: usize,
    /// 0 で出ていくカットだけ、1 で入ってくるカットだけ。
    pub progress: f32,
    /// 掛かり始めの時刻（シーケンスの時間軸の秒）。
    pub start: f64,
    /// 掛かる長さ（秒）。トランジションの長さを両側のカットと余白で縮めたもの。
    pub length: f64,
}

/// カットの素材の余白（シーケンスの時間軸の秒）。素材の長さに限りが無い種類（単色、シェーダー）は無限。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Handles {
    /// イン点の前にある素材の長さ。
    pub head: f64,
    /// 出の後ろにある素材の長さ。
    pub tail: f64,
}

impl Handles {
    pub const UNLIMITED: Self = Self {
        head: f64::INFINITY,
        tail: f64::INFINITY,
    };
}

/// 時刻 `t` が `placed[pos]` の中にあるとき、境のトランジションに掛かっていればその掛かり方を返す。
/// `handles` はカットの素材の余白を返す。
pub(crate) fn crossfade_at(
    seq: &Sequence,
    placed: &[PlacedCut],
    pos: usize,
    t: RationalTime,
    handles: &dyn Fn(&Cut) -> Handles,
) -> Result<Option<Crossfade>> {
    let tf = t.to_f64();
    if pos > 0 {
        if let Some(half) = half_at(seq, placed, pos, handles)? {
            let boundary = placed[pos].range.start.to_f64();
            if tf - boundary < half {
                return Ok(Some(crossfade(pos - 1, pos, tf, boundary, half)));
            }
        }
    }
    if pos + 1 < placed.len() {
        if let Some(half) = half_at(seq, placed, pos + 1, handles)? {
            let boundary = placed[pos + 1].range.start.to_f64();
            if boundary - tf <= half {
                return Ok(Some(crossfade(pos, pos + 1, tf, boundary, half)));
            }
        }
    }
    Ok(None)
}

fn crossfade(from: usize, to: usize, t: f64, boundary: f64, half: f64) -> Crossfade {
    let start = boundary - half;
    Crossfade {
        from,
        to,
        progress: ((t - start) / (2.0 * half)).clamp(0.0, 1.0) as f32,
        start,
        length: 2.0 * half,
    }
}

/// トランジションが出ていくカットを逆向きに描くか（種類のパラメータ `rewind` が正）。逆向きなら出の後ろの余白は使わない。
pub(crate) fn rewinds(tr: &Transition) -> bool {
    rewind_rate(tr) > 0.0
}

/// 出ていくカットを逆向きに描く倍率。0 なら普通に描く。
pub(crate) fn rewind_rate(tr: &Transition) -> f64 {
    node_params_at(&tr.node, RationalTime::ZERO)
        .number_or(REWIND_PARAM, 0.0)
        .max(0.0)
}

/// `placed[to - 1]` と `placed[to]` の境のトランジションの半分の長さ。トランジションが無いか、0 なら `None`。
fn half_at(
    seq: &Sequence,
    placed: &[PlacedCut],
    to: usize,
    handles: &dyn Fn(&Cut) -> Handles,
) -> Result<Option<f64>> {
    let Some(tr) = &seq.cuts[placed[to].index].transition_in else {
        return Ok(None);
    };
    let (before, after) = (&placed[to - 1], &placed[to]);
    let has_transition = |pos: usize| {
        pos > 0
            && placed
                .get(pos)
                .is_some_and(|p| seq.cuts[p.index].transition_in.is_some())
    };
    // 隣の境にもトランジションがあるカットは、長さの半分ずつを両端で分ける。
    let room = |p: &PlacedCut, shared: bool| -> Result<f64> {
        let len = p.range.duration()?.to_f64();
        Ok(if shared { len / 2.0 } else { len })
    };
    let tail = if rewinds(tr) {
        f64::INFINITY
    } else {
        handles(&seq.cuts[before.index]).tail
    };
    let half = (tr.duration.to_f64() / 2.0)
        .min(room(before, has_transition(to - 1))?)
        .min(room(after, has_transition(to + 1))?)
        .min(tail)
        .min(handles(&seq.cuts[after.index]).head);
    Ok((half > 0.0).then_some(half))
}

/// カットの素材の余白を、プロジェクトのデータから引く。素材（`AssetRef`）は素材の長さ、入れ子は
/// コンポジションの長さ、シーケンスは配置したカットの全体の長さが素材の長さ。引けないときは余白なし。
pub(crate) fn handles_in(project: &Project, cut: &Cut) -> Handles {
    let Some(length) = source_length(project, cut) else {
        return Handles::UNLIMITED;
    };
    let speed = cut.speed.to_f64();
    let (Some(length), true) = (length, speed > 0.0) else {
        return Handles {
            head: 0.0,
            tail: 0.0,
        };
    };
    Handles {
        head: (cut.in_point.to_f64() / speed).max(0.0),
        tail: ((length - cut.out_point.to_f64()) / speed).max(0.0),
    }
}

/// カットの素材の長さ（秒）。限りの無い種類は `None`、引けない素材は `Some(None)`。
fn source_length(project: &Project, cut: &Cut) -> Option<Option<f64>> {
    let params = node_params_at(&cut.source, cut.in_point);
    let text = |name: &str| params.get(name).and_then(ParamValue::as_text);
    if let Some(asset) = params.0.values().find_map(ParamValue::as_asset) {
        return Some(project.asset(asset).ok().map(|a| a.duration.to_f64()));
    }
    match cut.source.kind.as_str() {
        NESTED_KIND => Some(
            text(NESTED_PARAM)
                .and_then(|name| project.composition(&Id::new(name)).ok())
                .map(|c| c.duration.to_f64()),
        ),
        SEQUENCE_KIND => Some(
            text(SEQUENCE_PARAM)
                .and_then(|name| project.sequence(&Id::new(name)).ok())
                .and_then(|s| place_cuts(s).ok())
                .and_then(|p| p.last().map(|c| c.range.end.to_f64())),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
