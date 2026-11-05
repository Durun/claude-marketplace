//! movie-maker の基礎の型。GPU と FFmpeg に依存しない。
//!
//! 時刻は有理数（[`RationalTime`]）で持ち、浮動小数の秒を文書に書かない。
//! 色は線形の Rec.709、premultiplied のアルファ（[`Rgba`]）。

pub mod clock;
pub mod color;
pub mod error;
pub mod geom;
pub mod gpu;
pub mod id;
pub mod image;
pub mod param;
pub mod time;

pub use clock::{Clock, ManualClock, SystemClock};
pub use color::{ColorSpace, Rgba};
pub use error::{MmError, Result};
pub use geom::{Rect, Size, Vec2};
pub use gpu::GpuAdapterId;
pub use id::{AssetId, CompositionId, Id, LayerId, NodeId, RenderTargetId};
pub use image::RgbaImage;
pub use param::{ParamSchema, ParamSpec, ParamType, ParamValue, Params};
pub use time::{Rational, RationalTime, TimeRange};
