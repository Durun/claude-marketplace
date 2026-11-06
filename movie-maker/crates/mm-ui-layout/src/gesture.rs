//! ドラッグから編集の操作と量を決める純関数。どの操作を出すかは、つかんだ場所（つまみか中か）と修飾キーで選ぶ。
//!
//! 量 `by` はコンポジションの時間軸の秒で、符号は操作の定義に合わせる（`mm-ops` の `ripple`、`roll`、`slip`、`slide`、`trimLayer`）。
//! 1 フレームより短くなる量は、結果が空にならないように収める。

use crate::controller::Mods;
use crate::timeline::{Part, TimelineModel};

/// 動かす端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    In,
    Out,
}

/// ドラッグが表す編集。`item` はレイヤー、`cut` はそのシーケンスのカットの添字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// レイヤーの端（`trimLayer`）。
    TrimLayer { item: usize, side: Side },
    /// カットの端。後ろのカットがずれる（`ripple`）。
    Ripple { item: usize, cut: usize, side: Side },
    /// `cut` とすぐ後ろのカットの境目（`roll`）。
    Roll { item: usize, cut: usize },
    /// カットの中身をずらす（`slip`）。
    Slip { item: usize, cut: usize },
    /// カットを前後のカットの中で動かす（`slide`）。
    Slide { item: usize, cut: usize },
}

fn frame_secs(model: &TimelineModel) -> f64 {
    if model.fps > 0.0 {
        1.0 / model.fps
    } else {
        0.0
    }
}

/// 端のつまみを時刻 `t`（スナップ後）へ動かすときの編集と量。
/// `cut` が `None` ならレイヤーの端。`Alt` を押していると、カットの端は境目を動かす `roll` になる。
pub fn edge_edit(
    model: &TimelineModel,
    item: usize,
    cut: Option<usize>,
    part: Part,
    mods: Mods,
    t: f64,
) -> Option<(EditKind, f64)> {
    let it = model.items.get(item)?;
    let frame = frame_secs(model);
    let side = match part {
        Part::InHandle => Side::In,
        Part::OutHandle => Side::Out,
        Part::Body => return None,
    };
    let Some(cut) = cut else {
        let (edge, len) = (
            if side == Side::In { it.start } else { it.end },
            it.end - it.start,
        );
        return Some((
            EditKind::TrimLayer { item, side },
            shrink_limit(t - edge, side, len, frame),
        ));
    };
    let bar = *it.cuts.get(cut)?;
    let len = bar.end - bar.start;
    if !mods.alt {
        let edge = if side == Side::In { bar.start } else { bar.end };
        return Some((
            EditKind::Ripple { item, cut, side },
            shrink_limit(t - edge, side, len, frame),
        ));
    }
    // 境目の前のカットと後ろのカットの長さの内側に収める。間に無効なカットがあれば相手が違うので出さない。
    let (before, after, kind_cut) = match side {
        Side::Out if bar.disabled_after => return None,
        Side::In if bar.disabled_before => return None,
        Side::Out => (len, it.cuts.get(cut + 1)?, cut),
        Side::In => {
            let prev = it.cuts.get(cut.checked_sub(1)?)?;
            (prev.end - prev.start, &bar, cut - 1)
        }
    };
    let boundary = if side == Side::Out {
        bar.end
    } else {
        bar.start
    };
    let by = (t - boundary).clamp(
        -(before - frame).max(0.0),
        (after.end - after.start - frame).max(0.0),
    );
    Some((
        EditKind::Roll {
            item,
            cut: kind_cut,
        },
        by,
    ))
}

/// 端を縮める向きに 1 フレーム分は残す。イン点は右へ、アウト点は左へ動かすと縮む。
fn shrink_limit(by: f64, side: Side, len: f64, frame: f64) -> f64 {
    let room = (len - frame).max(0.0);
    match side {
        Side::In => by.min(room),
        Side::Out => by.max(-room),
    }
}

/// カットの中のドラッグ。`dt` はつかんだ時刻からの動き。`Shift` を押していると `slide`、そうでなければ `slip`。
/// `slip` は中身がポインターについてくる向き（右へ引くと、より前の素材が見える）にする。
pub fn body_edit(
    model: &TimelineModel,
    item: usize,
    cut: usize,
    mods: Mods,
    dt: f64,
) -> Option<(EditKind, f64)> {
    let it = model.items.get(item)?;
    let bar = it.cuts.get(cut)?;
    if !mods.shift {
        return (!bar.gap).then_some((EditKind::Slip { item, cut }, -dt));
    }
    if bar.disabled_before || bar.disabled_after {
        return None;
    }
    let frame = frame_secs(model);
    let prev = it.cuts.get(cut.checked_sub(1)?)?;
    let next = it.cuts.get(cut + 1)?;
    let by = dt.clamp(
        -(prev.end - prev.start - frame).max(0.0),
        (next.end - next.start - frame).max(0.0),
    );
    Some((EditKind::Slide { item, cut }, by))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{CutBar, Item, ItemKind};

    fn bar(start: f64, end: f64) -> CutBar {
        CutBar {
            start,
            end,
            gap: false,
            ..CutBar::default()
        }
    }

    fn model() -> TimelineModel {
        let mut seq = Item::layer(0.0, 20.0, ItemKind::Sequence, true);
        seq.cuts = vec![bar(0.0, 3.0), bar(3.0, 4.0), bar(4.0, 7.0)];
        TimelineModel {
            duration: 20.0,
            fps: 30.0,
            items: vec![seq, Item::layer(1.0, 5.0, ItemKind::Text, true)],
        }
    }

    const NONE: Mods = Mods {
        shift: false,
        alt: false,
    };
    const ALT: Mods = Mods {
        shift: false,
        alt: true,
    };
    const SHIFT: Mods = Mods {
        shift: true,
        alt: false,
    };

    #[test]
    fn カットの出つまみは_ripple_の出_動かした分が量() {
        let (kind, by) = edge_edit(&model(), 0, Some(0), Part::OutHandle, NONE, 3.5).unwrap();
        assert_eq!(
            kind,
            EditKind::Ripple {
                item: 0,
                cut: 0,
                side: Side::Out
            }
        );
        assert!((by - 0.5).abs() < 1e-9);
    }

    #[test]
    fn カットの入つまみは_ripple_の入_右へ引くと短くなる量が正() {
        let (kind, by) = edge_edit(&model(), 0, Some(2), Part::InHandle, NONE, 4.5).unwrap();
        assert_eq!(
            kind,
            EditKind::Ripple {
                item: 0,
                cut: 2,
                side: Side::In
            }
        );
        assert!((by - 0.5).abs() < 1e-9);
    }

    #[test]
    fn ripple_は_1_フレームより短くしない() {
        let (_, by) = edge_edit(&model(), 0, Some(1), Part::OutHandle, NONE, 0.0).unwrap();
        assert!((by + (1.0 - 1.0 / 30.0)).abs() < 1e-9, "{by}");
    }

    #[test]
    fn alt_でカットの出つまみは_roll_次のカットとの境目を動かす() {
        let (kind, by) = edge_edit(&model(), 0, Some(0), Part::OutHandle, ALT, 2.5).unwrap();
        assert_eq!(kind, EditKind::Roll { item: 0, cut: 0 });
        assert!((by + 0.5).abs() < 1e-9);
    }

    #[test]
    fn alt_でカットの入つまみは_前のカットとの境目の_roll() {
        let (kind, by) = edge_edit(&model(), 0, Some(1), Part::InHandle, ALT, 3.25).unwrap();
        assert_eq!(kind, EditKind::Roll { item: 0, cut: 0 });
        assert!((by - 0.25).abs() < 1e-9);
    }

    #[test]
    fn roll_は境目の前後のカットを_1_フレーム残す() {
        let (_, by) = edge_edit(&model(), 0, Some(1), Part::OutHandle, ALT, 100.0).unwrap();
        assert!((by - (3.0 - 1.0 / 30.0)).abs() < 1e-9, "{by}");
    }

    #[test]
    fn 端のカットには_roll_の相手が無い() {
        assert!(edge_edit(&model(), 0, Some(2), Part::OutHandle, ALT, 7.5).is_none());
        assert!(edge_edit(&model(), 0, Some(0), Part::InHandle, ALT, 0.5).is_none());
    }

    #[test]
    fn 普通のレイヤーの端は_trim_layer() {
        let (kind, by) = edge_edit(&model(), 1, None, Part::InHandle, NONE, 1.5).unwrap();
        assert_eq!(
            kind,
            EditKind::TrimLayer {
                item: 1,
                side: Side::In
            }
        );
        assert!((by - 0.5).abs() < 1e-9);
        let (_, by) = edge_edit(&model(), 1, None, Part::OutHandle, NONE, 4.0).unwrap();
        assert!((by + 1.0).abs() < 1e-9);
    }

    #[test]
    fn カットの中のドラッグは_slip_で中身が引いた向きに動く() {
        let (kind, by) = body_edit(&model(), 0, 0, NONE, 0.5).unwrap();
        assert_eq!(kind, EditKind::Slip { item: 0, cut: 0 });
        assert!((by + 0.5).abs() < 1e-9);
    }

    #[test]
    fn shift_で_slide_前後のカットの長さに収まる() {
        let (kind, by) = body_edit(&model(), 0, 1, SHIFT, 0.5).unwrap();
        assert_eq!(kind, EditKind::Slide { item: 0, cut: 1 });
        assert!((by - 0.5).abs() < 1e-9);
        let (_, by) = body_edit(&model(), 0, 1, SHIFT, 99.0).unwrap();
        assert!((by - (3.0 - 1.0 / 30.0)).abs() < 1e-9);
        assert!(body_edit(&model(), 0, 0, SHIFT, 0.5).is_none(), "前が無い");
    }

    #[test]
    fn 隙間は_slip_しない() {
        let mut m = model();
        m.items[0].cuts[1].gap = true;
        assert!(body_edit(&m, 0, 1, NONE, 0.5).is_none());
    }
}
