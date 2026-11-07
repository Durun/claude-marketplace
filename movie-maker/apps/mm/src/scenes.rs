//! 性能のシナリオ。`startup` は別プロセスを起動して測る（`export` は scenes_export.rs）。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mm_node_api::Quality;
use mm_runtime::{assemble, RuntimeConfig};
use mm_types::{CompositionId, MmError, RationalTime, Result, Size};

use crate::perf::{Budget, Item, Limit, PerfArgs};
use crate::project::load_project;

/// 内部のサブコマンド。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// `perf run startup` が起動する子プロセス。
    #[command(hide = true)]
    ProbeStartup { project: std::path::PathBuf },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::ProbeStartup { project } => probe_child(&project),
        }
    }
}

/// 子プロセスが「入力を受け付けられる」ときに標準出力へ出す行。
pub const READY_LINE: &str = "ready";
/// 子プロセスが最初のフレームを描き終えたときの行。
pub const FIRST_FRAME_LINE: &str = "first_frame";

const STARTUP_ROW: &str = "起動";
const MS_PER_SECOND: f64 = 1000.0;
/// 起動の測定の子プロセス 1 回に待つ上限。予算（2 秒）の桁を大きく超えたら固まったとみなす。
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// `startup`: 起動から入力を受け付けるまでと、プロジェクトを開いてから最初のフレームまで。
pub fn startup(args: &PerfArgs) -> Result<Vec<Item>> {
    let exe = std::env::current_exe()?;
    let (mut ready, mut first) = (Vec::new(), Vec::new());
    for _ in 0..args.repeat {
        let (r, f) = probe_once(&exe, &args.project)?;
        ready.push(r);
        first.push(f);
    }
    let budget = |ms| {
        Some(Budget {
            limit: Limit::AtMost(ms),
            row: STARTUP_ROW,
        })
    };
    Ok(vec![
        Item::from_samples("startup.ready_ms", "ms", ready, budget(2000.0)),
        Item::from_samples("startup.first_frame_ms", "ms", first, budget(1000.0)),
    ])
}

/// 子プロセスを 1 回起動し、(起動から ready まで, ready から最初のフレームまで) をミリ秒で返す。
fn probe_once(exe: &std::path::Path, project: &std::path::Path) -> Result<(f64, f64)> {
    let mut command = Command::new(exe);
    command.arg("probe-startup").arg(project);
    run_probe(command, PROBE_TIMEOUT)
}

/// 子を起動して行を読む。`timeout` までに標準出力が閉じなければ子を落として `Timeout`。
/// 行は別スレッドで読み、固まった子の読み取りで親が止まらないようにする。
fn run_probe(mut command: Command, timeout: Duration) -> Result<(f64, f64)> {
    let started = Instant::now();
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| MmError::Internal("子の標準出力が取れない".into()))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            // 親が時間切れで先に抜けていたら送れないだけ。
            let _ = tx.send((line, started.elapsed()));
        }
    });
    let (mut ready, mut first) = (None, None);
    loop {
        let Some(left) = timeout.checked_sub(started.elapsed()) else {
            return Err(kill_on_timeout(&mut child, timeout));
        };
        match rx.recv_timeout(left) {
            Ok((line, at)) => match line.as_str() {
                READY_LINE => ready = Some(at.as_secs_f64() * MS_PER_SECOND),
                FIRST_FRAME_LINE => first = Some(at.as_secs_f64() * MS_PER_SECOND),
                _ => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(kill_on_timeout(&mut child, timeout));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = wait_within(
        &mut child,
        timeout.saturating_sub(started.elapsed()),
        timeout,
    )?;
    match (status.success(), ready, first) {
        (true, Some(r), Some(f)) => Ok((r, f - r)),
        _ => Err(MmError::Internal(format!(
            "起動の測定の子プロセスが失敗した（{status}）。mm render が通るか確かめる"
        ))),
    }
}

/// 標準出力を閉じた子の終了を `left` まで待つ。過ぎたら落として `Timeout`（上限は `limit`）。
fn wait_within(
    child: &mut std::process::Child,
    left: Duration,
    limit: Duration,
) -> Result<std::process::ExitStatus> {
    const POLL: Duration = Duration::from_millis(10);
    let deadline = Instant::now() + left;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(kill_on_timeout(child, limit));
        }
        std::thread::sleep(POLL);
    }
}

/// 子を落として待ち、時間切れのエラーにする。落とせなかったときも測定は失敗として返す。
fn kill_on_timeout(child: &mut std::process::Child, limit: Duration) -> MmError {
    // 既に終わっていて kill が失敗しても、時間切れの報告は変わらない。
    let _ = child.kill();
    let _ = child.wait();
    MmError::Timeout {
        what: "起動の測定の子プロセス".into(),
        limit,
    }
}

/// 子プロセス側（`mm probe-startup`）。ready を出し、プロジェクトを開いて最初のフレームを描く。
pub fn probe_child(project: &std::path::Path) -> Result<()> {
    let mut engine = assemble(&RuntimeConfig::real())?;
    println!("{READY_LINE}");
    flush_stdout();
    let project = load_project(project)?;
    let comp = project
        .compositions
        .first()
        .ok_or_else(|| MmError::NotFound("コンポジションが無い".into()))?;
    let id: CompositionId = comp.id.clone();
    engine.render_frame(
        &project,
        &id,
        RationalTime::ZERO,
        Size::new(comp.width, comp.height),
        Quality::Full,
    )?;
    println!("{FIRST_FRAME_LINE}");
    flush_stdout();
    Ok(())
}

fn flush_stdout() {
    use std::io::Write;
    // 親が行を待っているので即座に出す。失敗しても親が打ち切りとして扱う。
    let _ = std::io::stdout().flush();
}

/// `1920x1080` のような大きさを読む。
pub fn parse_size(s: &str) -> std::result::Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("{s} は WxH の形ではない"))?;
    let num = |v: &str| v.parse::<u32>().map_err(|e| format!("{v}: {e}"));
    let (w, h) = (num(w)?, num(h)?);
    if w == 0 || h == 0 || w % 2 == 1 || h % 2 == 1 {
        return Err("幅と高さは 2 以上の偶数".into());
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD_MODE: &str = "MM_TEST_PROBE_CHILD";

    /// 子として呼ばれたときだけ働く。親のテストが現在の実行ファイルをこのテストだけで再実行する。
    #[test]
    fn probe_child_helper() {
        match std::env::var(CHILD_MODE).as_deref() {
            Ok("ok") => {
                println!("{READY_LINE}");
                println!("{FIRST_FRAME_LINE}");
            }
            Ok("hang") => {
                println!("{READY_LINE}");
                flush_stdout();
                std::thread::sleep(Duration::from_secs(600));
            }
            _ => {}
        }
    }

    fn child(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("テストの実行ファイル"));
        command
            .args([
                "--exact",
                "scenes::tests::probe_child_helper",
                "--nocapture",
            ])
            .env(CHILD_MODE, mode);
        command
    }

    #[test]
    fn 子が_ready_と_first_frame_を出せば測れる() {
        let (ready, first) = run_probe(child("ok"), Duration::from_secs(60)).unwrap();
        assert!(ready > 0.0 && first >= 0.0);
    }

    #[test]
    fn 子が固まったら時間切れで落とす() {
        let started = Instant::now();
        let result = run_probe(child("hang"), Duration::from_secs(1));
        assert!(matches!(result, Err(MmError::Timeout { .. })), "{result:?}");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "親が待ち続けない"
        );
    }
}
