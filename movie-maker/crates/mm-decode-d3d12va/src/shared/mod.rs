//! d3d12va と cuvid が共有する部分。cuvid のクレートは `#[path]` で同じソースを取り込む
//! （実装どうしは依存しない規則のため、クレートの依存にはしない）。
//!
//! - [`timeline`]: パケットの PTS の索引。コマの番号と `MediaInfo` を決める。
//! - [`reader`]: 復号器からコマを取り出し、番号を付けて、シークで位置を合わせる。
//! - [`release`]: GPU が読み終えるまで復号結果（FFmpeg のプールの面）を握る。
//! - [`adapter`]: 描画器の GPU を D3D12 のアダプターで頼む。
//! - [`yuv`]: YUV から線形の RGB への変換の係数と WGSL。
//! - [`pool`]: 変換先の `Rgba16Float` のテクスチャを使い回す。

pub mod adapter;
pub mod pool;
pub mod reader;
pub mod release;
pub mod timeline;
pub mod yuv;
