//! OpenColorIO による色の変換。
//!
//! - [`glsl`]: OCIO の GLSL → WGSL（naga）。OCIO が無くても使える。
//! - `ffi`、[`OcioColor`]: OCIO の本体を呼ぶ部分。feature `native` と環境変数 `OCIO_ROOT` が要る
//!   （切り替え方は README）。

pub mod glsl;

#[cfg(feature = "native")]
pub mod ffi;
#[cfg(feature = "native")]
mod ocio;

#[cfg(feature = "native")]
pub use ocio::{OcioColor, DEFAULT_CONFIG, DISPLAY_SPACE, OCIO_ID};
