//! Timeline のモデルと当たり判定。座標は Timeline の矩形の左上を (0, 0) とする。

use crate::time_view::TimeView;

/// 目盛りの帯の高さ。
pub const RULER_H: f32 = 26.0;
/// レイヤー 1 本の帯の高さ。
pub const ROW_H: f32 = 24.0;
/// 帯どうしの間隔。
pub const ROW_GAP: f32 = 3.0;
/// 端のつまみの幅の上限。
pub const HANDLE_PX: f32 = 7.0;
/// 帯の最小の幅。短いレイヤーも掴めるようにする。
const MIN_BAR_PX: f32 = 3.0;

/// レイヤーの種類（色分けに使う）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Footage,
    Image,
    Solid,
    Text,
    Composition,
    /// カットを並べたシーケンス。`Item::cuts` に中身が入る。
    Sequence,
    Other,
}

/// シーケンスのカット 1 つ（有効なものだけ）。コンポジションの時間軸での始まりと終わり（秒）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutBar {
    pub start: f64,
    pub end: f64,
    /// 隙間（`lift` で抜いた所）。空きとして描く。
    pub gap: bool,
}

/// Timeline に並ぶレイヤー 1 本。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// コンポジションの時間軸での始まりと終わり（秒）。
    pub start: f64,
    pub end: f64,
    pub kind: ItemKind,
    pub enabled: bool,
    /// シーケンスのレイヤーのカット。先頭から順。シーケンス以外は空。
    pub cuts: Vec<CutBar>,
}

impl Item {
    /// カットを持たない普通のレイヤー。
    pub fn layer(start: f64, end: f64, kind: ItemKind, enabled: bool) -> Self {
        Self {
            start,
            end,
            kind,
            enabled,
            cuts: Vec::new(),
        }
    }
}

/// Timeline に出すもの。`items` は上のレイヤーが先頭。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimelineModel {
    pub duration: f64,
    pub fps: f64,
    pub items: Vec<Item>,
}

impl TimelineModel {
    /// スナップの目標（0、終わり、各レイヤーとカットの端）。`skip` のレイヤーは除く。
    pub fn snap_targets(&self, skip: Option<usize>) -> Vec<f64> {
        self.targets_without(skip, None)
    }

    /// カット `(item, cut)` の端を動かすときのスナップの目標。そのカット自身の端は除く。
    pub fn cut_snap_targets(&self, item: usize, cut: usize) -> Vec<f64> {
        self.targets_without(None, Some((item, cut)))
    }

    fn targets_without(&self, skip: Option<usize>, skip_cut: Option<(usize, usize)>) -> Vec<f64> {
        let mut out = vec![0.0, self.duration];
        for (i, it) in self.items.iter().enumerate() {
            if Some(i) != skip {
                out.extend([it.start, it.end]);
            }
            if Some(i) == skip {
                continue;
            }
            for (c, bar) in it.cuts.iter().enumerate() {
                if Some((i, c)) != skip_cut {
                    out.extend([bar.start, bar.end]);
                }
            }
        }
        out
    }

    /// 全体の高さ（目盛りと全レイヤー）。
    pub fn content_height(&self) -> f32 {
        RULER_H + self.items.len() as f32 * (ROW_H + ROW_GAP)
    }
}

/// 帯の上の位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Body,
    InHandle,
    OutHandle,
}

/// 当たった所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Ruler,
    Layer {
        index: usize,
        part: Part,
    },
    /// シーケンスのカット。`item` はレイヤーの添字、`cut` はそのカットの添字。
    Cut {
        item: usize,
        cut: usize,
        part: Part,
    },
    Empty,
}

/// レイヤー `index` の帯の矩形の上端。
pub fn row_top(index: usize) -> f32 {
    RULER_H + ROW_GAP + index as f32 * (ROW_H + ROW_GAP)
}

/// 帯の左端と幅（x、w）。
pub fn bar_span(view: &TimeView, item: &Item) -> (f32, f32) {
    let x = view.x_of(item.start);
    let w = (view.x_of(item.end) - x).max(MIN_BAR_PX);
    (x, w)
}

/// カットの帯の左端と幅（x、w）。
pub fn cut_span(view: &TimeView, bar: &CutBar) -> (f32, f32) {
    let x = view.x_of(bar.start);
    let w = (view.x_of(bar.end) - x).max(MIN_BAR_PX);
    (x, w)
}

/// 幅 `bw`、左端 `bx` の帯の上の `x` が、どの部分か。つまみは帯の幅の 1/3 を超えない。
fn part_at(bx: f32, bw: f32, x: f32) -> Part {
    let handle = HANDLE_PX.min(bw / 3.0);
    if x < bx + handle {
        Part::InHandle
    } else if x >= bx + bw - handle {
        Part::OutHandle
    } else {
        Part::Body
    }
}

/// 点 (x, y) に当たるもの。つまみは帯の幅の 1/3 を超えない。
pub fn hit_test(model: &TimelineModel, view: &TimeView, x: f32, y: f32) -> Hit {
    if y < RULER_H {
        return Hit::Ruler;
    }
    for (index, item) in model.items.iter().enumerate() {
        let top = row_top(index);
        if y < top || y >= top + ROW_H {
            continue;
        }
        let (bx, bw) = bar_span(view, item);
        if x < bx || x >= bx + bw {
            return Hit::Empty;
        }
        if let Some(hit) = cut_hit(item, view, index, x) {
            return hit;
        }
        let part = if item.cuts.is_empty() {
            part_at(bx, bw, x)
        } else {
            Part::Body
        };
        return Hit::Layer { index, part };
    }
    Hit::Empty
}

/// シーケンスのレイヤーの上の `x` に当たるカット。
fn cut_hit(item: &Item, view: &TimeView, index: usize, x: f32) -> Option<Hit> {
    item.cuts.iter().enumerate().find_map(|(cut, bar)| {
        let (cx, cw) = cut_span(view, bar);
        (x >= cx && x < cx + cw).then(|| Hit::Cut {
            item: index,
            cut,
            part: part_at(cx, cw, x),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TimelineModel {
        TimelineModel {
            duration: 10.0,
            fps: 30.0,
            items: vec![
                Item::layer(0.0, 10.0, ItemKind::Text, true),
                Item::layer(2.0, 4.0, ItemKind::Footage, true),
            ],
        }
    }

    fn view() -> TimeView {
        TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        }
    }

    #[test]
    fn 目盛りの帯は_ruler() {
        assert_eq!(hit_test(&model(), &view(), 300.0, 10.0), Hit::Ruler);
    }

    #[test]
    fn 帯の左右の端はつまみ_中は本体() {
        let y = row_top(1) + 5.0;
        let at = |x| hit_test(&model(), &view(), x, y);
        assert_eq!(
            at(201.0),
            Hit::Layer {
                index: 1,
                part: Part::InHandle
            }
        );
        assert_eq!(
            at(300.0),
            Hit::Layer {
                index: 1,
                part: Part::Body
            }
        );
        assert_eq!(
            at(399.0),
            Hit::Layer {
                index: 1,
                part: Part::OutHandle
            }
        );
        assert_eq!(at(450.0), Hit::Empty);
    }

    #[test]
    fn 狭い帯ではつまみが幅の三分の一に収まる() {
        let mut m = model();
        m.items[1].end = 2.06; // 6 px
        let y = row_top(1) + 5.0;
        let mid = hit_test(&m, &view(), 203.0, y);
        assert!(matches!(
            mid,
            Hit::Layer {
                part: Part::Body,
                ..
            }
        ));
    }

    #[test]
    fn 帯と帯の隙間は何も無い() {
        let y = row_top(0) + ROW_H + 1.0;
        assert_eq!(hit_test(&model(), &view(), 300.0, y), Hit::Empty);
    }

    fn seq_model() -> TimelineModel {
        let cut = |start, end, gap| CutBar { start, end, gap };
        let mut seq = Item::layer(0.0, 10.0, ItemKind::Sequence, true);
        seq.cuts = vec![cut(0.0, 3.0, false), cut(3.0, 4.0, true)];
        TimelineModel {
            duration: 10.0,
            fps: 30.0,
            items: vec![seq],
        }
    }

    #[test]
    fn シーケンスの上ではカットとその端に当たり_カットの外はレイヤーの本体() {
        let m = seq_model();
        let y = row_top(0) + 5.0;
        let at = |x| hit_test(&m, &view(), x, y);
        let cut = |cut, part| Hit::Cut { item: 0, cut, part };
        assert_eq!(at(2.0), cut(0, Part::InHandle));
        assert_eq!(at(150.0), cut(0, Part::Body));
        assert_eq!(at(298.0), cut(0, Part::OutHandle));
        assert_eq!(at(350.0), cut(1, Part::Body), "隙間もカット");
        assert_eq!(
            at(700.0),
            Hit::Layer {
                index: 0,
                part: Part::Body
            }
        );
    }

    #[test]
    fn カットの端のスナップの目標は他のカットの端を含み_自分の端を除く() {
        let t = seq_model().cut_snap_targets(0, 0);
        assert!(t.contains(&4.0) && t.contains(&3.0), "次のカットの端");
        assert_eq!(t.iter().filter(|x| **x == 3.0).count(), 1, "自分の出は除く");
    }

    #[test]
    fn スナップの目標は自分の端を除く() {
        let t = model().snap_targets(Some(1));
        assert!(t.contains(&0.0) && t.contains(&10.0));
        assert!(!t.contains(&2.0) && !t.contains(&4.0));
    }
}
