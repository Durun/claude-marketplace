//! wgpu のサーフェス上の UI の計算。座標は物理ピクセル、時刻は秒（f64）。
//!
//! 純関数だけを持ち、GPU にも他の `mm-*` にも依存しない（`cargo test` が数秒で回る）。
//!
//! 表示の範囲は [`TimeView`]（時刻と x の変換、拡大縮小、移動）、目盛りは [`ticks`]、
//! スナップとフレームへの丸めは [`snap`] と [`quantize`]、帯と端のつまみの当たり判定は [`hit_test`]、
//! ポインター操作（スクラブ、選択、つまみのドラッグ）は [`TimelineController`] が受け持つ。
//! [`timeline_quads`] は Timeline を [`Quad`] の並びに落とし、描画は `mm-ui-wgpu` が行う。

mod controller;
mod digits;
mod geom;
mod gesture;
mod paint;
pub mod palette;
mod ruler;
mod screen;
mod snap;
mod time_view;
mod timeline;

pub use controller::{Action, Mods, PointerEvent, TimelineController};
pub use digits::{text_quads, text_width};
pub use geom::{fit_rect, Color, Quad, Rect};
pub use gesture::{body_edit, edge_edit, EditKind, Side};
pub use paint::{timeline_quads, PaintState};
pub use ruler::{label, major_step, ticks, Tick};
pub use screen::{split_surface, split_viewers, SurfaceLayout, VIEW_MARGIN};
pub use snap::{quantize, snap, Snapped, SNAP_PX};
pub use time_view::{TimeView, MAX_PX_PER_SEC, MIN_PX_PER_SEC};
pub use timeline::{
    bar_span, cut_span, hit_test, row_top, CutBar, Hit, Item, ItemKind, Part, TimelineModel,
    HANDLE_PX, ROW_GAP, ROW_H, RULER_H,
};
