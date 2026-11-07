//! `mm render`: レンダーターゲットを書き出す。Ctrl+C で取り消し（一時ファイルを残さない）、進み具合を stderr に出し、
//! 書き出した MP4 を ffprobe で検証する。

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use mm_export::{
    export_targets, ExportBatch, ExportControl, ExportOptions, ExportProgress, ExportSummary,
};
use mm_runtime::{assemble, RuntimeConfig};
use mm_types::{Id, MmError, RationalTime, Result, TimeRange};

use crate::captions_cmd;
use crate::project::load_project;

/// `mm render`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// プロジェクトのレンダーターゲットを MP4 に書き出す。Ctrl+C で取り消せる。
    Render(RenderArgs),
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Render(args) => render(&args),
        }
    }
}

/// 取り消したときの終了コード（シェルの慣例で 128 + SIGINT）。
const EXIT_CANCELLED: i32 = 130;
/// 進み具合の表示の間隔（ミリ秒）。
const PROGRESS_INTERVAL_MS: u64 = 250;
/// 秒の小数を有理数にするときの分母（マイクロ秒）。
const MICROS: u32 = 1_000_000;

/// `mm render` の引数。
#[derive(clap::Args)]
pub struct RenderArgs {
    /// プロジェクトの JSON、または `project.mmdb` のあるフォルダー。
    pub project: PathBuf,
    /// 出力先。省くとプロジェクトのデータの output_path。レンダーターゲットが 1 つのときだけ使える。
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// レンダーターゲットの ID（繰り返してよい）。省くと最初の 1 つ。
    #[arg(long)]
    pub target: Vec<String>,
    /// 全部のレンダーターゲットを書き出す。
    #[arg(long, conflicts_with = "target")]
    pub all: bool,
    /// レンダーターゲットのコンポジションの代わりに、この sequence を全長で書き出す。
    #[arg(long)]
    pub sequence: Option<String>,
    /// 書き出す区間（秒）。`開始:終了` の形で、コンポジションの範囲に収める。
    #[arg(long, value_parser = parse_range)]
    pub range: Option<TimeRange>,
    /// レンダーターゲットのコーデックを上書きする（`h264`、`h264_2pass`、`h264_nvenc`、`hevc_nvenc`、
    /// `prores_422hq`、`prores_4444`、`dnxhr_hq` など）。NVIDIA の GPU が無いと `h264_nvenc` は libx264 で書く。
    #[arg(long)]
    pub codec: Option<String>,
    /// 並行に描くレンダラーの数。1 は並行にしない。
    #[arg(long, default_value_t = 1)]
    pub parallel: usize,
    /// 書き出したあとの ffprobe の検証を省く。
    #[arg(long)]
    pub no_verify: bool,
}

/// `--range 1.5:4` を区間にする。
pub fn parse_range(text: &str) -> std::result::Result<TimeRange, String> {
    let (a, b) = text.split_once(':').ok_or("開始:終了 の形（秒）")?;
    let secs = |s: &str| -> std::result::Result<RationalTime, String> {
        let v: f64 = s.trim().parse().map_err(|_| format!("秒の数でない: {s}"))?;
        RationalTime::new((v * f64::from(MICROS)).round() as i128, MICROS)
            .map_err(|e| e.to_string())
    };
    TimeRange::new(secs(a)?, secs(b)?).map_err(|e| e.to_string())
}

/// 書き出して結果を表示する。取り消されたら終了コード 130 で終わる。
pub fn render(args: &RenderArgs) -> Result<()> {
    let project = load_project(&args.project)?;
    let targets = pick_targets(&project, args)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let handler_flag = cancel.clone();
    ctrlc::set_handler(move || handler_flag.store(true, Ordering::Relaxed))
        .map_err(|e| MmError::Internal(format!("Ctrl+C のハンドラーを作れない: {e}")))?;
    let mut control = ExportControl::default().with_cancel(cancel);
    if std::io::stderr().is_terminal() {
        control = control.with_progress(progress_printer());
    }
    let options = ExportOptions {
        output: args.output.clone(),
        range: args.range,
        sequence: args.sequence.clone().map(Id::new),
        codec: args.codec.clone(),
        parallel: args.parallel,
        transcript: needs_transcript(&project, &targets)
            .then(|| captions_cmd::transcript_words(&args.project, &project))
            .transpose()?
            .map(Arc::new),
        verify: !args.no_verify,
        control,
        ..ExportOptions::default()
    };
    let mut engine = assemble(&RuntimeConfig::real())?;
    let batch = export_targets(&mut engine, &project, &targets, &options)?;
    clear_progress_line();
    report(&batch);
    if batch.cancelled {
        eprintln!("取り消した。一時ファイルは消した");
        std::process::exit(EXIT_CANCELLED);
    }
    Ok(())
}

/// 書き出すレンダーターゲットのどれかが字幕を出すか焼き込む設定か（DB から単語を読む要るか）。
fn needs_transcript(project: &mm_doc::Project, targets: &[mm_types::RenderTargetId]) -> bool {
    targets.iter().any(|id| {
        project
            .render_target(id)
            .is_ok_and(|t| t.delivery.needs_transcript())
    })
}

fn pick_targets(
    project: &mm_doc::Project,
    args: &RenderArgs,
) -> Result<Vec<mm_types::RenderTargetId>> {
    if args.all {
        return Ok(project
            .render_targets
            .iter()
            .map(|t| t.id.clone())
            .collect());
    }
    if args.target.is_empty() {
        return mm_export::pick_target(project, None).map(|t| vec![t]);
    }
    args.target
        .iter()
        .map(|t| mm_export::pick_target(project, Some(&Id::new(t.as_str()))))
        .collect()
}

/// 同じ行を書き換える進み具合の表示。間隔を空けて、最後のフレームは必ず出す。
fn progress_printer() -> mm_export::ProgressFn {
    let started = Instant::now();
    let last_ms = AtomicU64::new(0);
    Arc::new(move |p: &ExportProgress| {
        let now = started.elapsed().as_millis() as u64;
        let finished = p.frames_done == p.frames_total;
        if !finished && now.saturating_sub(last_ms.load(Ordering::Relaxed)) < PROGRESS_INTERVAL_MS {
            return;
        }
        last_ms.store(now, Ordering::Relaxed);
        let percent = p.frames_done * 100 / p.frames_total.max(1);
        let fps = p.frames_done as f64 / p.elapsed.as_secs_f64().max(f64::EPSILON);
        eprint!(
            "\r{}: {}/{} フレーム（{percent}%、{fps:.1} fps）",
            p.target, p.frames_done, p.frames_total
        );
        let _ = std::io::stderr().flush();
    })
}

fn clear_progress_line() {
    if std::io::stderr().is_terminal() {
        eprint!("\r\x1b[2K");
    }
}

fn report(batch: &ExportBatch) {
    for s in &batch.done {
        print_summary(s);
    }
}

fn print_summary(s: &ExportSummary) {
    println!(
        "{}: {} フレーム、音 {} サンプル、{} バイト、{:.2} 秒（{:.1} fps）",
        s.output_path.display(),
        s.frames,
        s.audio_frames,
        s.bytes_written,
        s.elapsed.as_secs_f64(),
        s.fps
    );
    if let Some(encoder) = &s.encoder {
        println!("  エンコーダー: {encoder}");
    }
    if let Some(v) = &s.verification {
        println!(
            "  検証 OK: {} {}x{}、映像 {} フレーム {:.3} 秒、音 {}",
            v.video_codec,
            v.size.width,
            v.size.height,
            v.video_frames,
            v.video_seconds,
            v.audio_seconds
                .map_or_else(|| "なし".to_owned(), |a| format!("{a:.3} 秒"))
        );
    }
    print_delivery(&s.delivery);
}

fn print_delivery(d: &mm_export::DeliveryReport) {
    if let Some(n) = &d.normalization {
        match (n.measured_lufs, n.final_lufs) {
            (Some(before), Some(after)) => println!(
                "  ラウドネス: {before:.1} LUFS -> {after:.1} LUFS（{:+.1} dB、トゥルーピーク {:.1} dBTP{}）",
                n.gain_db,
                n.true_peak_dbtp,
                if n.limited { "、リミッター" } else { "" }
            ),
            _ => println!("  ラウドネス: 無音で測れず、音は変えていない"),
        }
    }
    for f in d.subtitle_files.iter().chain(&d.chapters_file) {
        println!("  {}", f.display());
    }
    for w in &d.warnings {
        eprintln!("  警告: {w}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 区間は秒の小数を開始と終了にする() {
        let r = parse_range("1.5:4").unwrap();
        assert_eq!(r.start, RationalTime::new(3, 2).unwrap());
        assert_eq!(r.end, RationalTime::seconds(4));
        assert!(parse_range("4:1").is_err());
        assert!(parse_range("4").is_err());
        assert!(parse_range("a:b").is_err());
    }
}
