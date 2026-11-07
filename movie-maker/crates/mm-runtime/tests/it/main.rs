//! `mm-runtime` の結合テスト。リンクを 1 回にするため、1 つの実行ファイルにまとめる（docs/ci.md）。

mod assemble_real;
#[cfg(all(feature = "ffmpeg", feature = "d3d12va", feature = "cuvid"))]
mod decode_auto;
mod render_texture;
mod vertical;
