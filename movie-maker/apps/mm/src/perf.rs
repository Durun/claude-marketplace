//! `mm perf run <シナリオ>`。シナリオを回して、予算と並べた結果を表と JSON に出す。
//!
//! 結果は `mm-trace` の `metrics.jsonl` の形（1 行 1 集計）と、予算つきの結果の JSON の 2 つ。

use std::borrow::Cow;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use mm_trace::{MetricsHandle, MetricsLayer, MetricsSnapshot};
use mm_types::{MmError, Result};
use serde_json::{json, Value};
use tracing_subscriber::prelude::*;

use crate::{compare, scenes, scenes_cpu, scenes_export, scenes_render};

/// `mm perf`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// 性能の計測。
    Perf {
        #[command(subcommand)]
        command: PerfCommand,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Perf { command } => run_perf(command),
        }
    }
}

/// フィクスチャのプロジェクト。`--project` を省くと、作業フォルダー（`movie-maker/`）基準の位置から探す。
/// ビルドした場所の絶対パスを埋め込まないので、別の場所に置いた実行ファイルでも同じ規則で動く。
fn default_project(scene: Scene) -> PathBuf {
    PathBuf::from(scene.default_project())
}

#[derive(clap::Subcommand)]
pub enum PerfCommand {
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
        /// export のコーデックを上書きする（`h264`、`h264_nvenc`、`prores_422hq` など）。
        #[arg(long)]
        codec: Option<String>,
        /// export で並行に描くレンダラーの数。
        #[arg(long, default_value_t = 1)]
        parallel: usize,
        /// export で、コーデックと並行の数の組を全部測る（libx264、2 パス、NVENC、ProRes を並行なしと 4 本で）。
        #[arg(long)]
        matrix: bool,
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
        codec,
        parallel,
        matrix,
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
        codec,
        parallel,
        matrix,
    };
    run(&args, &metrics)
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

/// 4K（3840x2160）として扱う幅。
const UHD_WIDTH: u32 = 3840;

/// 回せるシナリオ。`cpu` 以外は GPU が要り（`export` と `startup` は FFmpeg の素材も要る）、CI では回せない。
/// `all` は GPU のシナリオを全部と `cpu`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Scene {
    Startup,
    Export,
    Playback,
    Scrub,
    Cpu,
    All,
}

impl Scene {
    /// シナリオの名前（結果のファイル名と `scene` フィールド）。
    pub fn name(self) -> String {
        format!("{self:?}").to_lowercase()
    }

    /// 既定で使うプロジェクト。`startup` は素材つきの 1 本、`export` は 1080p60 の素材を 12 カットに切ったもの、
    /// 描画のシナリオは素材の要らないフィクスチャ。
    pub fn default_project(self) -> &'static str {
        match self {
            Scene::Playback | Scene::Scrub => "fixtures/projects/perf-preview/project.json",
            Scene::Export => "fixtures/projects/perf-export/project.json",
            _ => "fixtures/projects/stage0/project.json",
        }
    }
}

/// 予算の向き。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Limit {
    /// この値以下なら合格。
    AtMost(f64),
    /// この値以上なら合格。
    AtLeast(f64),
}

/// 予算。目標の値と、予算の表（docs/design/17-instrumentation.md）での項目名。
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub limit: Limit,
    pub row: &'static str,
}

/// 1 つの測定項目。`value` は繰り返しの中央値。
#[derive(Debug, Clone)]
pub struct Item {
    pub name: Cow<'static, str>,
    pub unit: &'static str,
    pub value: f64,
    pub samples: Vec<f64>,
    pub budget: Option<Budget>,
}

impl Item {
    /// サンプルの中央値を値にした項目。
    pub fn from_samples(
        name: impl Into<Cow<'static, str>>,
        unit: &'static str,
        samples: Vec<f64>,
        budget: Option<Budget>,
    ) -> Self {
        Self {
            name: name.into(),
            unit,
            value: median(&samples),
            samples,
            budget,
        }
    }

    /// 大きいほど良い項目か（予算が下限か、単位が fps）。比較が遅くなった向きを決める。
    pub fn higher_is_better(&self) -> bool {
        match self.budget {
            Some(b) => matches!(b.limit, Limit::AtLeast(_)),
            None => self.unit == "fps",
        }
    }

    /// 予算に収まっているか。予算が無ければ `None`。
    pub fn within_budget(&self) -> Option<bool> {
        self.budget.map(|b| match b.limit {
            Limit::AtMost(max) => self.value <= max,
            Limit::AtLeast(min) => self.value >= min,
        })
    }
}

/// 中央値。空なら 0。
pub fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    }
}

/// 百分位（`q` は 0〜1）。空なら 0。
pub fn percentile(samples: &[f64], q: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

/// `mm perf run` の引数。
#[derive(Debug, Clone)]
pub struct PerfArgs {
    pub scene: Scene,
    pub repeat: usize,
    pub out_dir: PathBuf,
    pub project: PathBuf,
    /// `export`、`playback`、`scrub` の描画の大きさ（予算は 1080p の値のため既定は 1920x1080）。
    pub size: (u32, u32),
    /// `playback` で再生する秒数。
    pub seconds: f64,
    /// `scrub` のシーク回数（1 回の繰り返しあたり）。
    pub seeks: usize,
    /// 結果を `<シナリオ>.json` の固定の名前で書く（ベースラインの更新用）。
    pub baseline: bool,
    /// 描画のシナリオで使う GPU の名前の一部。`None` は高性能のアダプター。
    pub adapter: Option<String>,
    /// `export` のコーデックの上書き。`None` はプロジェクトのレンダーターゲットのまま。
    pub codec: Option<String>,
    /// `export` で並行に描くレンダラーの数。
    pub parallel: usize,
    /// `export` で、コーデックと並行の数の組（`scenes_export::MATRIX`）を全部測る。
    pub matrix: bool,
}

/// シナリオを回して分かった、測った環境。
#[derive(Debug, Default)]
pub struct Environment {
    /// 使った GPU の名前。`Engine::gpu_adapter` から取る。
    pub adapter: Option<String>,
}

/// シナリオを回して結果を書く。
pub fn run(args: &PerfArgs, metrics: &MetricsHandle) -> Result<()> {
    if args.repeat == 0 {
        return Err(MmError::InvalidArgument("--repeat は 1 以上".into()));
    }
    let mut items = Vec::new();
    let mut env = Environment::default();
    let all = args.scene == Scene::All;
    if all || args.scene == Scene::Startup {
        items.extend(scenes::startup(args)?);
    }
    if all || args.scene == Scene::Export {
        items.extend(scenes_export::export(args, &mut env)?);
    }
    if all || args.scene == Scene::Playback {
        items.extend(scenes_render::playback(args, &mut env, metrics)?);
    }
    if all || args.scene == Scene::Scrub {
        items.extend(scenes_render::scrub(args, &mut env)?);
    }
    if all || args.scene == Scene::Cpu {
        items.extend(scenes_cpu::cpu(args)?);
    }
    let snapshot = metrics.snapshot();
    print_table(&items);
    let path = write_results(args, &items, &env, &snapshot)?;
    println!("結果: {}", path.display());
    Ok(())
}

fn print_table(items: &[Item]) {
    println!(
        "{:<28} {:>12} {:<6} {:<16} 判定",
        "項目", "値(中央値)", "単位", "予算"
    );
    for item in items {
        let (budget, verdict) = match (item.budget, item.within_budget()) {
            (Some(b), Some(ok)) => (
                match b.limit {
                    Limit::AtMost(v) => format!("<= {v}"),
                    Limit::AtLeast(v) => format!(">= {v}"),
                },
                if ok { "OK" } else { "超過" },
            ),
            _ => ("-".into(), "-"),
        };
        println!(
            "{:<28} {:>12.3} {:<6} {:<16} {}",
            item.name, item.value, item.unit, budget, verdict
        );
    }
}

fn item_json(item: &Item) -> Value {
    let budget = item.budget.map(|b| {
        let (kind, value) = match b.limit {
            Limit::AtMost(v) => ("at_most", v),
            Limit::AtLeast(v) => ("at_least", v),
        };
        json!({ "kind": kind, "value": value, "row": b.row })
    });
    json!({
        "name": item.name,
        "unit": item.unit,
        "better": if item.higher_is_better() { "higher" } else { "lower" },
        "value": item.value,
        "samples": item.samples,
        "budget": budget,
        "ok": item.within_budget(),
    })
}

/// 結果の JSON と `metrics.jsonl` を書き、JSON のパスを返す。
fn write_results(
    args: &PerfArgs,
    items: &[Item],
    env: &Environment,
    snapshot: &MetricsSnapshot,
) -> Result<PathBuf> {
    std::fs::create_dir_all(&args.out_dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let adapter = env.adapter.clone();
    let scene = scene_label(args);
    let metrics: Value = serde_json::from_str(&snapshot.to_jsonl_line())?;
    let doc = json!({
        "scene": scene,
        "timestamp": stamp,
        "repeat": args.repeat,
        "adapter": adapter,
        "items": items.iter().map(item_json).collect::<Vec<_>>(),
        "metrics": metrics,
    });
    let file = if args.baseline {
        format!("{scene}.json")
    } else {
        format!("{scene}-{stamp}.json")
    };
    let path = args.out_dir.join(file);
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
    // ベースラインは git に入れる確定した数だけを置く場所なので、追記する履歴は作らない。
    if !args.baseline {
        append_line(
            &args.out_dir.join("metrics.jsonl"),
            &snapshot.to_jsonl_line(),
        )?;
    }
    Ok(path)
}

/// 結果の `scene` と、ベースラインのファイル名の元。`export` を 4K で測ったときは別の組として扱う
/// （同じ項目の名前で、1080p の結果と混ざらないように）。
fn scene_label(args: &PerfArgs) -> String {
    let name = args.scene.name();
    if args.scene == Scene::Export && args.size.0 >= UHD_WIDTH {
        format!("{name}-4k")
    } else {
        name
    }
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 既定のプロジェクトはビルドした場所の絶対パスでない() {
        assert!(default_project(Scene::Export).is_relative());
    }

    #[test]
    fn 中央値() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn 予算の向きで合否が決まる() {
        let budget = |limit| Some(Budget { limit, row: "x" });
        let at_most = Item::from_samples("a", "ms", vec![10.0], budget(Limit::AtMost(20.0)));
        let at_least = Item::from_samples("b", "fps", vec![10.0], budget(Limit::AtLeast(20.0)));
        let none = Item::from_samples("c", "ms", vec![10.0], None);
        assert_eq!(at_most.within_budget(), Some(true));
        assert_eq!(at_least.within_budget(), Some(false));
        assert_eq!(none.within_budget(), None);
    }
}
