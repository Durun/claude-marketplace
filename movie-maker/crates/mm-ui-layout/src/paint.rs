//! Timeline を矩形の並びに落とす。座標は Timeline の矩形の左上を (0, 0) とする。

use crate::digits::{text_quads, text_width};
use crate::geom::{Quad, Rect};
use crate::palette::{self, Palette};
use crate::ruler::{label, major_step, ticks};
use crate::time_view::TimeView;
use crate::timeline::{bar_span, row_top, Hit, Part, TimelineModel, HANDLE_PX, ROW_H, RULER_H};

/// 目盛りのラベルの文字の高さ。
const LABEL_H: f32 = 10.0;
/// 再生位置の線の幅。
const PLAYHEAD_W: f32 = 2.0;
/// 再生位置の頭の一辺。
const PLAYHEAD_CAP: f32 = 10.0;

/// 描く時点の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintState {
    /// 再生位置（秒）。
    pub playhead: f64,
    /// ポインターが乗っている所。
    pub hover: Hit,
    /// 選ばれたレイヤー。
    pub selected: Option<usize>,
    /// スナップの案内線（秒）。
    pub guide: Option<f64>,
    /// 色。
    pub palette: Palette,
}

/// Timeline の矩形の大きさ `(w, h)` の中に描く矩形の並び。
pub fn timeline_quads(
    model: &TimelineModel,
    view: &TimeView,
    size: (f32, f32),
    state: &PaintState,
) -> Vec<Quad> {
    let (w, h) = size;
    let p = &state.palette;
    let mut q = vec![Quad::new(Rect::new(0.0, 0.0, w, h), p.panel)];
    rows(model, view, w, state, &mut q);
    ruler(view, w, p, &mut q);
    if let Some(g) = state.guide {
        q.push(vline(view.x_of(g), h, 1.0, p.guide));
    }
    playhead(view.x_of(state.playhead), h, p, &mut q);
    q
}

fn vline(x: f32, h: f32, width: f32, color: crate::geom::Color) -> Quad {
    Quad::new(Rect::new(x - width / 2.0, 0.0, width, h), color)
}

fn ruler(view: &TimeView, w: f32, p: &Palette, q: &mut Vec<Quad>) {
    q.push(Quad::new(Rect::new(0.0, 0.0, w, RULER_H), p.ruler));
    let step = major_step(view.px_per_sec);
    for t in ticks(view, w) {
        let len = if t.major { 9.0 } else { 4.0 };
        q.push(Quad::new(Rect::new(t.x, RULER_H - len, 1.0, len), p.tick));
        if t.major {
            let text = label(t.time, step);
            let tw = text_width(&text, LABEL_H);
            let x = (t.x + 4.0).min(w - tw);
            q.extend(text_quads(&text, x, 4.0, LABEL_H, p.label));
        }
    }
}

fn rows(model: &TimelineModel, view: &TimeView, w: f32, state: &PaintState, q: &mut Vec<Quad>) {
    let p = &state.palette;
    for (i, item) in model.items.iter().enumerate() {
        let top = row_top(i);
        let stripe = if i % 2 == 0 { p.row_even } else { p.row_odd };
        q.push(Quad::new(Rect::new(0.0, top - 1.0, w, ROW_H + 2.0), stripe));
        let (x, bw) = bar_span(view, item);
        let mut color = palette::item_color(item.kind);
        if !item.enabled {
            color = color.with_alpha(0.35);
        }
        let hovered = matches!(state.hover, Hit::Layer { index, .. } if index == i);
        if hovered {
            color = palette::lighten(color, 0.18);
        }
        let bar = Rect::new(x, top, bw, ROW_H);
        q.push(Quad::new(bar, color));
        handles(&bar, (state.hover, i), p, q);
        if state.selected == Some(i) {
            outline(&bar, p, q);
        }
    }
    outside(model, view, w, p, q);
}

fn handles(bar: &Rect, (hover, index): (Hit, usize), p: &Palette, q: &mut Vec<Quad>) {
    let hw = HANDLE_PX.min(bar.w / 3.0);
    let lit = |part| hover == Hit::Layer { index, part };
    for (part, x) in [(Part::InHandle, bar.x), (Part::OutHandle, bar.right() - hw)] {
        if lit(part) {
            q.push(Quad::new(
                Rect::new(x, bar.y, hw, bar.h),
                p.selection.with_alpha(0.85),
            ));
        } else {
            q.push(Quad::new(
                Rect::new(x, bar.y, hw, bar.h),
                p.selection.with_alpha(0.14),
            ));
        }
    }
}

fn outline(r: &Rect, p: &Palette, q: &mut Vec<Quad>) {
    let c = p.selection;
    q.push(Quad::new(Rect::new(r.x, r.y, r.w, 1.0), c));
    q.push(Quad::new(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), c));
    q.push(Quad::new(Rect::new(r.x, r.y, 1.0, r.h), c));
    q.push(Quad::new(Rect::new(r.right() - 1.0, r.y, 1.0, r.h), c));
}

/// コンポジションの終わりより右を暗くする。
fn outside(model: &TimelineModel, view: &TimeView, w: f32, p: &Palette, q: &mut Vec<Quad>) {
    let end = view.x_of(model.duration);
    if end < w {
        let x = end.max(0.0);
        q.push(Quad::new(
            Rect::new(x, RULER_H, w - x, model.content_height() - RULER_H),
            p.outside,
        ));
    }
}

fn playhead(x: f32, h: f32, p: &Palette, q: &mut Vec<Quad>) {
    q.push(vline(x, h, PLAYHEAD_W, p.playhead));
    q.push(Quad::new(
        Rect::new(
            x - PLAYHEAD_CAP / 2.0,
            0.0,
            PLAYHEAD_CAP,
            PLAYHEAD_CAP * 0.8,
        ),
        p.playhead,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{Item, ItemKind};

    fn model() -> TimelineModel {
        TimelineModel {
            duration: 5.0,
            fps: 30.0,
            items: vec![Item {
                start: 1.0,
                end: 3.0,
                kind: ItemKind::Footage,
                enabled: true,
            }],
        }
    }

    fn state(playhead: f64) -> PaintState {
        PaintState {
            playhead,
            hover: Hit::Empty,
            selected: None,
            guide: None,
            palette: Palette::DEFAULT,
        }
    }

    #[test]
    fn テーマの色で面と帯の段を塗る() {
        let v = TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        };
        let surface = palette::parse_hex("#123456").unwrap();
        let themed = Palette::themed(|name| (name == "surface").then_some(surface));
        let st = PaintState {
            palette: themed,
            ..state(0.0)
        };
        let quads = timeline_quads(&model(), &v, (800.0, 120.0), &st);
        assert_eq!(quads[0].color, surface, "面");
        assert_eq!(
            themed.playhead,
            palette::PLAYHEAD,
            "テーマに無い色は既定のまま"
        );
    }

    #[test]
    fn 再生位置の線は時刻の_x_に立つ() {
        let v = TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        };
        let quads = timeline_quads(&model(), &v, (800.0, 120.0), &state(2.0));
        let line = quads
            .iter()
            .find(|q| q.color == palette::PLAYHEAD && q.rect.h == 120.0)
            .expect("再生位置の線");
        assert!((line.rect.x + line.rect.w / 2.0 - 200.0).abs() < 1e-3);
    }

    #[test]
    fn 帯は始まりから終わりの幅で描く() {
        let v = TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        };
        let quads = timeline_quads(&model(), &v, (800.0, 120.0), &state(0.0));
        let color = palette::item_color(ItemKind::Footage);
        let bar = quads
            .iter()
            .find(|q| q.color == color)
            .expect("レイヤーの帯");
        assert_eq!((bar.rect.x, bar.rect.w), (100.0, 200.0));
    }

    #[test]
    fn 終わりより右は暗くする() {
        let v = TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        };
        let quads = timeline_quads(&model(), &v, (800.0, 120.0), &state(0.0));
        assert!(quads
            .iter()
            .any(|q| q.color == palette::OUTSIDE && q.rect.x == 500.0));
    }
}
