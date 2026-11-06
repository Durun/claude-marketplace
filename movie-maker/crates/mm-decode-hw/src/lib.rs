//! `mm-decode-d3d12va` と `mm-decode-cuvid` が共有する、GPU でデコードするデコーダーの部品
//! （実装の共有部。`docs/rfcs/0007-decode-shared-crates.md`）。FFmpeg の CPU の部分は
//! `mm-decode-ffmpeg-core` にあり、こちらはそれに加えて wgpu と D3D12 に依存する。
//!
//! - `adapter`: レンダラーの GPU を D3D12 のアダプターで頼む。
//! - `device`: wgpu のキューへのフェンスの待ちと信号、デバイスの消失の確認。
//! - `release`: GPU が読み終えるまでデコード結果（FFmpeg のプールの面）を握る。
//! - `yuv`: YUV から線形の RGB への変換の係数と WGSL。
//! - `render`: 変換の描画パスの組み立てと実行。
//! - `pool`: 変換先の `Rgba16Float` のテクスチャを使い回す。
//!
//! feature `hw` を入れたときだけ中身をビルドする。
#![cfg(feature = "hw")]

pub mod adapter;
pub mod device;
pub mod pool;
pub mod release;
pub mod render;
pub mod yuv;

#[cfg(feature = "testing")]
pub mod testing;
