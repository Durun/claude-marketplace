//! 段階 0 の縦の 1 本に要る動画オブジェクトの種類。
//!
//! - `footage`: 撮影素材。描画器が開いた素材を `FrameCx::footage` で受ける。
//! - `image`: 静止画。素材の最初のコマを使い、時間で変わらない。
//! - `solid`: 単色。
//! - `text`: 文字。整形と描画は `FrameCx::services` の口に任せる。
//! - `composition`: 入れ子。中身の展開は描画器が行い、種類自身は透明を返す。
//!
//! どの種類も絵の大きさは `cx.size` にそろえる（描画器は (0, 0) に重ねるだけでよい）。

mod composition;
mod footage;
mod solid;
mod text;
mod util;

use std::sync::Arc;

use mm_node_api::NodeKind;

pub use composition::{CompositionKind, COMPOSITION_PARAM};
pub use footage::{FootageKind, ImageKind};
pub use solid::SolidKind;
pub use text::TextKind;

/// 種類の版。パラメータの形を変えたら上げる。
pub const KIND_VERSION: u32 = 1;

/// このクレートの全部の種類。登録簿に入れるときに使う。
pub fn all_kinds() -> Vec<Arc<dyn NodeKind>> {
    vec![
        Arc::new(FootageKind),
        Arc::new(ImageKind),
        Arc::new(SolidKind),
        Arc::new(TextKind),
        Arc::new(CompositionKind),
    ]
}
