//! `export` シナリオ。フィクスチャのプロジェクトを `--size` で書き出し、速さを測る。
//! `--codec` と `--parallel` で 1 つの組を、`--matrix` でコーデックと並行の数の組を全部測る。

use mm_export::{export_project, ExportOptions};
use mm_runtime::{assemble, Engine, RuntimeConfig};
use mm_types::{Id, Rational, RationalTime, Result};

use crate::perf::{Budget, Environment, Item, Limit, PerfArgs};
use crate::project::load_project;

const EXPORT_ROW: &str = "書き出し（1080p60、NVENC、1 プロセス）";
const PARALLEL_ROW: &str = "書き出し（並行 4）";
/// 素材のレイヤーのノードの種類。
const FOOTAGE_KIND: &str = "footage";
const MS_PER_SECOND: f64 = 1000.0;
/// 並行の書き出しの予算（実時間の 5 倍、1080p60 の 300 fps）。
const PARALLEL_BUDGET_FPS: f64 = 300.0;
/// 1 プロセスの NVENC の予算（実時間の 2 倍、1080p60 の 120 fps）。
const SINGLE_BUDGET_FPS: f64 = 120.0;
/// `--matrix` の並行の数。
const MATRIX_PARALLEL: usize = 4;
/// `--matrix` のコーデック。libx264（CPU）、2 パス、NVENC の H.264 と HEVC、ProRes。
const MATRIX_CODECS: [&str; 5] = [
    "h264",
    "h264_2pass",
    "h264_nvenc",
    "hevc_nvenc",
    "prores_422hq",
];

/// 測る組 1 つ。`None` のコーデックはプロジェクトのレンダーターゲットのまま。
struct Variant<'a> {
    codec: Option<&'a str>,
    parallel: usize,
}

/// `export`。
pub fn export(args: &PerfArgs, env: &mut Environment) -> Result<Vec<Item>> {
    let mut project = load_project(&args.project)?;
    for comp in &mut project.compositions {
        (comp.width, comp.height) = args.size;
    }
    for target in &mut project.render_targets {
        (target.width, target.height) = args.size;
    }
    lengthen(&mut project, args.seconds)?;
    let mut engine = assemble(&RuntimeConfig::real())?;
    env.adapter = engine.gpu_adapter().map(|a| a.name.clone());
    let single = Variant {
        codec: args.codec.as_deref(),
        parallel: args.parallel,
    };
    let mut items = measure(args, &mut engine, &project, &single, "export", true)?;
    if args.matrix {
        for codec in MATRIX_CODECS {
            for parallel in [1, MATRIX_PARALLEL] {
                let variant = Variant {
                    codec: Some(codec),
                    parallel,
                };
                let prefix = format!("export.{codec}.p{parallel}");
                items.extend(measure(
                    args,
                    &mut engine,
                    &project,
                    &variant,
                    &prefix,
                    false,
                )?);
            }
        }
    }
    Ok(items)
}

/// コンポジションを `seconds` 秒にする。元が `seconds` 秒以上なら切り詰めるだけにする（カットごとに素材の時刻が違う
/// フィクスチャをそのまま測る）。短ければ素材のカットを並べて繰り返し、文字は全長に伸ばす。
/// 1 秒の素材では並行のチャンクが 1 つしかできず、立ち上がりの時間が数字を支配するため。
fn lengthen(project: &mut mm_doc::Project, seconds: f64) -> Result<()> {
    let whole = seconds.max(1.0).round() as i64;
    let Some(comp) = project.compositions.first_mut() else {
        return Ok(());
    };
    let covered = comp.duration >= RationalTime::seconds(whole);
    comp.duration = RationalTime::seconds(whole);
    if covered {
        return Ok(());
    }
    let layers = std::mem::take(&mut comp.layers);
    for layer in layers {
        if layer.source.kind == FOOTAGE_KIND {
            let cut = layer.out_point.checked_sub(layer.in_point)?;
            for i in 0..whole {
                let mut copy = layer.clone();
                copy.id = Id::new(format!("{}-{i}", layer.id));
                let offset = match i {
                    0 => RationalTime::ZERO,
                    n => cut.scale(Rational::integer(n as u32))?,
                };
                copy.start = layer.start.checked_add(offset)?;
                comp.layers.push(copy);
            }
        } else {
            let mut stretched = layer;
            stretched.out_point = stretched
                .in_point
                .checked_add(RationalTime::seconds(whole))?;
            comp.layers.push(stretched);
        }
    }
    Ok(())
}

/// 1 つの組を `args.repeat` 回書き出し、fps と経過時間の項目にする。`plain` は元の名前（予算つき）で出す。
fn measure(
    args: &PerfArgs,
    engine: &mut Engine,
    project: &mm_doc::Project,
    variant: &Variant<'_>,
    prefix: &str,
    plain: bool,
) -> Result<Vec<Item>> {
    let work = args.out_dir.join("export-work");
    let (mut fps, mut elapsed_ms) = (Vec::new(), Vec::new());
    for i in 0..args.repeat {
        // 書き出しは入れ物と合わない拡張子を断るので、ProRes と DNxHR は .mov、ほかは .mp4 にする。
        let mov = variant
            .codec
            .is_some_and(|c| c.starts_with("prores") || c.starts_with("dnx"));
        let output = work.join(format!("run{i}.{}", if mov { "mov" } else { "mp4" }));
        let summary = export_project(
            engine,
            project,
            &ExportOptions {
                output: Some(output.clone()),
                codec: variant.codec.map(str::to_owned),
                parallel: variant.parallel,
                ..ExportOptions::default()
            },
        )?;
        fps.push(summary.fps);
        elapsed_ms.push(summary.elapsed.as_secs_f64() * MS_PER_SECOND);
        std::fs::remove_file(&output)?;
    }
    let cold = fps.first().copied().unwrap_or(0.0);
    let budget = budget_of(args, variant, plain);
    let name = |suffix: &str| format!("{prefix}.{suffix}");
    Ok(vec![
        Item::from_samples(name("fps"), "fps", fps, budget),
        Item::from_samples(name("fps_cold"), "fps", vec![cold], None),
        Item::from_samples(name("elapsed_ms"), "ms", elapsed_ms, None),
    ])
}

/// 予算は 1080p の測定だけに付ける。元の名前の項目（従来の `export.fps`）と、NVENC の H.264 の組。
fn budget_of(args: &PerfArgs, variant: &Variant<'_>, plain: bool) -> Option<Budget> {
    let hd = args.size == (1920, 1080);
    if plain {
        return Some(Budget {
            limit: Limit::AtLeast(SINGLE_BUDGET_FPS),
            row: EXPORT_ROW,
        })
        .filter(|_| variant.parallel <= 1);
    }
    if !hd || variant.codec != Some("h264_nvenc") {
        return None;
    }
    match variant.parallel {
        1 => Some(Budget {
            limit: Limit::AtLeast(SINGLE_BUDGET_FPS),
            row: EXPORT_ROW,
        }),
        MATRIX_PARALLEL => Some(Budget {
            limit: Limit::AtLeast(PARALLEL_BUDGET_FPS),
            row: PARALLEL_ROW,
        }),
        _ => None,
    }
}
