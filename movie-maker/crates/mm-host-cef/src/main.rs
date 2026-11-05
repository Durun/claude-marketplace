//! CEF のオフスクリーン描画を共有テクスチャ（D3D11 → D3D12 → wgpu）で受け取るホスト。
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
