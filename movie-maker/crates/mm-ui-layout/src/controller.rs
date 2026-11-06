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
    /// 端のつまみや中のドラッグが表す編集。`by` はコンポジションの時間軸の秒（スナップ後。フレームへの丸めは受け取る側）。
    /// 確定するのは `done` のときだけで、それまではドラッグ中の表示（スナップのガイド線）に使う。
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
mod tests;
