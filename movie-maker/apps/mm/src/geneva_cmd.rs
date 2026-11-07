//! `mm geneva`: geneva のタイムライン（JSON。DTM_Vlog の `timeline.json`）を movie-maker のプロジェクトの JSON にする。
//!
//! 終わった回を movie-maker で書き出し直し、前の書き出しと比べる（`docs/design/11-testing.md` の「実データ」）ための取り込み。
//! 素材は動かさず、タイムラインの `assets.*.src` を `--assets`（省くとタイムラインのフォルダー）からの絶対パスで指す。
//! 変換できない要素は数えて、標準出力の JSON の `gaps` に出す。

mod media;
mod report;
mod sound;
mod syntax;
mod visual;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use mm_doc::{Asset, Composition, Project, RenderTarget};
use mm_types::{ColorSpace, Id, MmError, Rational, RationalTime, Result};
use serde_json::{json, Value};

use self::media::Media;
use self::report::{Gap, Report};
use self::visual::Ctx;

/// 出力の既定（geneva の既定と同じ）。
const DEFAULT_WIDTH: u32 = 1920;
const DEFAULT_HEIGHT: u32 = 1080;
const DEFAULT_FPS: u32 = 30;
/// 本編のコンポジションとレンダーターゲットの ID。
const MAIN_ID: &str = "main";
/// レンダーターゲットのコーデック。
const CODEC: &str = "h264";

/// `mm geneva` の引数。
#[derive(clap::Args)]
pub struct GenevaArgs {
    /// geneva のタイムラインの JSON。
    pub timeline: PathBuf,
    /// 書き出すプロジェクトの JSON。
    #[arg(short, long)]
    pub output: PathBuf,
    /// 素材の相対パスの基準。省くとタイムラインのフォルダー。
    #[arg(long)]
    pub assets: Option<PathBuf>,
    /// レンダーターゲットの出力先。省くとプロジェクトの JSON の隣の `render.mp4`。
    #[arg(long)]
    pub render_output: Option<PathBuf>,
}

/// 変換して JSON を書き、要約（素材、レイヤー、入れ子の数、長さ、変換できない要素）を標準出力に出す。
pub fn run(args: &GenevaArgs) -> Result<()> {
    let text = std::fs::read_to_string(&args.timeline)
        .map_err(|e| MmError::NotFound(format!("{}: {e}", args.timeline.display())))?;
    let timeline: Value = serde_json::from_str(&text)
        .map_err(|e| MmError::InvalidArgument(format!("タイムラインの JSON: {e}")))?;
    let base = args
        .assets
        .clone()
        .or_else(|| args.timeline.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let render_output = args
        .render_output
        .clone()
        .unwrap_or_else(|| args.output.with_file_name("render.mp4"));
    let (project, report) = convert(&timeline, &absolute(&base)?, &absolute(&render_output)?)?;
    let body = serde_json::to_string(&project)
        .map_err(|e| MmError::Internal(format!("プロジェクトの JSON: {e}")))?;
    std::fs::write(&args.output, body)
        .map_err(|e| MmError::Internal(format!("{}: {e}", args.output.display())))?;
    println!("{}", summary(&project, &report));
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path)
        .map_err(|e| MmError::InvalidArgument(format!("{}: {e}", path.display())))
}

fn summary(project: &Project, report: &Report) -> Value {
    let main = project
        .compositions
        .iter()
        .find(|c| c.id.as_str() == MAIN_ID);
    json!({
        "assets": project.assets.len(),
        "layers": main.map_or(0, |c| c.layers.len()),
        "nested": project.compositions.len().saturating_sub(1),
        "duration": main.map_or(0.0, |c| c.duration.to_f64()),
        "gaps": report.to_json(),
    })
}

/// タイムラインをプロジェクトにする。素材は `base` からのパスで調べる。
pub fn convert(timeline: &Value, base: &Path, render_output: &Path) -> Result<(Project, Report)> {
    let out = timeline.get("output").cloned().unwrap_or(Value::Null);
    let width = dimension(&out, "width", DEFAULT_WIDTH);
    let height = dimension(&out, "height", DEFAULT_HEIGHT);
    let fps = output_fps(&out)?;
    let (assets, media) = probe_assets(timeline, base, fps)?;
    let mut ctx = Ctx {
        fps,
        frame: [f64::from(width), f64::from(height)],
        media: &media,
        report: Report::default(),
        nested: Vec::new(),
    };
    note_top_level(&mut ctx.report, timeline);
    let list = |k: &str| {
        timeline
            .get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut layers = sound::audio_layers(&mut ctx, &list("audio"))?;
    layers.extend(visual::visual_layers(&mut ctx, &list("layers"))?);
    let duration = layers
        .iter()
        .map(|l| l.range().map(|r| r.end))
        .try_fold(RationalTime::ZERO, |m, e| e.map(|e| m.max(e)))?;
    let mut project = Project::new();
    project.assets = assets;
    project.compositions = std::mem::take(&mut ctx.nested);
    project.compositions.push(Composition {
        id: Id::new(MAIN_ID),
        width,
        height,
        fps,
        duration,
        layers,
        markers: Vec::new(),
        mixer: None,
    });
    project
        .render_targets
        .push(render_target(&out, [width, height], fps, render_output));
    project.normalize();
    project.validate()?;
    Ok((project, ctx.report))
}

fn dimension(out: &Value, key: &str, default: u32) -> u32 {
    out.get(key)
        .and_then(Value::as_u64)
        .map_or(default, |n| n as u32)
}

fn output_fps(out: &Value) -> Result<Rational> {
    match out.get("fps") {
        None => Ok(Rational::integer(DEFAULT_FPS)),
        Some(Value::String(s)) => {
            let (n, d) = s.split_once('/').unwrap_or((s, "1"));
            let parse = |x: &str| {
                x.trim()
                    .parse::<u32>()
                    .map_err(|_| MmError::InvalidArgument(format!("fps: {s}")))
            };
            Rational::new(parse(n)?, parse(d)?)
        }
        Some(v) => {
            let t = syntax::decimal(&v.to_string())?;
            Rational::new(u32::try_from(t.num()).unwrap_or(DEFAULT_FPS), t.den())
        }
    }
}

fn note_top_level(report: &mut Report, timeline: &Value) {
    let present = |k: &str| match timeline.get(k) {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(_) => true,
        None => false,
    };
    for key in ["subtitles", "compositions", "outputs"] {
        if present(key) {
            report.add(Gap::TopLevel);
        }
    }
}

fn render_target(
    out: &Value,
    [width, height]: [u32; 2],
    fps: Rational,
    path: &Path,
) -> RenderTarget {
    let loudness = out.pointer("/audio/loudness");
    let mut target = RenderTarget {
        id: Id::new(MAIN_ID),
        composition: Id::new(MAIN_ID),
        width,
        height,
        fps,
        codec: CODEC.into(),
        output_path: path.to_string_lossy().into_owned(),
        loudness_lufs: loudness
            .and_then(|l| l.get("target_lufs"))
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        delivery: Default::default(),
    };
    target.delivery.true_peak_dbtp = loudness
        .and_then(|l| l.get("true_peak_dbtp"))
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    target
}

/// タイムラインの素材を全部調べ、プロジェクトの素材と、ID ごとの性質にする。
fn probe_assets(
    timeline: &Value,
    base: &Path,
    fps: Rational,
) -> Result<(Vec<Asset>, HashMap<String, Media>)> {
    let entries = timeline
        .get("assets")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut assets = Vec::with_capacity(entries.len());
    let mut media = HashMap::with_capacity(entries.len());
    for (id, entry) in &entries {
        let src = entry
            .get("src")
            .and_then(Value::as_str)
            .ok_or_else(|| MmError::InvalidArgument(format!("素材 {id} に src が無い")))?;
        let path = base.join(src);
        let m = media::probe(&path)?;
        assets.push(asset(id, &path, &m, fps)?);
        media.insert(id.clone(), m);
    }
    Ok((assets, media))
}

fn asset(id: &str, path: &Path, m: &Media, fps: Rational) -> Result<Asset> {
    let duration = if m.still {
        RationalTime::from_frames(1, fps)?
    } else {
        m.duration
    };
    Ok(Asset {
        id: Id::new(id),
        path: path.to_string_lossy().into_owned(),
        content_hash: content_stamp(path),
        color_space: if m.still {
            ColorSpace::srgb()
        } else {
            ColorSpace::bt709()
        },
        duration,
        fps: if m.still { Some(fps) } else { m.fps },
        timebase_offset: RationalTime::ZERO,
    })
}

/// 中身を読まずに作る識別の文字（大きさと更新の時刻）。素材を全部読むハッシュは数百 GB になるため。
fn content_stamp(path: &Path) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    format!("size{}-mtime{modified}", meta.len())
}
