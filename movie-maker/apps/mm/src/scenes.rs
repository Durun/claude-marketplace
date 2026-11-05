//! 性能の場面。`startup` は別プロセスを起動して測り、`export` は同じプロセスで書き出しを繰り返す。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Instant;

use mm_export::{export_project, ExportOptions};
use mm_node_api::Quality;
use mm_runtime::{assemble, RuntimeConfig};
use mm_types::{CompositionId, MmError, RationalTime, Result, Size};

use crate::perf::{Budget, Item, Limit, PerfArgs};
use crate::project::load_project;

/// 子プロセスが「入力を受け付けられる」ときに標準出力へ出す行。
pub const READY_LINE: &str = "ready";
/// 子プロセスが最初のコマを描き終えたときの行。
pub const FIRST_FRAME_LINE: &str = "first_frame";

const STARTUP_ROW: &str = "起動";
const EXPORT_ROW: &str = "書き出し（1080p60、NVENC、1 プロセス）";
const MS_PER_SECOND: f64 = 1000.0;

/// `startup`: 起動から入力を受け付けるまでと、プロジェクトを開いてから最初のコマまで。
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

/// 子プロセスを 1 回起動し、(起動から ready まで, ready から最初のコマまで) をミリ秒で返す。
fn probe_once(exe: &std::path::Path, project: &std::path::Path) -> Result<(f64, f64)> {
    let started = Instant::now();
    let mut child = Command::new(exe)
        .arg("probe-startup")
        .arg(project)
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| MmError::Internal("子の標準出力が取れない".into()))?;
    let (mut ready, mut first) = (None, None);
    for line in BufReader::new(stdout).lines() {
        match line?.as_str() {
            READY_LINE => ready = Some(started.elapsed().as_secs_f64() * MS_PER_SECOND),
            FIRST_FRAME_LINE => first = Some(started.elapsed().as_secs_f64() * MS_PER_SECOND),
            _ => {}
        }
    }
    let status = child.wait()?;
    match (status.success(), ready, first) {
        (true, Some(r), Some(f)) => Ok((r, f - r)),
        _ => Err(MmError::Internal(format!(
            "起動の測定の子プロセスが失敗した（{status}）。mm render が通るか確かめる"
        ))),
    }
}

/// 子プロセス側（`mm probe-startup`）。ready を出し、プロジェクトを開いて最初のコマを描く。
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

/// `export`: 見本のプロジェクトを `--size` で書き出し、速さを測る。
pub fn export(args: &PerfArgs) -> Result<Vec<Item>> {
    let mut project = load_project(&args.project)?;
    for comp in &mut project.compositions {
        (comp.width, comp.height) = args.size;
    }
    for target in &mut project.render_targets {
        (target.width, target.height) = args.size;
    }
    let mut engine = assemble(&RuntimeConfig::real())?;
    let work = args.out_dir.join("export-work");
    let (mut fps, mut elapsed_ms) = (Vec::new(), Vec::new());
    for i in 0..args.repeat {
        let output = work.join(format!("run{i}.mp4"));
        let summary = export_project(
            &mut engine,
            &project,
            &ExportOptions {
                output: Some(output.clone()),
                target: None,
            },
        )?;
        fps.push(summary.fps);
        elapsed_ms.push(summary.elapsed.as_secs_f64() * MS_PER_SECOND);
        std::fs::remove_file(&output)?;
    }
    let cold = fps.first().copied().unwrap_or(0.0);
    let budget = Some(Budget {
        limit: Limit::AtLeast(120.0),
        row: EXPORT_ROW,
    });
    Ok(vec![
        Item::from_samples("export.fps", "fps", fps, budget),
        Item::from_samples("export.fps_cold", "fps", vec![cold], None),
        Item::from_samples("export.elapsed_ms", "ms", elapsed_ms, None),
    ])
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
