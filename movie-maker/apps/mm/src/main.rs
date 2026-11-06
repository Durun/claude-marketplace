//! `mm` コマンド。
//!
//! - `mm new` / `import` / `export` / `op` / `undo` / `redo` / `log` / `diff`: `project.mmdb` のフォルダーを扱う（commands.rs）。
//! - `mm ingest <dir> <素材>...`: 素材を取り込み、`cache/` に出力を作って素材を足す（ingest_cmd.rs）。
//! - `mm search <dir> <語>`: 文字起こしから語を引き、素材と時刻を出す（transcript_cmd.rs）。
//! - `mm render <project> -o <out.mp4>`: レンダーターゲットを書き出す。project は JSON か `project.mmdb` のフォルダー。
//! - `mm perf run <シナリオ>`: 性能のシナリオを回して予算と比べる（`startup`、`export`、`playback`、`scrub`、`cpu`、`all`）。
//! - `mm perf compare <前> <後>` と `mm perf budget <結果>`: 結果の比較と、予算の超過の一覧（compare.rs）。

mod commands;
mod compare;
mod describe;
mod ingest_cmd;
mod opargs;
mod perf;
mod project;
mod scenes;
mod scenes_cpu;
mod scenes_render;
mod transcript_cmd;

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

/// フィクスチャのプロジェクト。`--project` を省くと、作業フォルダー（`movie-maker/`）基準の位置から探す。
/// ビルドした場所の絶対パスを埋め込まないので、別の場所に置いた実行ファイルでも同じ規則で動く。
fn default_project(scene: Scene) -> PathBuf {
    PathBuf::from(scene.default_project())
}

#[derive(Parser)]
#[command(name = "mm", about = "movie-maker の CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 空のプロジェクト（`project.mmdb`）を作る。
    New { dir: PathBuf },
    /// `versions/` のフォルダーか、1 つの JSON から `project.mmdb` を作る。
    Import {
        dir: PathBuf,
        /// `versions/` のフォルダー、または 1 つのプロジェクトの JSON。
        source: PathBuf,
        /// すでにある `project.mmdb` を（バックアップを `backups/` に残して）置き換える。
        #[arg(long)]
        overwrite: bool,
    },
    /// `versions/` に決定的な JSON を書く。
    Export {
        dir: PathBuf,
        /// 名前付きバージョンとして残す。
        #[arg(long)]
        name: Option<String>,
    },
    /// 操作を適用する。`mm op <dir> ripple --cut c12 --edge out --by -0.4`。
    ///
    /// 引数は操作ごとのフィールドの名前（`mm op --schema` が一覧）。複雑なフィールドは `--json '{...}'`、
    /// 元にしたバージョンは `--base <バージョン>`、記録の名前は `--label <名前>`。
    Op {
        dir: Option<PathBuf>,
        /// 操作の名前（addLayer、ripple、setParam など）。
        op_name: Option<String>,
        /// 全部の操作の JSON Schema を出す。
        #[arg(long)]
        schema: bool,
        /// 複数の操作を 1 つのトランザクションとして適用する（`Transaction` の JSON）。
        #[arg(long)]
        txn: Option<PathBuf>,
        /// 操作の引数（`--フィールド 値` の並び）。
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// 直前のトランザクションを取り消す。
    Undo { dir: PathBuf },
    /// 取り消したトランザクションをやり直す。
    Redo { dir: PathBuf },
    /// 操作の記録を出す。
    Log { dir: PathBuf },
    /// 2 つのバージョンの間の意味の差分を出す。バージョンは番号、`head`、名前付きバージョンの名前。
    Diff {
        dir: PathBuf,
        from: String,
        to: String,
    },
    /// プロジェクトのレンダーターゲットを MP4 に書き出す。
    Render {
        /// プロジェクトの JSON、または `project.mmdb` のあるフォルダー。
        project: PathBuf,
        /// 出力先。省くとプロジェクトのデータの output_path。
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// レンダーターゲットの ID。省くと最初の 1 つ。
        #[arg(long)]
        target: Option<String>,
    },
    /// 素材を取り込む。`cache/` にプレビュー動画・波形・サムネイルを作り、素材を足す操作を適用する。
    Ingest {
        dir: PathBuf,
        /// 取り込む素材のパス（1 つ以上）。
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// プレビュー動画のエンコーダー（auto、nvenc、x264）。
        #[arg(long, default_value = "auto", value_parser = ingest_cmd::parse_encoder)]
        encoder: mm_ingest::PreviewEncoder,
    },
    /// 文字起こしから語を引く。1 行に 1 件（素材、始まりと終わりの秒、単語の ID、続きの文字）。
    Search {
        dir: PathBuf,
        query: String,
        #[arg(long, default_value_t = transcript_cmd::DEFAULT_LIMIT)]
        limit: usize,
        /// 1 行に 1 件の JSON（時刻は有理数）で出す。
        #[arg(long)]
        json: bool,
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
    /// シナリオを回し、結果を JSON と metrics.jsonl に書く。
    Run {
        scene: Scene,
        #[arg(long, default_value_t = 5)]
        repeat: usize,
        /// 結果の置き場所。
        #[arg(long, default_value = ".mm/perf")]
        out_dir: PathBuf,
        /// シナリオに使うプロジェクト。
        #[arg(long)]
        project: Option<PathBuf>,
        /// export、playback、scrub の描画の大きさ（WxH）。
        #[arg(long, default_value = "1920x1080", value_parser = scenes::parse_size)]
        size: (u32, u32),
        /// playback で再生する秒数。
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        /// scrub のシーク回数（繰り返し 1 回あたり）。
        #[arg(long, default_value_t = 100)]
        seeks: usize,
        /// 結果を `<シナリオ>.json` の固定の名前で書く（ベースラインの更新用）。
        #[arg(long)]
        baseline: bool,
        /// 描画のシナリオ（playback、scrub）で使う GPU を、名前の一部（大小文字を区別しない）で選ぶ。
        /// CI は `Microsoft Basic Render Driver`（WARP、D3D12 のソフトウェアの GPU）を指す。
        #[arg(long)]
        adapter: Option<String>,
    },
    /// 2 つの結果（JSON かそのフォルダー）を比べ、閾値を超えて遅くなった項目があれば失敗する。
    Compare {
        before: PathBuf,
        after: PathBuf,
        /// 遅くなったとみなす割合（5%、0.3 など）。
        #[arg(long, default_value = "10%", value_parser = compare::parse_threshold)]
        threshold: f64,
        /// この絶対値より小さい差は無視する（項目の単位）。
        #[arg(long, default_value_t = 0.0)]
        min_delta: f64,
    },
    /// 結果（JSON かそのフォルダー）の項目を予算と並べる。
    Budget {
        results: PathBuf,
        /// 超えた項目だけを JSON で出す（起票の入力）。
        #[arg(long)]
        json: bool,
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
        Command::Perf { command } => run_perf(command),
        Command::ProbeStartup { project } => scenes::probe_child(&project),
        other => run_store_command(other),
    }
}

/// `project.mmdb` のフォルダーを扱うサブコマンド。
fn run_store_command(command: Command) -> Result<()> {
    match command {
        Command::New { dir } => commands::new_project(&dir),
        Command::Import {
            dir,
            source,
            overwrite,
        } => commands::import(&dir, &source, overwrite),
        Command::Export { dir, name } => commands::export(&dir, name.as_deref()),
        Command::Op { schema: true, .. } => {
            commands::schema();
            Ok(())
        }
        Command::Op {
            dir,
            op_name,
            txn,
            args,
            ..
        } => {
            let dir = dir
                .ok_or_else(|| MmError::InvalidArgument("プロジェクトのフォルダーを渡す".into()))?;
            commands::op(&dir, op_name.as_deref(), &args, txn.as_deref())
        }
        Command::Ingest {
            dir,
            inputs,
            encoder,
        } => ingest_cmd::ingest(&ingest_cmd::IngestArgs {
            dir,
            inputs,
            encoder,
        }),
        Command::Search {
            dir,
            query,
            limit,
            json,
        } => transcript_cmd::search(&dir, &query, limit, json),
        Command::Undo { dir } => commands::undo(&dir),
        Command::Redo { dir } => commands::redo(&dir),
        Command::Log { dir } => commands::log(&dir),
        Command::Diff { dir, from, to } => commands::diff(&dir, &from, &to),
        other => Err(MmError::Internal(format!(
            "保存のサブコマンドでない: {}",
            command_name(&other)
        ))),
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Render { .. } => "render",
        Command::Perf { .. } => "perf",
        Command::ProbeStartup { .. } => "probe-startup",
        _ => "store",
    }
}

fn run_perf(command: PerfCommand) -> Result<()> {
    let PerfCommand::Run {
        scene,
        repeat,
        out_dir,
        project,
        size,
        seconds,
        seeks,
        baseline,
        adapter,
    } = command
    else {
        return run_perf_report(command);
    };
    let (layer, metrics) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|e| MmError::Internal(format!("購読者を置けない: {e}")))?;
    let args = PerfArgs {
        scene,
        repeat,
        out_dir,
        project: project.unwrap_or_else(|| default_project(scene)),
        size,
        seconds,
        seeks,
        baseline,
        adapter,
    };
    perf::run(&args, &metrics)
}

/// 結果を読むだけのサブコマンド（`compare`、`budget`）。
fn run_perf_report(command: PerfCommand) -> Result<()> {
    match command {
        PerfCommand::Compare {
            before,
            after,
            threshold,
            min_delta,
        } => {
            let options = compare::CompareOptions {
                threshold,
                min_delta,
            };
            if !compare::run_compare(&before, &after, options)? {
                // 回帰は使い方の誤りではないので、エラーの文言は出さず終了コードだけで知らせる。
                std::process::exit(1);
            }
            Ok(())
        }
        PerfCommand::Budget { results, json } => compare::run_budget(&results, json),
        PerfCommand::Run { .. } => Err(MmError::Internal("run は別の経路".into())),
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
        "{}: {} フレーム、音 {} サンプル、{} バイト、{:.2} 秒（{:.1} fps）",
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
        assert!(default_project(Scene::Export).is_relative());
    }
}
