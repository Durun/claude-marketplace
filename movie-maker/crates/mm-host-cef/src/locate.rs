//! アプリが起こすホストの実行ファイルを探す。CEF の配布物（`CEF_PATH`）が無い環境では探さない。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::launcher::{child_path, CEF_PATH_ENV};

/// ホストの実行ファイルを直に指す環境変数。`--features cef` で作ったものを指す。
pub const HOST_ENV: &str = "MM_CEF_HOST";
/// 能力を問い合わせるサブコマンド。CEF を初期化せずに [`PROBE_REPLY`] を 1 行書いて終わる。
pub const PROBE_COMMAND: &str = "probe";
/// `probe` の答え。`cef` の feature つきで作ったホストだけが書く。
pub const PROBE_REPLY: &str = "mm-host-cef cef";
/// sandbox つきの起動（`bundle.sh` の組み立て）のホストが `probe` の答えの後ろに足す印。
pub const PROBE_SANDBOX: &str = " sandbox";
/// ホストの実行ファイルの名前（拡張子を除く）。
const HOST_STEM: &str = "mm-host-cef";
/// 問い合わせの期限。`libcef.dll` の読み込みを含む。
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// 問い合わせの終わりを確かめる間隔。
const PROBE_STEP: Duration = Duration::from_millis(10);

/// 起こすホストの実行ファイル。`CEF_PATH` が無ければ `None`。
/// `MM_CEF_HOST` があればそれ、無ければ今の実行ファイルと同じフォルダー（テストの実行ファイルなら 1 つ上も）の
/// `mm-host-cef`。`cef` の feature 無しで作ったもの（[`supports_cef`] が偽）は使わず `None` にする。
pub fn find_host() -> Option<PathBuf> {
    std::env::var_os(CEF_PATH_ENV)?;
    if let Some(path) = std::env::var_os(HOST_ENV) {
        return Some(PathBuf::from(path)).filter(|p| p.is_file() && supports_cef(p));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = format!("{HOST_STEM}{}", std::env::consts::EXE_SUFFIX);
    let found = [Some(dir), dir.parent()]
        .into_iter()
        .flatten()
        .map(|d: &Path| d.join(&name))
        .find(|p| p.is_file());
    found.filter(|p| supports_cef(p))
}

/// `exe` が CEF で描けるホストか。`exe probe` を起こして答えを見る。結果は実行ファイルごとに覚える。
/// `cef` の feature 無しのホストは何も書かずに終わり、`libcef.dll` が見つからないホストは起動に失敗するので、どちらも偽。
pub fn supports_cef(exe: &Path) -> bool {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    let known = KNOWN.get_or_init(Default::default);
    if let Some(answer) = known.lock().ok().and_then(|k| k.get(exe).copied()) {
        return answer;
    }
    let answer = probe_reply(exe).is_some_and(|out| out.starts_with(PROBE_REPLY));
    if let Ok(mut k) = known.lock() {
        k.insert(exe.to_path_buf(), answer);
    }
    answer
}

/// `exe probe` の答えの 1 行（[`PROBE_REPLY`]、sandbox つきなら [`PROBE_SANDBOX`] が続く）。
/// 起動できないか、期限までに正常に終わらなければ `None`。
pub fn probe_reply(exe: &Path) -> Option<String> {
    probe(exe).map(|out| out.trim().to_string())
}

/// `exe probe` の標準出力。起動できないか、期限までに正常に終わらなければ `None`。
fn probe(exe: &Path) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.arg(PROBE_COMMAND)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = child_path() {
        cmd.env("PATH", path);
    }
    let mut child = cmd.spawn().ok()?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < PROBE_TIMEOUT => std::thread::sleep(PROBE_STEP),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    status.success().then_some(out)
}
