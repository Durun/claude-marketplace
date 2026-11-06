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
    Other,
}

/// Timeline に並ぶレイヤー 1 本。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// コンポジションの時間軸での始まりと終わり（秒）。
    pub start: f64,
    pub end: f64,
    pub kind: ItemKind,
    pub enabled: bool,
}

/// Timeline に出すもの。`items` は上のレイヤーが先頭。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimelineModel {
    pub duration: f64,
    pub fps: f64,
    pub items: Vec<Item>,
}

impl TimelineModel {
    /// スナップの目標（0、終わり、各レイヤーの端）。
    pub fn snap_targets(&self, skip: Option<usize>) -> Vec<f64> {
        let mut out = vec![0.0, self.duration];
        for (i, it) in self.items.iter().enumerate() {
            if Some(i) != skip {
                out.extend([it.start, it.end]);
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
    Layer { index: usize, part: Part },
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
        let handle = HANDLE_PX.min(bw / 3.0);
        let part = if x < bx + handle {
            Part::InHandle
        } else if x >= bx + bw - handle {
            Part::OutHandle
        } else {
            Part::Body
        };
        return Hit::Layer { index, part };
    }
    Hit::Empty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TimelineModel {
        TimelineModel {
            duration: 10.0,
            fps: 30.0,
            items: vec![
                Item {
                    start: 0.0,
                    end: 10.0,
                    kind: ItemKind::Text,
                    enabled: true,
                },
                Item {
                    start: 2.0,
                    end: 4.0,
                    kind: ItemKind::Footage,
                    enabled: true,
                },
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

    #[test]
    fn スナップの目標は自分の端を除く() {
        let t = model().snap_targets(Some(1));
        assert!(t.contains(&0.0) && t.contains(&10.0));
        assert!(!t.contains(&2.0) && !t.contains(&4.0));
    }
}
