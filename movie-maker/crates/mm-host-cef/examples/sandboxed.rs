//! sandbox つきで動かすホストの本体の DLL。CEF の `bootstrapc.exe` を `mm-host-cef.exe` の名前で置き、
//! 隣のこの DLL（`mm-host-cef.dll`）の `RunConsoleMain` を呼ばせる（`bundle.sh` が組み立てる）。
//!
//! Windows の CEF は、sandbox を Chromium と同じツールチェーンで作った起動の実行ファイルに持たせ、
//! アプリの処理を DLL で受け取る形にしている。実行ファイルの `mm-host-cef` を直に起こすと sandbox なしで動く。

/// `bootstrapc.exe` が呼ぶ入口。`sandbox_info` を CEF の `execute_process` と `initialize` に渡す。
///
/// # Safety
/// `bootstrapc.exe` が渡す引数のまま呼ぶ。コマンドラインはプロセスのものを読むので `argc` と `argv` は使わない。
#[no_mangle]
pub unsafe extern "C" fn RunConsoleMain(
    _argc: std::ffi::c_int,
    _argv: *mut *mut std::ffi::c_char,
    sandbox_info: *mut std::ffi::c_void,
    _version_info: *mut std::ffi::c_void,
) -> std::ffi::c_int {
    let code = mm_host_cef::host_main(sandbox_info);
    std::ffi::c_int::from(code != std::process::ExitCode::SUCCESS)
}
