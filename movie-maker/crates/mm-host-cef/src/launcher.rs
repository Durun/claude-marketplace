//! アプリ側のランチャー。`mm-host-cef serve` を子プロセスで起こし、標準入出力で
//! `HostRequest`/`HostResponse` の JSON を 1 行ずつやり取りする。
//!
//! 期限を過ぎた要求の応答は後から届くので、その数を数えておき、次の要求の前に読み捨てる
//! （ホストは要求を 1 つずつ順に処理し、どの要求にも 1 行で答える）。読み捨てる応答の共有ハンドルはここで閉じる。
//!
//! ホストの一時フォルダー（CEF のキャッシュとページ）はランチャーが作って渡し、ホストを止めた後に消す。
//! kill で止めたホストは自分で片付けられないため。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mm_host_api::{
    HostLauncher, HostProcess, HostRequest, HostResponse, HostSpec, SharedFrame, PROTOCOL_VERSION,
};
use mm_types::{MmError, Result};

use crate::job::{kill_tree, Job};

/// ホストへ起動の指定（`HostSpec` の JSON）を渡す環境変数。
pub const SPEC_ENV: &str = "MM_HOST_SPEC";
/// ホストへ一時フォルダーを渡す環境変数。ホストはこの下に CEF のキャッシュとページを置く。
pub const WORK_DIR_ENV: &str = "MM_HOST_WORK_DIR";
/// プロトコルで応答するモードのサブコマンド。
pub const SERVE_COMMAND: &str = "serve";
/// CEF の配布物の置き場所。子プロセスが `libcef.dll` を見つけられるよう `PATH` の先頭に足す。
pub const CEF_PATH_ENV: &str = "CEF_PATH";
/// 起動直後の `Ping` の期限。CEF の初期化と GPU プロセスの起動を含む。
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// 止めたホストの一時フォルダーを消し終えるまでの期限。CEF の子のプロセスが終わってファイルを放すのを待つ。
const CLEANUP_LIMIT: Duration = Duration::from_secs(5);
/// 一時フォルダーを消し直す間隔。
const CLEANUP_STEP: Duration = Duration::from_millis(50);

/// `mm-host-cef` を起こすランチャー。
#[derive(Debug, Clone, Default)]
pub struct CefLauncher;

impl HostLauncher for CefLauncher {
    fn id(&self) -> &'static str {
        "cef"
    }

    fn spawn(&self, spec: &HostSpec) -> Result<Box<dyn HostProcess>> {
        Ok(Box::new(PipeHost::start(spec.clone())?))
    }
}

/// 起動した 1 プロセスとの接続。
struct Connection {
    child: Mutex<Child>,
    /// ホストと CEF の子のプロセスの木。作れなかった環境ではホストだけを止める。
    job: Option<Job>,
    stdin: ChildStdin,
    lines: Receiver<String>,
    pid: u32,
    work_dir: PathBuf,
}

/// 子プロセスのホスト。
pub struct PipeHost {
    spec: HostSpec,
    conn: Option<Connection>,
    /// 期限を過ぎて、まだ読み捨てていない応答の数。
    stale: usize,
    /// 読み捨てた応答のうち、閉じられなかった共有ハンドルの数。
    open: usize,
}

impl PipeHost {
    /// 起動して `Ping` が通るまで待つ。
    pub fn start(spec: HostSpec) -> Result<Self> {
        let mut host = Self {
            conn: Some(launch(&spec)?),
            spec,
            stale: 0,
            open: 0,
        };
        match host.request(&HostRequest::Ping, STARTUP_TIMEOUT) {
            Ok(HostResponse::Pong { protocol_version }) if protocol_version == PROTOCOL_VERSION => {
                Ok(host)
            }
            other => {
                let _ = host.kill();
                Err(MmError::HostCrashed(format!(
                    "起動直後の Ping にプロトコルのバージョン {PROTOCOL_VERSION} の Pong が来ない: {other:?}"
                )))
            }
        }
    }

    fn send(conn: &mut Connection, req: &HostRequest) -> Result<()> {
        let mut line = serde_json::to_string(req)?;
        line.push('\n');
        conn.stdin
            .write_all(line.as_bytes())
            .and_then(|()| conn.stdin.flush())
            .map_err(|e| MmError::HostCrashed(format!("要求を書けない: {e}")))
    }
}

/// 子プロセスの `PATH`。`CEF_PATH` があれば先頭に足し、子プロセスが `libcef.dll` を見つけられるようにする。
pub(crate) fn child_path() -> Option<std::ffi::OsString> {
    let cef = std::env::var_os(CEF_PATH_ENV)?;
    let rest = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(cef.into()).chain(std::env::split_paths(&rest));
    std::env::join_paths(paths).ok()
}

/// このプロセスが起こすホストごとの一時フォルダーを作る。
fn create_work_dir() -> Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mm-host-cef-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| MmError::Internal(format!("{} を作れない: {e}", dir.display())))?;
    Ok(dir)
}

/// 止めたホストの一時フォルダーを消す。CEF の子のプロセスがファイルを放すまで `limit` まで消し直す。
/// 消せたら（元から無ければ）真。
pub fn remove_work_dir(dir: &Path, limit: Duration) -> bool {
    let start = Instant::now();
    loop {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) if start.elapsed() < limit => std::thread::sleep(CLEANUP_STEP),
            Err(_) => return false,
        }
    }
}

fn launch(spec: &HostSpec) -> Result<Connection> {
    let work_dir = create_work_dir()?;
    let mut cmd = Command::new(&spec.executable);
    cmd.arg(SERVE_COMMAND)
        .args(&spec.args)
        .env(SPEC_ENV, serde_json::to_string(spec)?)
        .env(WORK_DIR_ENV, &work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(path) = child_path() {
        cmd.env("PATH", path);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            remove_work_dir(&work_dir, Duration::ZERO);
            return Err(MmError::HostCrashed(format!(
                "{} を起動できない: {e}",
                spec.executable.display()
            )));
        }
    };
    let job = Job::attach(&child);
    let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
        (Some(i), Some(o)) => (i, o),
        _ => return Err(MmError::Internal("子プロセスのパイプが無い".into())),
    };
    let (tx, lines) = mpsc::channel();
    // 読む側を別スレッドにして、期限つきで待てるようにする。パイプが閉じるとスレッドも終わる。
    std::thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
        {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Ok(Connection {
        pid: child.id(),
        child: Mutex::new(child),
        job,
        stdin,
        lines,
        work_dir,
    })
}

/// 読み捨てる応答の共有ハンドルを閉じる。閉じられなかった数を返す。
fn discard(response: &HostResponse) -> usize {
    let HostResponse::Frame(SharedFrame::SharedTexture { handle, fence, .. }) = response else {
        return 0;
    };
    let handles = std::iter::once(*handle).chain(fence.map(|f| f.handle));
    handles.filter(|h| !close_handle(*h)).count()
}

#[cfg(windows)]
fn close_handle(handle: u64) -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    // SAFETY: ホストがこのプロセスへ複製した NT ハンドルで、ほかに使う所は無い。
    unsafe { CloseHandle(HANDLE(handle as usize as *mut _)) }.is_ok()
}

#[cfg(not(windows))]
fn close_handle(_handle: u64) -> bool {
    false
}

impl HostProcess for PipeHost {
    fn is_alive(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| {
            let mut child = c.child.lock().unwrap_or_else(|e| e.into_inner());
            matches!(child.try_wait(), Ok(None))
        })
    }

    fn pid(&self) -> Option<u32> {
        self.conn.as_ref().map(|c| c.pid)
    }

    fn request(&mut self, req: &HostRequest, timeout: Duration) -> Result<HostResponse> {
        let alive = self.is_alive();
        let conn = self
            .conn
            .as_mut()
            .filter(|_| alive)
            .ok_or_else(|| MmError::HostCrashed("ホストが動いていない".into()))?;
        Self::send(conn, req)?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match conn.lines.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    self.stale += 1;
                    return Err(MmError::Timeout {
                        what: "host_cef.request".into(),
                        limit: timeout,
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(MmError::HostCrashed("ホストの出力が閉じた".into()))
                }
            };
            // 読めない行をスキップすると、読み捨てる数が合わなくなり、以後の要求が前の要求の応答を受け取り続ける。
            let response: HostResponse = match serde_json::from_str(&line) {
                Ok(response) => response,
                Err(e) => {
                    self.kill()?;
                    return Err(MmError::HostCrashed(format!("ホストの応答が読めない: {e}")));
                }
            };
            if self.stale == 0 {
                return Ok(response);
            }
            self.stale -= 1;
            self.open += discard(&response);
        }
    }

    fn kill(&mut self) -> Result<()> {
        if let Some(conn) = self.conn.take() {
            let mut child = conn.child.lock().unwrap_or_else(|e| e.into_inner());
            let killed = kill_tree(&mut child, conn.job.as_ref());
            drop(child);
            drop(conn.job);
            // 消せなくても次のホストは別のフォルダーを使うので、止めたことを優先して続ける。
            remove_work_dir(&conn.work_dir, CLEANUP_LIMIT);
            killed?;
        }
        self.stale = 0;
        Ok(())
    }

    fn restart(&mut self) -> Result<()> {
        self.kill()?;
        *self = Self::start(self.spec.clone())?;
        Ok(())
    }

    fn open_handles(&self) -> usize {
        self.open
    }
}

impl PipeHost {
    /// 動いているホストの一時フォルダー。止めた後は `None`。
    pub fn work_dir(&self) -> Option<&Path> {
        self.conn.as_ref().map(|c| c.work_dir.as_path())
    }

    /// `Shutdown` を送って終わるのを少し待ち、終わらなければ止める。
    fn shutdown(&mut self) {
        const WAIT: Duration = Duration::from_secs(2);
        const STEP: Duration = Duration::from_millis(10);
        if self.is_alive() && self.request(&HostRequest::Shutdown, WAIT).is_ok() {
            let start = Instant::now();
            while self.is_alive() && start.elapsed() < WAIT {
                std::thread::sleep(STEP);
            }
        }
        let _ = self.kill();
    }
}

impl Drop for PipeHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(executable: &str) -> HostSpec {
        HostSpec {
            executable: executable.into(),
            args: Vec::new(),
            default_timeout: Duration::from_secs(1),
            gpu: None,
            client_pid: None,
        }
    }

    #[test]
    fn 実行ファイルが無ければ起動の失敗を返す() {
        let err = CefLauncher
            .spawn(&spec("mm-host-cef-が無い場所/none.exe"))
            .err()
            .expect("起動できないはず");
        assert!(matches!(err, MmError::HostCrashed(_)), "{err:?}");
    }

    #[test]
    fn cpu_の画像の応答は閉じるハンドルが無い() {
        let frame = SharedFrame::Cpu {
            size: mm_types::Size::new(1, 1),
            data: vec![0.0; 4],
        };
        assert_eq!(discard(&HostResponse::Frame(frame)), 0);
        assert_eq!(discard(&HostResponse::Prepared), 0);
    }
}
