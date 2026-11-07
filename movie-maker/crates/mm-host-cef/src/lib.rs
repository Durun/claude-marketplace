//! CEF のオフスクリーン描画を、別プロセスのアプリへ共有テクスチャで渡すホスト（`mm-host-api` の実装）。
//!
//! - アプリ側: [`CefLauncher`] が実行ファイル `mm-host-cef serve` を起こし、パイプで 1 行ずつ JSON をやり取りする。
//!   これは CEF の配布物なしでコンパイルできる。
//! - ホスト側（`cef` feature）: CEF の `OnAcceleratedPaint` の D3D11 共有テクスチャを、コールバックの中で
//!   ホストが作った D3D12 の共有テクスチャへコピーし、その NT ハンドルを `HostSpec::client_pid` のプロセスへ複製して返す。
//!
//! 手順と数字は README と `docs/design/spikes/cef.md`。

#[cfg(feature = "cef")]
extern crate self as mm_host_cef;

#[cfg(feature = "cef")]
mod host;

/// ホストの本体（`serve`、`bench`、`probe`）。実行ファイルの `main` と、sandbox つきの起動の DLL
/// （`examples/sandboxed.rs`）が呼ぶ。`sandbox` は CEF の `bootstrapc.exe` が渡す sandbox の情報で、
/// null なら sandbox なしで動く。
#[cfg(feature = "cef")]
pub fn host_main(sandbox: *mut std::ffi::c_void) -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some(PROBE_COMMAND) {
        let mode = if sandbox.is_null() { "" } else { PROBE_SANDBOX };
        println!("{PROBE_REPLY}{mode}");
        return std::process::ExitCode::SUCCESS;
    }
    host::run(sandbox)
}

mod job;
mod launcher;
mod locate;
pub mod pixel;
pub mod policy;

pub use launcher::{
    remove_work_dir, CefLauncher, PipeHost, CEF_PATH_ENV, SERVE_COMMAND, SPEC_ENV, WORK_DIR_ENV,
};
pub use locate::{
    find_host, probe_reply, supports_cef, HOST_ENV, PROBE_COMMAND, PROBE_REPLY, PROBE_SANDBOX,
};
