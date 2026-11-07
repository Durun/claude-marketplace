//! アプリ側のランチャー。`mm-host-ofx serve` を子プロセスで起こし、標準入出力で
//! `HostRequest`/`HostResponse` の JSON を 1 行ずつやり取りする（`mm-host-cef` の `PipeHost` と同じ約束）。
//!
//! 期限を過ぎた要求の応答は後から届くので、その数を数えておき、次の要求の前に読み捨てる
//! （ホストは要求を 1 つずつ順に処理し、どの要求にも 1 行で答える）。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mm_host_api::{
    HostLauncher, HostProcess, HostRequest, HostResponse, HostSpec, SharedFrame, PROTOCOL_VERSION,
};
use mm_types::{MmError, Result};

use crate::desc::BundleDesc;

/// ホストへ起動の指定（`HostSpec` の JSON）を渡す環境変数。
pub const SPEC_ENV: &str = "MM_HOST_SPEC";
/// プロトコルで応答するモードのサブコマンド。
pub const SERVE_COMMAND: &str = "serve";
/// 1 つのバンドルを describe して JSON を 1 行で出すサブコマンド。
pub const DESCRIBE_COMMAND: &str = "describe";
/// 起動直後の `Ping` の期限。プラグインはまだ読み込まないので短くてよい。
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// `mm-host-ofx` を起こすランチャー。
#[derive(Debug, Clone, Default)]
pub struct OfxLauncher;

impl HostLauncher for OfxLauncher {
    fn id(&self) -> &'static str {
        "ofx"
    }

    fn spawn(&self, spec: &HostSpec) -> Result<Box<dyn HostProcess>> {
        Ok(Box::new(PipeHost::start(spec.clone())?))
    }
}

/// `executable describe <bundle>` を別プロセスで走らせ、バンドルの記述を受け取る。
/// プラグインの describe がクラッシュしても、このプロセスには `HostCrashed` で返る。
pub fn describe_bundle(executable: &Path, bundle: &Path, timeout: Duration) -> Result<BundleDesc> {
    let mut child = Command::new(executable)
        .arg(DESCRIBE_COMMAND)
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            MmError::HostCrashed(format!("{} を起動できない: {e}", executable.display()))
        })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| MmError::Internal("子プロセスのパイプが無い".into()))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = tx.send(stdout.read_to_string(&mut text).map(|_| text));
    });
    let text = match rx.recv_timeout(timeout) {
        Ok(read) => read?,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(MmError::Timeout {
                what: "host_ofx.describe".into(),
                limit: timeout,
            });
        }
    };
    let status = child.wait()?;
    if !status.success() {
        return Err(MmError::HostCrashed(format!(
            "{} の describe が {status} で終わった",
            bundle.display()
        )));
    }
    Ok(serde_json::from_str(text.trim())?)
}

/// 起動した 1 プロセスとの接続。
struct Connection {
    child: Mutex<Child>,
    stdin: ChildStdin,
    lines: Receiver<String>,
    pid: u32,
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

fn launch(spec: &HostSpec) -> Result<Connection> {
    let mut child = Command::new(&spec.executable)
        .arg(SERVE_COMMAND)
        .args(&spec.args)
        .env(SPEC_ENV, serde_json::to_string(spec)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            MmError::HostCrashed(format!("{} を起動できない: {e}", spec.executable.display()))
        })?;
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
        stdin,
        lines,
    })
}

/// 読み捨てる応答に含まれる共有ハンドルの数。このホストは CPU の画像だけを返すので、
/// 共有テクスチャが来たらプロトコルの外の応答として漏れに数える。
fn unclosed(response: &HostResponse) -> usize {
    match response {
        HostResponse::Frame(SharedFrame::SharedTexture { fence, .. }) => {
            1 + usize::from(fence.is_some())
        }
        _ => 0,
    }
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
                        what: "host_ofx.request".into(),
                        limit: timeout,
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // 出力が閉じたのはプロセスが終わる途中なので、終わるまで待って is_alive を偽にそろえる。
                    self.kill()?;
                    return Err(MmError::HostCrashed("ホストの出力が閉じた".into()));
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
            self.open += unclosed(&response);
        }
    }

    fn kill(&mut self) -> Result<()> {
        if let Some(conn) = self.conn.take() {
            let mut child = conn.child.lock().unwrap_or_else(|e| e.into_inner());
            // 既に終わっていれば kill は失敗するが、wait で後片付けだけする。
            let _ = child.kill();
            child.wait()?;
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

impl Drop for PipeHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 実行ファイルが無ければ起動の失敗を返す() {
        let spec = HostSpec {
            executable: "mm-host-ofx-が無い場所/none.exe".into(),
            args: Vec::new(),
            default_timeout: Duration::from_secs(1),
            gpu: None,
            client_pid: None,
        };
        let err = OfxLauncher.spawn(&spec).err().expect("起動できないはず");
        assert!(matches!(err, MmError::HostCrashed(_)), "{err:?}");
    }

    #[test]
    fn cpu_の画像の応答は漏れに数えない() {
        let frame = SharedFrame::Cpu {
            size: mm_types::Size::new(1, 1),
            data: vec![0.0; 4],
        };
        assert_eq!(unclosed(&HostResponse::Frame(frame)), 0);
        assert_eq!(unclosed(&HostResponse::Prepared), 0);
    }
}
