//! 基本のノードの種類。
//!
//! - `footage`: 撮影素材。レンダラーが開いた素材を `FrameCx::footage` で受ける。
//! - `image`: 静止画。素材の最初のフレームを使い、時間で変わらない。
//! - `solid`: 単色。
//! - `text`: 文字。シェーピングと描画は `FrameCx::services` のインターフェースに任せ、同じパラメータの結果は覚える。
//! - `composition`、`sequence`: 入れ子とカットの並び。内容の展開はレンダラーが行い、種類自身は透明を返す。
//! - `shader.wgsl`: 利用者が書いた WGSL のフラグメントシェーダー。
//! - `color.adjust`、`color.cdl`、`color.lut`、`color.convert`: 線形の光の上の色の処理（効果）。
//! - `transition.dissolve`: カットの境のクロスディゾルブ。
//! - `transition.wgsl`: 利用者が書いた WGSL で前後のカットを混ぜるトランジション。
//!
//! 画像の大きさは `cx.size`（画布）にそろえる。例外は 2 つで、レンダラーはどちらも `cx.region` の矩形に置く:
//! `text` は `cx.region` の分だけの画布を返し、`footage` の GPU のフレームは素材のままの大きさで返す。

mod color;
mod color_lut;
mod composition;
mod footage;
mod gpu_pass;
mod lut;
mod sequence;
mod shader;
mod solid;
mod text;
mod transition;
mod transition_wgsl;
mod util;

use std::sync::Arc;

use mm_node_api::NodeKind;

pub use color::{ColorAdjustKind, ColorCdlKind};
pub use color_lut::{ColorConvertKind, ColorLutKind, LUT_PARAM};
pub use composition::{CompositionKind, COMPOSITION_PARAM};
pub use footage::{FootageKind, ImageKind};
pub use sequence::{SequenceKind, SEQUENCE_PARAM};
pub use shader::ShaderWgslKind;
pub use solid::SolidKind;
pub use text::TextKind;
pub use transition::{DissolveKind, TRANSITION_PROGRESS_PARAM};
pub use transition_wgsl::{TransitionWgslKind, TRANSITION_DURATION_PARAM, TRANSITION_REWIND_PARAM};

/// 種類のバージョン。パラメータの形を変えたら上げる。
pub const KIND_VERSION: u32 = 1;

/// このクレートの全部の種類。レジストリに入れるときに使う。
pub fn all_kinds() -> Vec<Arc<dyn NodeKind>> {
    vec![
        Arc::new(FootageKind),
        Arc::new(ImageKind),
        Arc::new(SolidKind),
        Arc::new(TextKind::default()),
        Arc::new(CompositionKind),
        Arc::new(SequenceKind),
        Arc::new(ShaderWgslKind::default()),
        Arc::new(ColorAdjustKind::default()),
        Arc::new(ColorCdlKind::default()),
        Arc::new(ColorLutKind::default()),
        Arc::new(ColorConvertKind::default()),
        Arc::new(DissolveKind::default()),
        Arc::new(TransitionWgslKind::default()),
    ]
}
