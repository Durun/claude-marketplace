//! `mm` コマンド。
//!
//! - `mm new` / `import` / `export` / `op` / `undo` / `redo` / `log` / `diff`: `project.mmdb` のフォルダーを扱う（commands.rs）。
//! - `mm ingest <dir> <素材>...`: 素材を取り込み、`cache/` に出力を作って素材を足す（ingest_cmd.rs）。
//! - `mm search <dir> <語>`: 文字起こしから語を引き、素材と時刻を出す（transcript_cmd.rs）。
//! - `mm render <project> -o <out.mp4>`: レンダーターゲットを書き出す。project は JSON か `project.mmdb` のフォルダー。
//! - `mm frame <project> <t> -o x.png`: 1 フレームを PNG に書く。アプリが動いていれば HTTP でアプリに描いてもらう（remote.rs）。
//! - `mm geneva <timeline.json> -o <project.json>`: geneva のタイムラインを取り込む（geneva_cmd.rs）。
//! - `mm otio export <project> -o x.otio` と `mm otio import <dir> x.otio`: sequence と OTIO の変換（otio_cmd.rs）。
//! - `mm pack check / list / expand / apply`: パックの検査、マクロの一覧、マクロを展開して適用する（pack_cmd.rs）。
//! - `mm perf run <シナリオ>`: 性能のシナリオを回して予算と比べる（`startup`、`export`、`playback`、`scrub`、`cpu`、`all`）。
//! - `mm perf compare <前> <後>` と `mm perf budget <結果>`: 結果の比較と、予算の超過の一覧（compare.rs）。
//!
//! サブコマンドは領域のファイルが `Cmd`（clap の `Subcommand`）と `run` を持ち、このファイルは `Command` に 1 行ずつ並べる。

mod captions_cmd;
mod commands;
mod compare;
mod describe;
mod geneva_cmd;
mod ingest_cmd;
mod opargs;
mod otio_cmd;
mod pack_cmd;
mod perf;
mod project;
mod proposal_cmd;
mod remote;
mod render_cmd;
mod scenes;
mod scenes_cpu;
mod scenes_export;
mod scenes_render;
mod shorts_cmd;
mod transcript_cmd;

use std::process::ExitCode;

use clap::Parser;
use mm_types::Result;

#[derive(Parser)]
#[command(name = "mm", about = "movie-maker の CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// 上の段のサブコマンド。`--help` の並びはこの順。
#[derive(clap::Subcommand)]
enum Command {
    #[command(flatten)]
    Store(commands::Cmd),
    #[command(flatten)]
    Render(render_cmd::Cmd),
    #[command(flatten)]
    Subtitles(captions_cmd::Cmd),
    #[command(flatten)]
    Ingest(ingest_cmd::Cmd),
    #[command(flatten)]
    Frame(remote::Cmd),
    #[command(flatten)]
    Search(transcript_cmd::Cmd),
    #[command(flatten)]
    Shorts(shorts_cmd::Cmd),
    #[command(flatten)]
    Proposal(proposal_cmd::Cmd),
    #[command(flatten)]
    Geneva(geneva_cmd::Cmd),
    #[command(flatten)]
    Otio(otio_cmd::Cmd),
    #[command(flatten)]
    Pack(pack_cmd::Cmd),
    #[command(flatten)]
    Perf(perf::Cmd),
    #[command(flatten)]
    ProbeStartup(scenes::Cmd),
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
        Command::Store(c) => c.run(),
        Command::Render(c) => c.run(),
        Command::Subtitles(c) => c.run(),
        Command::Ingest(c) => c.run(),
        Command::Frame(c) => c.run(),
        Command::Search(c) => c.run(),
        Command::Shorts(c) => c.run(),
        Command::Proposal(c) => c.run(),
        Command::Geneva(c) => c.run(),
        Command::Otio(c) => c.run(),
        Command::Pack(c) => c.run(),
        Command::Perf(c) => c.run(),
        Command::ProbeStartup(c) => c.run(),
    }
}
