//! CEF のオフスクリーン描画を、別プロセスのアプリへ共有テクスチャで渡すホスト（`mm-host-api` の実装）。
//!
//! - アプリ側: [`CefLauncher`] が実行ファイル `mm-host-cef serve` を起こし、パイプで 1 行ずつ JSON をやり取りする。
//!   これは CEF の配布物なしでコンパイルできる。
//! - ホスト側（`cef` feature）: CEF の `OnAcceleratedPaint` の D3D11 共有テクスチャを、コールバックの中で
//!   ホストが作った D3D12 の共有テクスチャへコピーし、その NT ハンドルを `HostSpec::client_pid` のプロセスへ複製して返す。
//!
//! 手順と数字は README と `docs/design/spikes/cef.md`。

mod launcher;
pub mod pixel;

pub use launcher::{CefLauncher, PipeHost, CEF_PATH_ENV, SERVE_COMMAND, SPEC_ENV};
