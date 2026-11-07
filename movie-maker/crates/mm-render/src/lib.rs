//! レンダラーの実装（wgpu）。
//!
//! - `WgpuRenderer`: コンポジションと時刻からフレームを作る。レイヤーを下から順に、位置・拡大・回転・
//!   不透明度をかけて `Rgba16Float`（premultiplied）に重ねる。入れ子は別のテクスチャに描いてから重ねる。
//!   入力のフレーム（ノードの種類やデコーダーが返す画像）は線形の premultiplied という契約で、レンダラーは色空間を変換しない。
//! - GPU のコンテキストと `GpuHelper` の wgpu 実装は `mm-gpu`（共通実装）から使う。デコーダーが返す
//!   GPU のフレームは `import` で借用し、合成の `submit` の前に待ちを登録し、後に返す。
//! - 計測: `render.frame` などの CPU のスパンと、パスごとの GPU の時間（wgpu のタイムスタンプ）を
//!   `mm-trace` に載せる。
//! - 変形はプロジェクトのデータの `Layer.transform` を読む（[`transform`]）。入れ子（種類 `composition`）はレンダラーが
//!   展開する。ノードのインターフェースからはプロジェクトのデータが見えないため、種類自身は透明を返す（[`NESTED_KIND`]）。
//! - シーケンス（種類 `sequence`）の展開とカットの境のトランジションは `sequence.rs`、レイヤーの効果は `effects.rs` に置く。
//!   画像を入力に取る種類は自前のコマンドをすぐ提出するので、呼ぶ前にレンダラーが積んだコマンドを提出する。
//! - 書き出しは GPU で NV12 にしてリードバックを先行させ、エンコーダーに押す（[`export`]）。
//! - デバイスが消えたら（`GpuContext::is_lost`）、次のフレームの前にコンテキストとその上の全部を作り直す。

mod audio;
mod audio_gain;
mod clip;
mod compose;
mod crossfade;
mod effects;
mod export;
mod frame_cache;
mod kind_cache;
mod layers;
mod renderer;
mod sequence;
mod source;
pub mod transform;

pub use frame_cache::{FrameCacheConfig, MemoStats, DEFAULT_MEMORY_BYTES};
pub use layers::{NESTED_KIND, NESTED_PARAM};
pub use mm_gpu::{GpuContext, GpuRequest, WgpuGpu, FRAME_FORMAT};
pub use renderer::{WgpuRenderer, WGPU_RENDERER_VERSION};
