//! 描画器の実装（wgpu）。
//!
//! - `WgpuRenderer`: コンポジションと時刻からコマを作る。レイヤーを下から順に、位置・拡大・回転・
//!   不透明度をかけて `Rgba16Float`（premultiplied、線形）に重ねる。入れ子は別のテクスチャに描いてから重ねる。
//! - `WgpuGpu`: 口の `GpuHelper`。種類は `as_any` で降りて wgpu のテクスチャを使える。
//! - 計測: `render.frame` などの CPU のスパンと、パスごとの GPU の時間（wgpu のタイムスタンプ）を
//!   `mm-trace` に載せる。
//!
//! 文書の `Layer` に変形の欄が無い間は、レイヤーの種類のパラメータ `transform.*` を読む（[`transform`]）。

mod audio;
mod compose;
mod context;
mod gpu;
mod layers;
mod renderer;
mod source;
mod timer;
pub mod transform;

pub use context::{GpuContext, FRAME_FORMAT};
pub use gpu::WgpuGpu;
pub use layers::{NESTED_KIND, NESTED_PARAM};
pub use renderer::{WgpuRenderer, WGPU_RENDERER_VERSION};
