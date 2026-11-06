//! Timeline のポインター操作。目盛りのスクラブ、レイヤーとカットの選択、端のつまみと中のドラッグ（スナップ）、拡大縮小と移動。
//!
//! 入力は Timeline の矩形の左上を (0, 0) とする座標で受け、操作の結果は [`Action`] で返す。
//! ドラッグから編集の操作と量を決める所は [`crate::gesture`]。操作の適用（`mm-ops`）はここの外で行う。

use crate::gesture::{body_edit, edge_edit, EditKind};
use crate::paint::PaintState;
use crate::palette::Palette;
use crate::snap::{quantize, snap, SNAP_PX};
use crate::time_view::TimeView;
use crate::timeline::{hit_test, Hit, Part, TimelineModel};

/// ホイール 1 目盛りあたりの拡大率。
const ZOOM_STEP: f64 = 1.15;
/// ホイールで横に動かす量（px）。
const PAN_STEP_PX: f32 = 60.0;

/// 押したときの修飾キー。ドラッグの種類を選ぶ（カットの端は `alt` で roll、中は `shift` で slide）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
}

/// ポインターの入力。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Down {
        x: f32,
        y: f32,
        mods: Mods,
    },
    Move {
        x: f32,
        y: f32,
    },
    Up,
    /// `dy` が正で奥へ（拡大や右への移動）。
    Wheel {
        x: f32,
        y: f32,
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
    /// シーケンスのカットの選択（レイヤーの添字、カットの添字）。外れたら `None`。
    SelectCut(Option<(usize, usize)>),
    /// 端のつまみや中のドラッグが表す編集。`by` はコンポジションの時間軸の秒（スナップ後。フレームへの丸めは受け手）。
    /// 確定するのは `done` のときだけで、それまでは案内線の表示に使う。
    Edit { kind: EditKind, by: f64, done: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    None,
    Scrub,
    /// 端のつまみ。`cut` が `None` ならレイヤーの端。
    Edge {
        item: usize,
        cut: Option<usize>,
        part: Part,
        mods: Mods,
    },
    /// カットの中。`from` はつかんだ時刻。
    Body {
        item: usize,
        cut: usize,
        mods: Mods,
        from: f64,
    },
}

/// Timeline の操作の状態。
#[derive(Debug, Clone)]
pub struct TimelineController {
    pub view: TimeView,
    pub hover: Hit,
    pub selected: Option<usize>,
    /// 選んだカット（レイヤーの添字、カットの添字）。
    pub selected_cut: Option<(usize, usize)>,
    guide: Option<f64>,
    /// ドラッグで最後に返した編集。離したときの確定にそのまま使う。
    last_edit: Option<(EditKind, f64)>,
    drag: Drag,
}

impl TimelineController {
    pub fn new(view: TimeView) -> Self {
        Self {
            view,
            hover: Hit::Empty,
            selected: None,
            selected_cut: None,
            guide: None,
            last_edit: None,
            drag: Drag::None,
        }
    }

    /// 並びが変わったモデルに合わせる。範囲の外の選択を外し、古い添字を指すホバーとドラッグを捨てる。
    pub fn model_changed(&mut self, model: &TimelineModel) {
        self.selected = self.selected.filter(|i| *i < model.items.len());
        self.selected_cut = self
            .selected_cut
            .filter(|(i, c)| model.items.get(*i).is_some_and(|it| *c < it.cuts.len()));
        self.hover = Hit::Empty;
        self.drag = Drag::None;
        self.guide = None;
        self.last_edit = None;
    }

    /// 描くときの状態。
    pub fn paint_state(&self, playhead: f64) -> PaintState {
        PaintState {
            playhead,
            hover: self.hover,
            selected: self.selected,
            selected_cut: self.selected_cut,
            guide: self.guide,
            palette: Palette::DEFAULT,
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
            PointerEvent::Down { x, y, mods } => self.down(x, y, mods, model),
            PointerEvent::Move { x, y } => self.moved(x, y, model, playhead),
            PointerEvent::Up => self.up(model),
            PointerEvent::Wheel { x, dy, ctrl, .. } => {
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

    fn down(&mut self, x: f32, y: f32, mods: Mods, model: &TimelineModel) -> Vec<Action> {
        let hit = hit_test(model, &self.view, x, y);
        if hit == Hit::Ruler {
            self.drag = Drag::Scrub;
            return vec![Action::Seek(self.seek_at(x, model))];
        }
        let had_cut = self.selected_cut.take();
        match hit {
            Hit::Layer { index, part } => {
                self.selected = Some(index);
                if part != Part::Body {
                    self.drag = Drag::Edge {
                        item: index,
                        cut: None,
                        part,
                        mods,
                    };
                }
                deselect_cut(Action::Select(Some(index)), had_cut)
            }
            Hit::Cut { item, cut, part } => {
                self.selected = Some(item);
                self.selected_cut = Some((item, cut));
                self.drag = match part {
                    Part::Body => Drag::Body {
                        item,
                        cut,
                        mods,
                        from: self.view.t_of(x),
                    },
                    _ => Drag::Edge {
                        item,
                        cut: Some(cut),
                        part,
                        mods,
                    },
                };
                vec![
                    Action::Select(Some(item)),
                    Action::SelectCut(self.selected_cut),
                ]
            }
            _ => {
                self.selected = None;
                deselect_cut(Action::Select(None), had_cut)
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
            Drag::Edge {
                item,
                cut,
                part,
                mods,
            } => {
                let mut targets = match cut {
                    Some(c) => model.cut_snap_targets(item, c),
                    None => model.snap_targets(Some(item)),
                };
                targets.push(playhead);
                let s = snap(self.view.t_of(x), &targets, self.view.px_per_sec, SNAP_PX);
                let time = if s.target.is_some() {
                    s.time
                } else {
                    quantize(s.time, model.fps)
                };
                self.guide = Some(time);
                self.report(edge_edit(model, item, cut, part, mods, time))
            }
            Drag::Body {
                item,
                cut,
                mods,
                from,
            } => {
                let dt = quantize(self.view.t_of(x) - from, model.fps);
                self.report(body_edit(model, item, cut, mods, dt))
            }
        }
    }

    fn report(&mut self, edit: Option<(EditKind, f64)>) -> Vec<Action> {
        self.last_edit = edit;
        edit.map(|(kind, by)| Action::Edit {
            kind,
            by,
            done: false,
        })
        .into_iter()
        .collect()
    }

    fn up(&mut self, model: &TimelineModel) -> Vec<Action> {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        self.guide = None;
        let last = self.last_edit.take();
        let frame = if model.fps > 0.0 {
            1.0 / model.fps
        } else {
            0.0
        };
        match (drag, last) {
            // 動かさずに離したクリックは編集にしない。
            (Drag::Edge { .. } | Drag::Body { .. }, Some((kind, by)))
                if by.abs() >= frame / 2.0 =>
            {
                vec![Action::Edit {
                    kind,
                    by,
                    done: true,
                }]
            }
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

/// レイヤーの選択の操作に、カットの選択が外れたことを添える。
fn deselect_cut(select: Action, had_cut: Option<(usize, usize)>) -> Vec<Action> {
    let mut out = vec![select];
    if had_cut.is_some() {
        out.push(Action::SelectCut(None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::Side;
    use crate::timeline::{row_top, CutBar, Item, ItemKind};

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

    /// 先頭がシーケンスのレイヤー（カット 3 つ: 0-3、3-4、4-7 秒）、次が普通のレイヤー。
    fn seq_model() -> TimelineModel {
        let cut = |start, end| CutBar {
            start,
            end,
            gap: false,
        };
        let mut seq = Item::layer(0.0, 20.0, ItemKind::Sequence, true);
        seq.cuts = vec![cut(0.0, 3.0), cut(3.0, 4.0), cut(4.0, 7.0)];
        TimelineModel {
            duration: 20.0,
            fps: 30.0,
            items: vec![seq, Item::layer(10.0, 14.0, ItemKind::Text, true)],
        }
    }

    fn ctl() -> TimelineController {
        TimelineController::new(TimeView {
            origin: 0.0,
            px_per_sec: 100.0,
        })
    }

    fn down(x: f32, y: f32) -> PointerEvent {
        PointerEvent::Down {
            x,
            y,
            mods: Mods::default(),
        }
    }

    #[test]
    fn 目盛りの上のドラッグは再生位置をフレームの境目で追う() {
        let mut c = ctl();
        let m = model();
        let a = c.handle(down(251.0, 5.0), &m, 0.0);
        assert_eq!(a, vec![Action::Seek(quantize(2.51, 30.0))]);
        let b = c.handle(PointerEvent::Move { x: 5000.0, y: 80.0 }, &m, 0.0);
        let Action::Seek(t) = b[0] else {
            panic!("{b:?}")
        };
        assert!(t < 10.0 && t > 9.9, "終わりの手前で止まる: {t}");
    }

    #[test]
    fn レイヤーのつまみのドラッグは別のレイヤーの端と再生位置に吸い付き_trim_layer_になる() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        // 先頭の帯の終わり(10 秒)ではなく、2 番目の帯の出つまみを掴む。
        let a = c.handle(down(399.0, y), &m, 0.0);
        assert_eq!(a, vec![Action::Select(Some(1))]);
        let b = c.handle(PointerEvent::Move { x: 697.0, y }, &m, 7.0);
        let Action::Edit { kind, by, done } = b[0] else {
            panic!("{b:?}")
        };
        assert_eq!(
            kind,
            EditKind::TrimLayer {
                item: 1,
                side: Side::Out
            }
        );
        assert!((by - 3.0).abs() < 1e-9, "出つまみの 4 秒から 7 秒へ: {by}");
        assert!(!done);
        let done = c.handle(PointerEvent::Up, &m, 7.0);
        assert!(matches!(done[0], Action::Edit { done: true, .. }));
    }

    #[test]
    fn スナップしないドラッグも離したときに確定する() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        c.handle(down(399.0, y), &m, 0.0);
        // どの目標からも遠い所（5.55 秒）で離す。
        let moved = c.handle(PointerEvent::Move { x: 555.0, y }, &m, 0.0);
        let Action::Edit { by, .. } = moved[0] else {
            panic!("{moved:?}")
        };
        let done = c.handle(PointerEvent::Up, &m, 0.0);
        assert_eq!(
            done,
            vec![Action::Edit {
                kind: EditKind::TrimLayer {
                    item: 1,
                    side: Side::Out
                },
                by,
                done: true
            }]
        );
        assert!(
            c.handle(PointerEvent::Up, &m, 0.0).is_empty(),
            "確定は 1 回だけ"
        );
    }

    #[test]
    fn 動かさずに離したクリックは編集にしない() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        c.handle(down(399.0, y), &m, 0.0);
        c.handle(PointerEvent::Move { x: 399.0, y }, &m, 0.0);
        assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
    }

    #[test]
    fn モデルの並びが変わったら範囲の外の選択とドラッグを捨てる() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        c.handle(down(399.0, y), &m, 0.0);
        assert_eq!(c.selected, Some(1));
        c.model_changed(&TimelineModel {
            items: vec![Item::layer(0.0, 10.0, ItemKind::Text, true)],
            ..m.clone()
        });
        assert_eq!(c.selected, None);
        assert!(
            c.handle(PointerEvent::Up, &m, 0.0).is_empty(),
            "古いドラッグは確定しない"
        );
    }

    #[test]
    fn 本体のクリックは選ぶだけ_空きで選択を外す() {
        let mut c = ctl();
        let m = model();
        let y = row_top(1) + 4.0;
        assert_eq!(
            c.handle(down(300.0, y), &m, 0.0),
            vec![Action::Select(Some(1))]
        );
        assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
        assert_eq!(
            c.handle(down(900.0, y), &m, 0.0),
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
                y: 0.0,
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
                y: 0.0,
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

    #[test]
    fn カットのクリックはレイヤーとカットの両方を選ぶ() {
        let mut c = ctl();
        let m = seq_model();
        let y = row_top(0) + 4.0;
        let a = c.handle(down(150.0, y), &m, 0.0);
        assert_eq!(
            a,
            vec![Action::Select(Some(0)), Action::SelectCut(Some((0, 0)))]
        );
        assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
        let b = c.handle(down(900.0, y), &m, 0.0);
        assert_eq!(
            b,
            vec![Action::Select(Some(0)), Action::SelectCut(None)],
            "カットの外はレイヤー"
        );
    }

    #[test]
    fn カットの出つまみのドラッグは_ripple_離したときに確定() {
        let mut c = ctl();
        let m = seq_model();
        let y = row_top(0) + 4.0;
        c.handle(down(298.0, y), &m, 0.0);
        let moved = c.handle(PointerEvent::Move { x: 250.0, y }, &m, 0.0);
        assert_eq!(
            moved,
            vec![Action::Edit {
                kind: EditKind::Ripple {
                    item: 0,
                    cut: 0,
                    side: Side::Out
                },
                by: -0.5,
                done: false
            }]
        );
        let done = c.handle(PointerEvent::Up, &m, 0.0);
        assert!(matches!(done[0], Action::Edit { done: true, by, .. } if (by + 0.5).abs() < 1e-9));
    }

    #[test]
    fn alt_を押してつかんだカットの端は_roll() {
        let mut c = ctl();
        let m = seq_model();
        let y = row_top(0) + 4.0;
        c.handle(
            PointerEvent::Down {
                x: 298.0,
                y,
                mods: Mods {
                    shift: false,
                    alt: true,
                },
            },
            &m,
            0.0,
        );
        let moved = c.handle(PointerEvent::Move { x: 270.0, y }, &m, 0.0);
        let Action::Edit { kind, by, .. } = moved[0] else {
            panic!("{moved:?}")
        };
        assert_eq!(kind, EditKind::Roll { item: 0, cut: 0 });
        assert!((by + 0.3).abs() < 1e-9, "{by}");
    }

    #[test]
    fn カットの中のドラッグは_slip_で_shift_なら_slide() {
        let m = seq_model();
        let y = row_top(0) + 4.0;
        let mut c = ctl();
        c.handle(down(100.0, y), &m, 0.0);
        let moved = c.handle(PointerEvent::Move { x: 140.0, y }, &m, 0.0);
        let Action::Edit { kind, by, .. } = moved[0] else {
            panic!("{moved:?}")
        };
        assert_eq!(kind, EditKind::Slip { item: 0, cut: 0 });
        assert!((by + 0.4).abs() < 1e-9, "{by}");
        c.handle(PointerEvent::Up, &m, 0.0);

        c.handle(
            PointerEvent::Down {
                x: 350.0,
                y,
                mods: Mods {
                    shift: true,
                    alt: false,
                },
            },
            &m,
            0.0,
        );
        let moved = c.handle(PointerEvent::Move { x: 380.0, y }, &m, 0.0);
        let Action::Edit { kind, by, .. } = moved[0] else {
            panic!("{moved:?}")
        };
        assert_eq!(kind, EditKind::Slide { item: 0, cut: 1 });
        assert!((by - 0.3).abs() < 1e-9, "{by}");
    }

    #[test]
    fn カットの端は他のカットの端に吸い付く() {
        let mut c = ctl();
        let m = seq_model();
        let y = row_top(0) + 4.0;
        // カット 0 の出つまみを 6.97 秒付近へ。カット 2 の出（7 秒）に吸い付く。
        c.handle(down(298.0, y), &m, 0.0);
        let moved = c.handle(PointerEvent::Move { x: 697.0, y }, &m, 0.0);
        let Action::Edit { by, .. } = moved[0] else {
            panic!("{moved:?}")
        };
        assert!((by - 4.0).abs() < 1e-9, "7 秒へ: {by}");
    }

    #[test]
    fn カットからレイヤーへ選び直すとカットの選択が外れる() {
        let mut c = ctl();
        let m = seq_model();
        c.handle(down(150.0, row_top(0) + 4.0), &m, 0.0);
        c.handle(PointerEvent::Up, &m, 0.0);
        let a = c.handle(down(1200.0, row_top(1) + 4.0), &m, 0.0);
        assert_eq!(a, vec![Action::Select(Some(1)), Action::SelectCut(None)]);
    }
}
