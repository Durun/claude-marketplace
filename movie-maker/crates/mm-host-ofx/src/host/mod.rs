//! ホストの実行ファイルの本体。OpenFX の C API のホストを Rust で持ち、プラグインを同じプロセスに読み込む。
//! このプロセスはアプリと分かれているので、プラグインのクラッシュはアプリに `HostCrashed` で届くだけで済む。

mod effect;
mod ffi;
mod names;
mod param_suite;
mod params;
mod pixels;
mod plugin;
mod props;
mod render;
mod serve;
mod suites;

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use mm_host_ofx::{DESCRIBE_COMMAND, SERVE_COMMAND};

const USAGE: &str = "使い方: mm-host-ofx serve | mm-host-ofx describe <バンドル>";

/// サブコマンドを走らせる。
pub fn run() -> ExitCode {
    // SAFETY: プロセスのエラーモードと標準出力の付け替えだけを行う C の関数。プラグインを読み込む前に呼ぶ。
    unsafe { ffi::mm_ofx_quiet_crash_dialogs() };
    let result = protocol_out().and_then(|mut out| {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match (args.first().map(String::as_str), args.get(1)) {
            (Some(SERVE_COMMAND), _) => serve::run(&mut out),
            (Some(DESCRIBE_COMMAND), Some(bundle)) => describe(Path::new(bundle), &mut out),
            _ => Err(USAGE.to_string()),
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mm-host-ofx: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 応答を書く先。標準出力を複製してから、プラグインの printf が混ざらないよう標準出力を標準エラーへ付け替える。
fn protocol_out() -> Result<File, String> {
    let out = clone_stdout().map_err(|e| format!("標準出力を複製できない: {e}"))?;
    // SAFETY: 標準出力のハンドルを標準エラーのものに付け替えるだけ。
    if unsafe { ffi::mm_ofx_stdout_to_stderr() } < 0 {
        return Err("標準出力を付け替えられない".into());
    }
    Ok(out)
}

#[cfg(windows)]
fn clone_stdout() -> std::io::Result<File> {
    use std::os::windows::io::AsHandle;
    Ok(File::from(
        std::io::stdout().as_handle().try_clone_to_owned()?,
    ))
}

#[cfg(not(windows))]
fn clone_stdout() -> std::io::Result<File> {
    use std::os::fd::AsFd;
    Ok(File::from(std::io::stdout().as_fd().try_clone_to_owned()?))
}

/// バンドルを describe し、結果の JSON を 1 行で書く。
fn describe(bundle: &Path, out: &mut impl Write) -> Result<(), String> {
    let desc = plugin::describe_bundle(bundle)?;
    let text = serde_json::to_string(&desc).map_err(|e| e.to_string())?;
    writeln!(out, "{text}")
        .and_then(|()| out.flush())
        .map_err(|e| format!("結果を書けない: {e}"))
}
