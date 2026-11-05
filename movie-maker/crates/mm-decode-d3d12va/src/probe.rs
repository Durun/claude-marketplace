//! 開く前に、別プロセス（`mm-d3d12va-probe`）で素材を試す。
//!
//! NVIDIA の d3d12va が復号に失敗すると、ドライバーの内部エラーでプロセスの D3D12 のデバイスが全部消える
//! （別のデバイスを作って試しても、描画器のデバイスも道連れになる）。失敗しうる素材は、描画器のデバイスで
//! 開く前に別プロセスで確かめ、だめなら `Err(Unsupported)` で次の実装（cuvid、CPU）へ譲る。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use mm_types::{MmError, Result};

/// 試す復号のコマ数。開いた直後に落ちる素材は 1 コマ目で落ちるので、数コマで足りる。
pub const PROBE_FRAMES: usize = 4;

/// 別プロセスの待ちの上限。
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// 実行ファイルの場所を指す環境変数。
pub const PROBE_ENV: &str = "MM_D3D12VA_PROBE";

const PROBE_EXE: &str = "mm-d3d12va-probe.exe";

type Key = (PathBuf, u64, Option<SystemTime>);

/// 結果の覚え。同じ素材を開くたびに別プロセスを起こさないため。
static CACHE: Mutex<Option<HashMap<Key, std::result::Result<(), String>>>> = Mutex::new(None);

fn probe_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(PROBE_ENV) {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    // 実行ファイルと同じ場所、またはテストの `deps` の一つ上。
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|dir| dir.join(PROBE_EXE))
        .find(|p| p.exists())
}

/// `path` を別プロセスで試す。実行ファイルが見つからなければ、試さずに通す（警告を出す）。
pub fn probe(path: &Path, luid: u64) -> Result<()> {
    let meta = std::fs::metadata(path).ok();
    let key: Key = (
        path.to_owned(),
        meta.as_ref().map_or(0, |m| m.len()),
        meta.and_then(|m| m.modified().ok()),
    );
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cached = cache.get_or_insert_with(HashMap::new);
    let result = match cached.get(&key) {
        Some(r) => r.clone(),
        None => {
            let r = run_probe(path, luid);
            cached.insert(key, r.clone());
            r
        }
    };
    result.map_err(|e| MmError::Unsupported(format!("d3d12va の試し復号に失敗: {e}")))
}

fn run_probe(path: &Path, luid: u64) -> std::result::Result<(), String> {
    let Some(exe) = probe_exe() else {
        tracing::warn!("{PROBE_EXE} が見つからないので、d3d12va の試し復号を飛ばす");
        return Ok(());
    };
    let mut child = Command::new(&exe)
        .arg(path)
        .arg("--luid")
        .arg(luid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let mut reason = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err, &mut reason);
                }
                return Err(format!("{status}: {}", reason.trim()));
            }
            None if started.elapsed() > PROBE_TIMEOUT => {
                let _ = child.kill();
                return Err("時間切れ".into());
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}
