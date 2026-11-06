//! プレビューの音の出力。cpal のストリームに音を流し、実際に鳴った位置を再生の主の時計として返す。
//!
//! [`AudioOutput`] は既定のデバイスを開き、`AudioSink` を実装する。ミキサーの出力（48 kHz ステレオ）を
//! デバイスの形に変えて足し、`position_secs` が耳に届いている位置を返す。映像はこれに合わせる。
//! [`Ring`] はリングバッファと時計で、デバイスに依存しないので時計の外挿は単体で試せる。
//!
//! デバイスが無い環境（CI など）では `open` が `Unsupported` を返す。呼び出し側は壁時計に切り替える。

mod convert;
mod output;
mod ring;

pub use convert::{remap_channels, Resampler};
pub use output::AudioOutput;
pub use ring::Ring;
