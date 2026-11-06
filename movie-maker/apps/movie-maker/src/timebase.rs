//! アプリの時間軸の変換。コンポジションの秒（再生位置）、シーケンスの時刻、素材の時刻の間の行き来をここに集め、
//! Program の表示、イン点・アウト点、split、ドラッグ、Story の移動先が同じ規則で数える。写像そのものは `mm-eval`（有理数）を使う。

use mm_doc::{Composition, Layer, Project, Sequence, FOOTAGE_KIND, FOOTAGE_PARAM};
use mm_doc::{SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_eval::{comp_time, place_cuts, source_time};
use mm_types::{Id, Rational, RationalTime};

/// 浮動小数の秒を有理数に直すときの分母。単語の時刻（ミリ秒）より十分細かい。
const SECS_DEN: u32 = 1_000_000;

/// 秒 `secs` に映るフレームの番号。Program の表示と同じく、フレームの途中は切り捨てる。
pub fn frame_index(secs: f64, fps: f64) -> i64 {
    (secs * fps).floor() as i64
}

/// 秒 `secs` に映るフレームの始まりの時刻。イン点・アウト点と split と再生位置の判定に使う。
pub fn frame_start(secs: f64, fps: Rational) -> Result<RationalTime, String> {
    RationalTime::from_frames(frame_index(secs, fps.to_f64()), fps).map_err(|e| e.to_string())
}

/// レイヤーのコンポジションの時刻 `t` を、そのレイヤーが映すシーケンス（や素材）の時刻にする。
pub fn layer_inner_time(layer: &Layer, t: RationalTime) -> Result<RationalTime, String> {
    source_time(layer, t).map_err(|e| e.to_string())
}

/// コンポジションの時間軸での長さ `by` を、レイヤーの中の時間軸の長さにする（`speed` 倍）。
/// ドラッグの量を、シーケンスの時間軸で働くカットの操作に渡すときに使う。
pub fn layer_inner_delta(layer: &Layer, by: RationalTime) -> Result<RationalTime, String> {
    by.scale(layer.speed).map_err(|e| e.to_string())
}

/// 浮動小数の秒を有理数にする（[`SECS_DEN`] 分の 1 秒に丸める）。
pub fn secs(t: f64) -> Result<RationalTime, String> {
    let num = (t * f64::from(SECS_DEN)).round() as i128;
    RationalTime::new(num, SECS_DEN).map_err(|e| e.to_string())
}

/// レイヤーのシーケンス。シーケンスのレイヤーでなければ `None`。
pub fn layer_sequence<'a>(project: &'a Project, layer: &Layer) -> Option<&'a Sequence> {
    if layer.source.kind != SEQUENCE_KIND {
        return None;
    }
    let id = Id::new(layer.source.text_param(SEQUENCE_PARAM)?);
    project.sequence(&id).ok()
}

/// レイヤーの中の時刻 `inner` が、レイヤーの `[in, out)` に入っているときのコンポジションの時刻。
fn shown_at(layer: &Layer, inner: RationalTime) -> Option<RationalTime> {
    let inside = layer.in_point <= inner && inner < layer.out_point;
    inside.then(|| comp_time(layer, inner).ok()).flatten()
}

/// 素材 `asset` の時刻 `at` が、コンポジション `comp` のどこかのレイヤーに映るときの再生位置（秒）。
/// 撮影素材のレイヤーと、シーケンスのレイヤーの有効なカットを見る。どこにも映らなければ `None`。
pub fn source_to_comp(project: &Project, comp: &Composition, asset: &str, at: f64) -> Option<f64> {
    let at = secs(at).ok()?;
    comp.layers
        .iter()
        .find_map(|layer| layer_source_to_comp(project, layer, asset, at))
        .map(RationalTime::to_f64)
}

fn layer_source_to_comp(
    project: &Project,
    layer: &Layer,
    asset: &str,
    at: RationalTime,
) -> Option<RationalTime> {
    if layer.source.kind == FOOTAGE_KIND {
        let own = layer.source.params.get(FOOTAGE_PARAM)?.value.as_asset()?;
        return (own.as_str() == asset)
            .then(|| shown_at(layer, at))
            .flatten();
    }
    let seq = layer_sequence(project, layer)?;
    place_cuts(seq).ok()?.iter().find_map(|p| {
        let cut = &seq.cuts[p.index];
        let own = cut.source.params.get(FOOTAGE_PARAM)?.value.as_asset()?;
        let inside = own.as_str() == asset && cut.in_point <= at && at < cut.out_point;
        let local = at.checked_sub(cut.in_point).ok()?.div_by(cut.speed).ok()?;
        let inner = p.range.start.checked_add(local).ok()?;
        inside.then(|| shown_at(layer, inner)).flatten()
    })
}

/// カット `cut` の始まりが映る再生位置（秒）。そのカットを持つシーケンスのレイヤーに映っていなければ `None`。
pub fn cut_to_comp(project: &Project, comp: &Composition, cut: &str) -> Option<f64> {
    comp.layers
        .iter()
        .find_map(|layer| {
            let seq = layer_sequence(project, layer)?;
            let placed = place_cuts(seq).ok()?;
            let p = placed.iter().find(|p| p.id.as_str() == cut)?;
            // カットの先頭がレイヤーの in より前に切られていても、映り始める所に寄せる。
            let inner = p.range.start.max(layer.in_point);
            (inner < p.range.end)
                .then(|| shown_at(layer, inner))
                .flatten()
        })
        .map(RationalTime::to_f64)
}

#[cfg(test)]
mod tests;
