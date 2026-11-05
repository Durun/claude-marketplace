//! 書き出し。レンダーターゲットを選び、出力先を整え、区間を描いてエンコーダーに押し、速さを測って返す。
//!
//! 口の実装を選ぶのは mm-runtime だけという約束のため、このクレートは実装を知らず、`Engine` から
//! 描画器とエンコーダーを取り出して使う（[`Engine::renderer`]、[`Engine::encoders`]）。
//! 段ごとの時間はスパン `export.render`（1 コマの描画と読み戻し）と `export.encode`（1 バッチの押し込み）に
//! 出す。段階 0 は 1 プロセス・1 区間で、並行の描画とラウドネスの正規化は無い。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mm_audio_api::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{AudioStreamInfo, EncodeReport, EncodeSettings, Encoder};
use mm_doc::Project;
use mm_node_api::Quality;
use mm_render_api::RangeRequest;
use mm_runtime::Engine;
use mm_types::{MmError, RenderTargetId, Result, RgbaImage, Size};

/// エンコーダーに一度に押すコマの数。口の呼び出しを毎コマにしない。
pub const EXPORT_BATCH_FRAMES: usize = 8;

/// 書き出しの指定。
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// 文書のレンダーターゲットの `output_path` を上書きする。
    pub output: Option<PathBuf>,
    /// 書き出すレンダーターゲット。`None` なら文書の最初の 1 つ。
    pub target: Option<RenderTargetId>,
}

/// 書き出した結果と速さ。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub output_path: PathBuf,
    pub frames: u64,
    pub audio_frames: u64,
    pub bytes_written: u64,
    pub elapsed: Duration,
    /// 全体の fps（コマ数 / 経過時間）。
    pub fps: f64,
}

/// 書き出すレンダーターゲットを決める。指定が無ければ最初の 1 つ。
pub fn pick_target(project: &Project, wanted: Option<&RenderTargetId>) -> Result<RenderTargetId> {
    match wanted {
        Some(id) => project.render_target(id).map(|t| t.id.clone()),
        None => project
            .render_targets
            .first()
            .map(|t| t.id.clone())
            .ok_or_else(|| MmError::NotFound("文書にレンダーターゲットが無い".into())),
    }
}

/// 出力先の親ディレクトリを作る。無ければエンコーダーが開けずに落ちるため先に整える。
fn ensure_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Ok(std::fs::create_dir_all(dir)?),
        _ => Ok(()),
    }
}

/// レンダーターゲットを書き出す。ゲージ `export.fps` に全体の fps を出す。
pub fn export_project(
    engine: &mut Engine,
    project: &Project,
    options: &ExportOptions,
) -> Result<ExportSummary> {
    let span = tracing::info_span!("export.run");
    let _g = span.enter();
    let target = pick_target(project, options.target.as_ref())?;
    let mut project = project.clone();
    if let Some(output) = &options.output {
        let t = project
            .render_targets
            .iter_mut()
            .find(|t| t.id == target)
            .ok_or_else(|| MmError::NotFound(format!("render target {target}")))?;
        t.output_path = output.to_string_lossy().into_owned();
    }
    let output_path = PathBuf::from(&project.render_target(&target)?.output_path);
    ensure_parent(&output_path)?;

    let started = Instant::now();
    let encode = export_target(engine, &project, &target)?;
    let elapsed = started.elapsed();
    let fps = encode.frames as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    mm_trace::gauge!("export.fps", fps);
    mm_trace::counter!("export.frames", encode.frames);
    Ok(ExportSummary {
        output_path,
        frames: encode.frames,
        audio_frames: encode.audio_frames,
        bytes_written: encode.bytes_written,
        elapsed,
        fps,
    })
}

/// 区間を描き、バッチでエンコーダーに押し、音を付けて閉じる。
fn export_target(
    engine: &mut Engine,
    project: &Project,
    target: &RenderTargetId,
) -> Result<EncodeReport> {
    let target = project.render_target(target)?;
    let comp = project.composition(&target.composition)?;
    let encoders = engine.encoders().clone();
    if !encoders.supports(&target.codec) {
        return Err(MmError::Unsupported(format!(
            "コーデック {} は {} が扱えない",
            target.codec,
            encoders.id()
        )));
    }
    let settings = EncodeSettings {
        size: Size::new(target.width, target.height),
        fps: target.fps,
        codec: target.codec.clone(),
        output_path: PathBuf::from(&target.output_path),
        audio: Some(AudioStreamInfo {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
        }),
    };
    let mut encoder = encoders.create()?;
    encoder.begin(&settings)?;
    let range = comp.range();
    export_frames(
        engine,
        project,
        &target.composition,
        &settings,
        encoder.as_mut(),
    )?;
    let audio = engine.render_audio(project, &target.composition, range)?;
    encoder.push_audio(&audio)?;
    encoder.finish()
}

/// 区間を描き、`EXPORT_BATCH_FRAMES` ずつエンコーダーに押す。
/// 描画器は区間を一息に描くので、1 コマの描画の時間は sink が呼ばれる間隔で測り `export.render` に載せる。
fn export_frames(
    engine: &mut Engine,
    project: &Project,
    composition: &mm_types::CompositionId,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<()> {
    let registry = engine.registry().clone();
    let range = project.composition(composition)?.range();
    let mut batch: Vec<RgbaImage> = Vec::with_capacity(EXPORT_BATCH_FRAMES);
    let mut push_error: Option<MmError> = None;
    let mut last = Instant::now();
    engine.renderer().render_range(
        &RangeRequest {
            project,
            composition,
            range,
            fps: settings.fps,
            size: settings.size,
            quality: Quality::Full,
            nodes: &registry,
        },
        &mut |_index, frame, gpu| {
            batch.push((*gpu.download(&frame)?).clone());
            mm_trace::record_span("export.render", last.elapsed());
            if batch.len() == EXPORT_BATCH_FRAMES {
                let span = tracing::info_span!("export.encode", frames = batch.len());
                let _g = span.enter();
                if let Err(e) = encoder.push_frames(&batch) {
                    push_error = Some(e);
                }
                batch.clear();
            }
            last = Instant::now();
            Ok(())
        },
    )?;
    if let Some(e) = push_error {
        return Err(e);
    }
    if !batch.is_empty() {
        let span = tracing::info_span!("export.encode", frames = batch.len());
        let _g = span.enter();
        encoder.push_frames(&batch)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_runtime::{assemble, RuntimeConfig};
    use mm_testkit::decode::SampleMedia;
    use mm_testkit::fixtures::{vertical_project, VERTICAL_DURATION_SECONDS};
    use mm_trace::MetricsLayer;
    use tracing_subscriber::prelude::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mm_export_{name}_{}", std::process::id()))
    }

    #[test]
    fn 出力先を上書きして書き出し段ごとのスパンが出る() {
        let media = SampleMedia::default();
        let (project, _, _) = vertical_project(&media, &temp("unused.json"));
        let out = temp("dir").join("nested").join("out.json");
        let (layer, metrics) = MetricsLayer::new();
        let subscriber = tracing_subscriber::registry().with(layer);
        let summary = tracing::subscriber::with_default(subscriber, || {
            let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
            export_project(
                &mut engine,
                &project,
                &ExportOptions {
                    output: Some(out.clone()),
                    target: None,
                },
            )
            .unwrap()
        });
        let frames = VERTICAL_DURATION_SECONDS as u64 * media.fps.num() as u64;
        assert_eq!(summary.output_path, out);
        assert_eq!(summary.frames, frames);
        assert_eq!(
            summary.audio_frames,
            VERTICAL_DURATION_SECONDS as u64 * DEFAULT_SAMPLE_RATE as u64
        );
        assert!(out.exists(), "親ディレクトリごと作られる");
        let written: EncodeReport = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(written.frames, frames, "エンコーダーの報告が出力と合う");

        let snap = metrics.snapshot();
        assert_eq!(snap.spans["export.run"].count, 1);
        assert_eq!(snap.spans["export.render"].count, frames);
        assert_eq!(
            snap.spans["export.encode"].count,
            frames.div_ceil(EXPORT_BATCH_FRAMES as u64)
        );
        assert_eq!(snap.counters["export.frames"], frames as f64);
        let _ = std::fs::remove_dir_all(temp("dir"));
    }

    #[test]
    fn 無いレンダーターゲットは断る() {
        let project = Project::new();
        assert!(pick_target(&project, None).is_err());
    }
}
