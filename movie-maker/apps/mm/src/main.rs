//! `mm` コマンド。
//!
//! - `mm render <project> -o <out.mp4>`: レンダーターゲットを書き出す。
//! - `mm perf run <場面>`: 性能の場面を回して予算と比べる（`startup`、`export`、`all`）。

mod perf;
mod project;
mod scenes;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mm_export::{export_project, ExportOptions};
use mm_runtime::{assemble, RuntimeConfig};
use mm_trace::MetricsLayer;
use mm_types::{Id, MmError, Result};
use tracing_subscriber::prelude::*;

use crate::perf::{PerfArgs, Scene};
use crate::project::load_project;

/// 見本のプロジェクト。`--project` を省くと、作業フォルダー（`movie-maker/`）基準の位置から探す。
/// ビルドした場所の絶対パスを埋め込まないので、別の場所に置いた実行ファイルでも同じ規則で動く。
fn default_project() -> PathBuf {
    PathBuf::from("fixtures/projects/stage0/project.json")
}

#[derive(Parser)]
#[command(name = "mm", about = "movie-maker の CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// プロジェクトのレンダーターゲットを MP4 に書き出す。
    Render {
        /// プロジェクトの JSON。
        project: PathBuf,
        /// 出力先。省くと文書の output_path。
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// レンダーターゲットの ID。省くと最初の 1 つ。
        #[arg(long)]
        target: Option<String>,
    },
    /// 性能の計測。
    Perf {
        #[command(subcommand)]
        command: PerfCommand,
    },
    /// `perf run startup` が起動する子プロセス。
    #[command(hide = true)]
    ProbeStartup { project: PathBuf },
}

#[derive(Subcommand)]
enum PerfCommand {
    /// 場面を回し、結果を JSON と metrics.jsonl に書く。
    Run {
        scene: Scene,
        #[arg(long, default_value_t = 5)]
        repeat: usize,
        /// 結果の置き場所。
        #[arg(long, default_value = ".mm/perf")]
        out_dir: PathBuf,
        /// 場面に使うプロジェクト。
        #[arg(long)]
        project: Option<PathBuf>,
        /// export の描画の大きさ（WxH）。
        #[arg(long, default_value = "1920x1080", value_parser = scenes::parse_size)]
        size: (u32, u32),
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("エラー: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Render {
            project,
            output,
            target,
        } => render(&project, output, target),
        Command::Perf {
            command:
                PerfCommand::Run {
                    scene,
                    repeat,
                    out_dir,
                    project,
                    size,
                },
        } => {
            let (layer, metrics) = MetricsLayer::new();
            tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
                .map_err(|e| MmError::Internal(format!("購読者を置けない: {e}")))?;
            let args = PerfArgs {
                scene,
                repeat,
                out_dir,
                project: project.unwrap_or_else(default_project),
                size,
            };
            perf::run(&args, &metrics)
        }
        Command::ProbeStartup { project } => scenes::probe_child(&project),
    }
}

fn render(
    project: &std::path::Path,
    output: Option<PathBuf>,
    target: Option<String>,
) -> Result<()> {
    let project = load_project(project)?;
    let mut engine = assemble(&RuntimeConfig::real())?;
    let summary = export_project(
        &mut engine,
        &project,
        &ExportOptions {
            output,
            target: target.map(Id::new),
        },
    )?;
    println!(
        "{}: {} コマ、音 {} サンプル、{} バイト、{:.2} 秒（{:.1} fps）",
        summary.output_path.display(),
        summary.frames,
        summary.audio_frames,
        summary.bytes_written,
        summary.elapsed.as_secs_f64(),
        summary.fps
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 既定のプロジェクトはビルドした場所の絶対パスでない() {
        assert!(default_project().is_relative());
    }
}
