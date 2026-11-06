//! Timeline のポインター操作。目盛りのスクラブ、レイヤーの選択、端のつまみのドラッグ（スナップ）、拡大縮小と移動。
//!
//! 入力は Timeline の矩形の左上を (0, 0) とする座標で受け、操作の結果は [`Action`] で返す。
//! 操作の適用（`mm-ops`）はここの外で行う。

use crate::paint::PaintState;
use crate::snap::{quantize, snap, SNAP_PX};
use crate::time_view::TimeView;
use crate::timeline::{hit_test, Hit, Part, TimelineModel};

/// ホイール 1 目盛りあたりの拡大率。
const ZOOM_STEP: f64 = 1.15;
/// ホイールで横に動かす量（px）。
const PAN_STEP_PX: f32 = 60.0;

/// ポインターの入力。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Down {
        x: f32,
        y: f32,
    },
    Move {
        x: f32,
        y: f32,
    },
    Up,
    /// `dy` が正で奥へ（拡大や右への移動）。
    Wheel {
        x: f32,
        dy: f32,
        ctrl: bool,
    },
    /// ポインターが Timeline から出た。
    Leave,
}

/// 操作の結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// 再生位置を動かす（秒。フレームの境目）。
    Seek(f64),
    /// レイヤーの選択が変わった。
    Select(Option<usize>),
    /// 端のつまみのドラッグ。`time` はスナップ後。`done` はボタンを離した。
    Trim {
        index: usize,
        part: Part,
        time: f64,
        done: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    None,
    Scrub,
    Trim { index: usize, part: Part },
}

/// Timeline の操作の状態。
#[derive(Debug, Clone)]
pub struct TimelineController {
    pub view: TimeView,
    pub hover: Hit,
    pub selected: Option<usize>,
    guide: Option<f64>,
    drag: Drag,
}

impl TimelineController {
    pub fn new(view: TimeView) -> Self {
        Self {
            view,
            hover: Hit::Empty,
            selected: None,
            guide: None,
            drag: Drag::None,
        }
    }

    /// 描くときの状態。
    pub fn paint_state(&self, playhead: f64) -> PaintState {
        PaintState {
            playhead,
            hover: self.hover,
            selected: self.selected,
            guide: self.guide,
        }
    }

    /// 入力を 1 つ受けて、起きた操作を返す。
    pub fn handle(
        &mut self,
        ev: PointerEvent,
        model: &TimelineModel,
        playhead: f64,
    ) -> Vec<Action> {
        match ev {
            PointerEvent::Down { x, y } => self.down(x, y, model),
            PointerEvent::Move { x, y } => self.moved(x, y, model, playhead),
            PointerEvent::Up => self.up(),
            PointerEvent::Wheel { x, dy, ctrl } => {
                self.wheel(x, dy, ctrl);
                Vec::new()
            }
            PointerEvent::Leave => {
                self.hover = Hit::Empty;
                Vec::new()
            }
        }
    }

    fn seek_at(&self, x: f32, model: &TimelineModel) -> f64 {
        let frame = if model.fps > 0.0 {
            1.0 / model.fps
        } else {
            0.0
        };
        let t = quantize(self.view.t_of(x), model.fps);
        t.clamp(0.0, (model.duration - frame).max(0.0))
    }

    fn down(&mut self, x: f32, y: f32, model: &TimelineModel) -> Vec<Action> {
        let hit = hit_test(model, &self.view, x, y);
        match hit {
            Hit::Ruler => {
                self.drag = Drag::Scrub;
                vec![Action::Seek(self.seek_at(x, model))]
            }
            Hit::Layer { index, part } => {
                self.selected = Some(index);
                if part != Part::Body {
                    self.drag = Drag::Trim { index, part };
                }
                vec![Action::Select(Some(index))]
            }
            Hit::Empty => {
                self.selected = None;
                vec![Action::Select(None)]
            }
        }
    }

    fn moved(&mut self, x: f32, y: f32, model: &TimelineModel, playhead: f64) -> Vec<Action> {
        match self.drag {
            Drag::None => {
                self.hover = hit_test(model, &self.view, x, y);
                Vec::new()
            }
            Drag::Scrub => vec![Action::Seek(self.seek_at(x, model))],
            Drag::Trim { index, part } => {
                let mut targets = model.snap_targets(Some(index));
                targets.push(playhead);
                let s = snap(self.view.t_of(x), &targets, self.view.px_per_sec, SNAP_PX);
                self.guide = s.target;
                vec![Action::Trim {
                    index,
                    part,
                    time: s.time,
                    done: false,
                }]
            }
        }
    }

    fn up(&mut self) -> Vec<Action> {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        let guide = self.guide.take();
        match (drag, guide) {
            (Drag::Trim { index, part }, Some(time)) => vec![Action::Trim {
                index,
                part,
                time,
                done: true,
            }],
            _ => Vec::new(),
        }
    }

    fn wheel(&mut self, x: f32, dy: f32, ctrl: bool) {
        if ctrl {
            self.view = self.view.zoom_at(x, ZOOM_STEP.powf(f64::from(dy)));
        } else {
            self.view = self.view.pan_px(-dy * PAN_STEP_PX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{row_top, Item, ItemKind};

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

    fn ctl() -> TimelineController {
        TimelineController::new(TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        })
    }

    #[test]
    fn 目盛りの上のドラッグは再生位置をフレームの境目で追う() {
        let mut c = ctl();
        let m = model();
        let a = c.handle(PointerEvent::Down { x: 251.0, y: 5.0 }, &m, 0.0);
        assert_eq!(a, vec![Action::Seek(quantize(2.51, 30.0))]);
        let b = c.handle(PointerEvent::Move { x: 5000.0, y: 80.0 }, &m, 0.0);
        let Action::Seek(t) = b[0] else {
            panic!("{b:?}")
        };
        assert!(t < 10.0 && t > 9.9, "終わりの手前で止まる: {t}");
    }

    #[test]
    fn つまみのドラッグは別のレイヤーの端と再生位置に吸い付く() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        // 先頭の帯の終わり(10 秒)ではなく、2 番目の帯の出つまみを掴む。
        let a = c.handle(PointerEvent::Down { x: 399.0, y }, &m, 0.0);
        assert_eq!(a, vec![Action::Select(Some(1))]);
        let b = c.handle(PointerEvent::Move { x: 697.0, y }, &m, 7.0);
        assert_eq!(
            b,
            vec![Action::Trim {
                index: 1,
                part: Part::OutHandle,
                time: 7.0,
                done: false
            }]
        );
        let done = c.handle(PointerEvent::Up, &m, 7.0);
        assert!(matches!(done[0], Action::Trim { done: true, .. }));
    }

    #[test]
    fn 本体のクリックは選ぶだけ_空きで選択を外す() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        assert_eq!(
            c.handle(PointerEvent::Down { x: 300.0, y }, &m, 0.0),
            vec![Action::Select(Some(1))]
        );
        assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
        assert_eq!(
            c.handle(PointerEvent::Down { x: 900.0, y }, &m, 0.0),
            vec![Action::Select(None)]
        );
    }

    #[test]
    fn ホイールは_ctrl_で拡大_なしで移動() {
        let mut c = ctl();
        let m = model();
        let t = c.view.t_of(300.0);
        c.handle(
            PointerEvent::Wheel {
                x: 300.0,
                dy: 2.0,
                ctrl: true,
            },
            &m,
            0.0,
        );
        assert!(c.view.px_per_sec > 100.0);
        assert!((c.view.t_of(300.0) - t).abs() < 1e-6);
        let origin = c.view.origin;
        c.handle(
            PointerEvent::Wheel {
                x: 300.0,
                dy: 1.0,
                ctrl: false,
            },
            &m,
            0.0,
        );
        assert!(c.view.origin > origin, "奥へ回すと先の時刻が見える");
    }

    #[test]
    fn ポインターが乗った所を覚え_出たら消す() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        c.handle(PointerEvent::Move { x: 300.0, y }, &m, 0.0);
        assert_eq!(
            c.hover,
            Hit::Layer {
                index: 1,
                part: Part::Body
            }
        );
        c.handle(PointerEvent::Leave, &m, 0.0);
        assert_eq!(c.hover, Hit::Empty);
    }
}
