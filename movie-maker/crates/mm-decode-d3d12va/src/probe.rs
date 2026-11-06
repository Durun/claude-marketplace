//! 開く前に、別プロセス（`mm-d3d12va-probe`）で素材を試す。
//!
//! NVIDIA の d3d12va がデコードに失敗すると、ドライバーの内部エラーでプロセスの D3D12 のデバイスが全部消える
//! （別のデバイスを作って試しても、レンダラーのデバイスも道連れになる）。失敗しうる素材は、レンダラーのデバイスで
//! 開く前に別プロセスで確かめ、だめなら `Err(Unsupported)` で次の実装（cuvid、CPU）へ譲る。
//! 試せなかったとき（実行ファイルが無い、起動できない、時間切れ）も同じく譲る。素材がデコードできる証拠が無いまま
//! レンダラーのプロセスでデコードすると、クラッシュしたときにデバイスが消えるため。

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use mm_types::{MmError, Result};

/// 試すデコードのフレーム数。開いた直後にクラッシュする素材は 1 フレーム目でクラッシュするので、数フレームで足りる。
pub const PROBE_FRAMES: usize = 4;

/// 別プロセスの待ちの上限。
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// 子の標準エラーのうち、失敗の理由として残す末尾のバイト数。
const STDERR_TAIL_BYTES: usize = 4096;

/// 実行ファイルの場所を指す環境変数。
pub const PROBE_ENV: &str = "MM_D3D12VA_PROBE";

const PROBE_EXE: &str = "mm-d3d12va-probe.exe";

type Key = (PathBuf, u64, Option<SystemTime>);

/// 別プロセスで試した結果。
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// 数フレームをデコードできた。
    Passed,
    /// 子が明示的に失敗した（終了コードが 0 でない）。素材のせいなのでキャッシュしてよい。
    Failed(String),
    /// 試せなかった（実行ファイルが無い、起動できない、時間切れ）。一時の事情かもしれないのでキャッシュしない。
    Unavailable(String),
}

/// 結果のキャッシュ。同じ素材を開くたびに別プロセスを起こさないため。
static CACHE: Mutex<Option<HashMap<Key, std::result::Result<(), String>>>> = Mutex::new(None);

/// 実行ファイルを探す。環境変数が指すファイル、なければ実行ファイルと同じ場所かテストの `deps` の一つ上。
fn locate_exe(env: Option<OsString>, current_exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = env {
        return Some(PathBuf::from(p)).filter(|p| p.exists());
    }
    current_exe?
        .ancestors()
        .skip(1)
        .take(2)
        .map(|dir| dir.join(PROBE_EXE))
        .find(|p| p.exists())
}

/// `path` を別プロセスで試す。試せなかったときも `Err(Unsupported)` で、その結果はキャッシュしない。
pub fn probe(path: &Path, luid: u64) -> Result<()> {
    let meta = std::fs::metadata(path).ok();
    let key: Key = (
        path.to_owned(),
        meta.as_ref().map_or(0, |m| m.len()),
        meta.and_then(|m| m.modified().ok()),
    );
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cached = cache.get_or_insert_with(HashMap::new);
    if let Some(r) = cached.get(&key) {
        return to_result(r.clone());
    }
    let exe = locate_exe(
        std::env::var_os(PROBE_ENV),
        std::env::current_exe().ok().as_deref(),
    );
    let outcome = match exe {
        Some(exe) => run_exe(&exe, path, luid),
        None => Outcome::Unavailable(format!("{PROBE_EXE} が見つからない")),
    };
    to_result(settle(cached, key, outcome))
}

/// 結果をキャッシュする種類ならキャッシュして、判定を返す。
fn settle(
    cached: &mut HashMap<Key, std::result::Result<(), String>>,
    key: Key,
    outcome: Outcome,
) -> std::result::Result<(), String> {
    let verdict = match outcome {
        Outcome::Passed => Ok(()),
        Outcome::Failed(reason) => Err(reason),
        Outcome::Unavailable(reason) => return Err(reason),
    };
    cached.insert(key, verdict.clone());
    verdict
}

fn to_result(verdict: std::result::Result<(), String>) -> Result<()> {
    verdict.map_err(|e| MmError::Unsupported(format!("d3d12va の試しデコードに失敗: {e}")))
}

fn run_exe(exe: &Path, path: &Path, luid: u64) -> Outcome {
    let mut cmd = Command::new(exe);
    cmd.arg(path).arg("--luid").arg(luid.to_string());
    run_child(cmd, PROBE_TIMEOUT)
}

/// 子を起こして終わりを待つ。標準エラーは別のスレッドで読み続け（読まないとパイプが埋まって子が止まる）、
/// 末尾だけを理由に残す。
fn run_child(mut cmd: Command, timeout: Duration) -> Outcome {
    let mut child = match cmd.stdout(Stdio::null()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            return Outcome::Unavailable(format!("{:?} を起動できない: {e}", cmd.get_program()))
        }
    };
    let reader = child
        .stderr
        .take()
        .map(|err| std::thread::spawn(move || read_tail(err)));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Outcome::Unavailable("時間切れ".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Outcome::Unavailable(e.to_string()),
        }
    };
    let reason = reader
        .and_then(|h| h.join().ok())
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .unwrap_or_default();
    if status.success() {
        Outcome::Passed
    } else {
        Outcome::Failed(format!("{status}: {reason}"))
    }
}

/// `r` を最後まで読み、末尾の `STDERR_TAIL_BYTES` バイトを返す。
fn read_tail(mut r: impl Read) -> Vec<u8> {
    let mut tail: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return tail,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > STDERR_TAIL_BYTES {
                    tail.drain(..tail.len() - STDERR_TAIL_BYTES);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD_ENV: &str = "MM_PROBE_TEST_CHILD";
    const FLOOD_BYTES: usize = 4 * 1024 * 1024;

    /// 子として呼ばれたときだけ働く。環境変数が無ければ何もしない。
    #[test]
    fn child_helper() {
        let Ok(mode) = std::env::var(CHILD_ENV) else {
            return;
        };
        let line = "x".repeat(99) + "\n";
        for _ in 0..FLOOD_BYTES / line.len() {
            eprint!("{line}");
        }
        eprintln!("END-MARKER");
        match mode.as_str() {
            "pass" => std::process::exit(0),
            "fail" => std::process::exit(1),
            _ => std::thread::sleep(Duration::from_secs(120)),
        }
    }

    fn child(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().expect("テストの実行ファイル"));
        cmd.args([
            "--exact",
            "probe::tests::child_helper",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD_ENV, mode);
        cmd
    }

    #[test]
    fn 標準エラーを大量に書く子でも詰まらず通る() {
        let started = Instant::now();
        assert_eq!(
            run_child(child("pass"), Duration::from_secs(20)),
            Outcome::Passed
        );
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "詰まらずに終わる"
        );
    }

    #[test]
    fn 失敗した子の理由は標準エラーの末尾が残る() {
        match run_child(child("fail"), Duration::from_secs(20)) {
            Outcome::Failed(reason) => {
                assert!(reason.ends_with("END-MARKER"), "{reason}");
                assert!(reason.len() < 2 * STDERR_TAIL_BYTES, "末尾だけ残す");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 時間切れは試せなかった扱いになる() {
        let outcome = run_child(child("hang"), Duration::from_millis(1500));
        assert!(matches!(outcome, Outcome::Unavailable(_)), "{outcome:?}");
    }

    #[test]
    fn 起動できない実行ファイルは試せなかった扱いになる() {
        let outcome = run_child(Command::new("no-such-probe-exe.exe"), PROBE_TIMEOUT);
        assert!(matches!(outcome, Outcome::Unavailable(_)), "{outcome:?}");
    }

    #[test]
    fn 実行ファイルが無ければ見つからない() {
        let dir = std::env::temp_dir().join("mm-probe-locate-empty/a/b");
        assert_eq!(locate_exe(None, Some(&dir.join("test.exe"))), None);
        let missing = OsString::from("D:/no/such/mm-d3d12va-probe.exe");
        assert_eq!(locate_exe(Some(missing), None), None);
    }

    #[test]
    fn 試せなかった結果はキャッシュせず_失敗と成功はキャッシュする() {
        let key = |n: &str| -> Key { (PathBuf::from(n), 0, None) };
        let mut cache = HashMap::new();
        let unavailable = settle(&mut cache, key("a"), Outcome::Unavailable("無い".into()));
        assert!(unavailable.is_err() && cache.is_empty());
        assert!(settle(&mut cache, key("b"), Outcome::Passed).is_ok());
        assert!(settle(
            &mut cache,
            key("c"),
            Outcome::Failed("クラッシュした".into())
        )
        .is_err());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn 実行ファイルが無いときは_unsupported_で譲る() {
        let outcome = Outcome::Unavailable("無い".into());
        let mut cache = HashMap::new();
        let key: Key = (PathBuf::from("x"), 0, None);
        let verdict = settle(&mut cache, key, outcome);
        assert!(matches!(to_result(verdict), Err(MmError::Unsupported(_))));
    }
}
