//! `mm-host-cef` の実行ファイル。`serve` はアプリ（`CefLauncher`）からの要求に答え、`bench` は 1 フレームの時間を測る。
//! `probe` は CEF を初期化せずに、CEF で描けることを答える（アプリの `find_host` が使う）。
//!
//! `cef` feature を切ったビルドは CEF の配布物を要らない代わりに、何もせずに終わる。
//! 取得と実行の手順は README を見る。

#[cfg(feature = "cef")]
fn main() -> std::process::ExitCode {
    mm_host_cef::host_main(std::ptr::null_mut())
}

#[cfg(not(feature = "cef"))]
fn main() -> std::process::ExitCode {
    eprintln!("mm-host-cef は `--features cef` なしでビルドされている。README の手順で CEF を入れて作り直す。");
    std::process::ExitCode::from(2)
}
