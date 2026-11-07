//! `mm-host-cef` の実行ファイル。`serve` はアプリ（`CefLauncher`）からの要求に答え、`bench` は 1 フレームの時間を測る。
//!
//! `cef` feature を切ったビルドは CEF の配布物を要らない代わりに、何もせずに終わる。
//! 取得と実行の手順は README を見る。

#[cfg(feature = "cef")]
mod host;

#[cfg(feature = "cef")]
fn main() -> std::process::ExitCode {
    host::run()
}

#[cfg(not(feature = "cef"))]
fn main() -> std::process::ExitCode {
    eprintln!("mm-host-cef は `--features cef` なしでビルドされている。README の手順で CEF を入れて作り直す。");
    std::process::ExitCode::from(2)
}
