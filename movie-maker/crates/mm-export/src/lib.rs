//! 書き出し。レンダーターゲットを選び、出力先を整え、区間を描いてエンコーダーに押し、速さを測って返す。
//!
//! インターフェースの実装を選ぶのは mm-runtime だけという契約のため、このクレートは実装を知らず、`Engine` から
//! レンダラーとエンコーダーを取り出して使う（[`Engine::renderer`]、[`Engine::encoders`]）。
//! 段ごとの時間はスパン `export.render`（1 フレームの描画とリードバック）と `export.encode`（1 バッチの押し込み）に
//! 出す。
//!
//! - レンダーターゲットごと（[`export_targets`]）、範囲の指定（[`ExportOptions::range`]）、
//!   sequence のまま（[`ExportOptions::sequence`]）の書き出し。
//! - 出力は `<名前>.partial.<拡張子>` に書き、完成してから置き換える。失敗と取り消しでは書きかけを消す。
//! - 進み具合と取り消しは [`ExportControl`]。結果の検証は [`verify`]（ffprobe）。
//! - 1 プロセス・1 区間で、並行の描画（区間の分割と mux）とラウドネスの正規化は段階 4。

mod control;
mod sequence;
pub mod verify;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mm_audio_api::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{AudioStreamInfo, EncodeReport, EncodeSettings, Encoder, Nv12Frame};
use mm_doc::{Project, SequenceId};
use mm_node_api::Quality;
use mm_render_api::RangeRequest;
use mm_runtime::Engine;
use mm_types::{MmError, RationalTime, RenderTargetId, Result, RgbaImage, Size, TimeRange};

pub use control::{ExportControl, ExportProgress, ProgressFn};
pub use verify::{verify_output, Expected, Verification};

use control::{partial_path, PartialFile};

/// エンコーダーに一度に押すフレームの数。インターフェースの呼び出しを毎フレームにしない。
pub const EXPORT_BATCH_FRAMES: usize = 8;
/// 取り消しでエンコーダーの押し込みを止めたときのエラーの文言。呼び出しの内側だけで使い、外へは出さない。
const CANCELLED_MARK: &str = "書き出しの取り消し";

/// 書き出しの指定。
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// プロジェクトのデータのレンダーターゲットの `output_path` を上書きする。
    pub output: Option<PathBuf>,
    /// 書き出すレンダーターゲット。`None` ならプロジェクトのデータの最初の 1 つ。
    pub target: Option<RenderTargetId>,
    /// 書き出す区間。`None` ならコンポジション（`sequence` があればその全長）の全部。コンポジションの外は拒否する。
    pub range: Option<TimeRange>,
    /// レンダーターゲットのコンポジションの代わりに、この sequence を全長で書き出す。
    pub sequence: Option<SequenceId>,
    /// 書き出したあと ffprobe で出力を検証する。モックのエンコーダー（JSON を書く）では使えない。
    pub verify: bool,
    /// 取り消しと進み具合。
    pub control: ExportControl,
}

/// 書き出した結果と速さ。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub target: RenderTargetId,
    pub output_path: PathBuf,
    pub frames: u64,
    pub audio_frames: u64,
    pub bytes_written: u64,
    pub elapsed: Duration,
    /// 全体の fps（フレーム数 / 経過時間）。
    pub fps: f64,
    /// `ExportOptions::verify` のときの、ffprobe が読んだ出力の中身。
    pub verification: Option<Verification>,
}

/// 1 つのレンダーターゲットの書き出しの終わり方。
#[derive(Debug, Clone, PartialEq)]
pub enum ExportOutcome {
    Completed(ExportSummary),
    /// 取り消した。書きかけのファイルは消してあり、前の出力はそのまま。
    Cancelled,
}

/// 複数のレンダーターゲットを書き出した結果。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportBatch {
    /// 完成したもの（指定の順）。
    pub done: Vec<ExportSummary>,
    /// 途中で取り消したか。取り消したら、残りのレンダーターゲットは書き出さない。
    pub cancelled: bool,
}

/// 書き出すレンダーターゲットを決める。指定が無ければ最初の 1 つ。
pub fn pick_target(project: &Project, wanted: Option<&RenderTargetId>) -> Result<RenderTargetId> {
    match wanted {
        Some(id) => project.render_target(id).map(|t| t.id.clone()),
        None => project
            .render_targets
            .first()
            .map(|t| t.id.clone())
            .ok_or_else(|| {
                MmError::NotFound("プロジェクトのデータにレンダーターゲットが無い".into())
            }),
    }
}

/// 出力先の親ディレクトリを作る。無ければエンコーダーが開けずにクラッシュするため先に整える。
fn ensure_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Ok(std::fs::create_dir_all(dir)?),
        _ => Ok(()),
    }
}

/// レンダーターゲットを 1 つ書き出す。取り消されたら `Err`（取り消しを区別したいときは [`export_run`]）。
/// ゲージ `export.fps` に全体の fps を出す。
pub fn export_project(
    engine: &mut Engine,
    project: &Project,
    options: &ExportOptions,
) -> Result<ExportSummary> {
    match export_run(engine, project, options)? {
        ExportOutcome::Completed(summary) => Ok(summary),
        ExportOutcome::Cancelled => Err(MmError::Internal("書き出しを取り消した".into())),
    }
}

/// `targets` を順に書き出す。`options.output` は 1 つのときだけ使える。
/// 同じ出力先を指すレンダーターゲットが 2 つあると互いに上書きするので、始める前に拒否する。
pub fn export_targets(
    engine: &mut Engine,
    project: &Project,
    targets: &[RenderTargetId],
    options: &ExportOptions,
) -> Result<ExportBatch> {
    if targets.len() > 1 && options.output.is_some() {
        return Err(MmError::InvalidArgument(
            "出力先の上書きは、レンダーターゲットが 1 つのときだけ使える".into(),
        ));
    }
    reject_shared_outputs(project, targets)?;
    let mut done = Vec::with_capacity(targets.len());
    for target in targets {
        let one = ExportOptions {
            target: Some(target.clone()),
            ..options.clone()
        };
        match export_run(engine, project, &one)? {
            ExportOutcome::Completed(summary) => done.push(summary),
            ExportOutcome::Cancelled => {
                return Ok(ExportBatch {
                    done,
                    cancelled: true,
                })
            }
        }
    }
    Ok(ExportBatch {
        done,
        cancelled: false,
    })
}

fn reject_shared_outputs(project: &Project, targets: &[RenderTargetId]) -> Result<()> {
    let mut seen: Vec<(&str, &RenderTargetId)> = Vec::new();
    for id in targets {
        let path = project.render_target(id)?.output_path.as_str();
        if let Some((_, other)) = seen.iter().find(|(p, _)| *p == path) {
            return Err(MmError::InvalidArgument(format!(
                "レンダーターゲット {other} と {id} が同じ出力先 {path} を指している"
            )));
        }
        seen.push((path, id));
    }
    Ok(())
}

/// レンダーターゲットを 1 つ書き出し、取り消しも結果として返す。
pub fn export_run(
    engine: &mut Engine,
    project: &Project,
    options: &ExportOptions,
) -> Result<ExportOutcome> {
    let span = tracing::info_span!("export.run");
    let _g = span.enter();
    let target = pick_target(project, options.target.as_ref())?;
    let mut project = project.clone();
    if let Some(sequence) = &options.sequence {
        project = sequence::retarget_to_sequence(project, &target, sequence)?.0;
    }
    let output_path = options.output.clone().unwrap_or_else(|| {
        PathBuf::from(
            &project
                .render_target(&target)
                .map_or("", |t| &t.output_path),
        )
    });
    ensure_parent(&output_path)?;
    let partial = PartialFile::new(partial_path(&output_path));
    set_output(&mut project, &target, &partial_path(&output_path))?;

    let started = Instant::now();
    let Some(done) = export_target(engine, &project, &target, options)? else {
        return Ok(ExportOutcome::Cancelled);
    };
    partial.commit(&output_path)?;
    let elapsed = started.elapsed();
    let fps = done.report.frames as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    mm_trace::gauge!("export.fps", fps);
    mm_trace::counter!("export.frames", done.report.frames);
    let verification = options
        .verify
        .then(|| verify_output(&output_path, &done.expected))
        .transpose()?;
    Ok(ExportOutcome::Completed(ExportSummary {
        target,
        output_path,
        frames: done.report.frames,
        audio_frames: done.report.audio_frames,
        bytes_written: done.report.bytes_written,
        elapsed,
        fps,
        verification,
    }))
}

fn set_output(project: &mut Project, target: &RenderTargetId, path: &Path) -> Result<()> {
    let t = project
        .render_targets
        .iter_mut()
        .find(|t| t.id == *target)
        .ok_or_else(|| MmError::NotFound(format!("render target {target}")))?;
    t.output_path = path.to_string_lossy().into_owned();
    Ok(())
}

/// 書き出しが終わったときのエンコーダーの報告と、出力が満たすべきこと。
struct Finished {
    report: EncodeReport,
    expected: Expected,
}

/// 区間を描き、バッチでエンコーダーに押し、音を付けて閉じる。取り消されたら `None`。
fn export_target(
    engine: &mut Engine,
    project: &Project,
    target: &RenderTargetId,
    options: &ExportOptions,
) -> Result<Option<Finished>> {
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
    let range = resolve_range(comp.range()?, options.range)?;
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
    let plan = Plan {
        target: &target.id,
        composition: &target.composition,
        range,
        control: &options.control,
    };
    let frames = match export_frames(engine, project, &plan, &settings, encoder.as_mut()) {
        Ok(frames) => frames,
        Err(_) if options.control.is_cancelled() => return Ok(None),
        Err(e) => return Err(e),
    };
    // 29.97 fps の 1 秒のように映像の長さが半端なフレームで切れるとき、音も同じ長さで切って揃える。
    let first = range.frame_indices(settings.fps).start;
    let audio_range = TimeRange::new(
        RationalTime::from_frames(first, target.fps)?,
        RationalTime::from_frames(first + frames as i64, target.fps)?,
    )?;
    if options.control.is_cancelled() {
        return Ok(None);
    }
    let audio = engine.render_audio(project, &target.composition, audio_range)?;
    encoder.push_audio(&audio)?;
    let report = encoder.finish()?;
    let expected = Expected {
        frames: frames as u64,
        size: settings.size,
        fps: target.fps.to_f64(),
        audio_seconds: Some(audio_range.duration()?.to_f64()),
    };
    Ok(Some(Finished { report, expected }))
}

/// 指定の区間をコンポジションの範囲に収める。範囲の外を含む指定は拒否する（黙って縮めない）。
fn resolve_range(whole: TimeRange, wanted: Option<TimeRange>) -> Result<TimeRange> {
    let Some(wanted) = wanted else {
        return Ok(whole);
    };
    if wanted.is_empty() || wanted.start < whole.start || wanted.end > whole.end {
        return Err(MmError::InvalidArgument(format!(
            "書き出す区間 [{}, {}) がコンポジションの範囲 [{}, {}) に収まらない",
            wanted.start, wanted.end, whole.start, whole.end
        )));
    }
    Ok(wanted)
}

/// 何をどの区間で描くか。
struct Plan<'a> {
    target: &'a RenderTargetId,
    composition: &'a mm_types::CompositionId,
    range: TimeRange,
    control: &'a ExportControl,
}

/// 区間を描き、レンダラーの `export_range`（`EXPORT_BATCH_FRAMES` ずつ、または GPU で YUV にした経路）で
/// エンコーダーに押す。レンダラーは区間を一息に描くので、1 フレームの描画の時間はフレームが出てくる間隔で測り
/// `export.render` に載せる。エンコーダーが失敗したらそこで描画を止め、その `Err` をそのまま返す。押したフレームの数を返す。
fn export_frames(
    engine: &mut Engine,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<usize> {
    let registry = engine.registry().clone();
    let indices = plan.range.frame_indices(settings.fps);
    let total = (indices.end - indices.start).max(0) as u64;
    let started = Instant::now();
    let mut last = Instant::now();
    let mut done = 0u64;
    let mut metered = Metered {
        inner: encoder,
        control: plan.control,
    };
    engine.renderer().export_range(
        &RangeRequest {
            project,
            composition: plan.composition,
            range: plan.range,
            fps: settings.fps,
            size: settings.size,
            quality: Quality::Full,
            nodes: &registry,
        },
        &mut metered,
        EXPORT_BATCH_FRAMES,
        &mut |_index| {
            mm_trace::record_span("export.render", last.elapsed());
            last = Instant::now();
            done += 1;
            plan.control.report(&ExportProgress {
                target: plan.target.clone(),
                frames_done: done,
                frames_total: total,
                elapsed: started.elapsed(),
            });
        },
    )
}

/// 押し込みの時間をスパン `export.encode` に載せ、取り消されていたら押さずに止めるエンコーダー。
struct Metered<'a> {
    inner: &'a mut dyn Encoder,
    control: &'a ExportControl,
}

impl Metered<'_> {
    /// 取り消されていれば、レンダラーに描画を止めさせる `Err`。
    fn check_cancel(&self) -> Result<()> {
        if self.control.is_cancelled() {
            return Err(MmError::Internal(CANCELLED_MARK.into()));
        }
        Ok(())
    }
}

impl Encoder for Metered<'_> {
    fn begin(&mut self, settings: &EncodeSettings) -> Result<()> {
        self.inner.begin(settings)
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        self.check_cancel()?;
        let span = tracing::info_span!("export.encode", frames = frames.len());
        let _g = span.enter();
        self.inner.push_frames(frames)
    }

    fn push_nv12_frames(&mut self, frames: &[Nv12Frame]) -> Result<()> {
        // 空の列は NV12 に対応するかの問い合わせなので、取り消しとは関係なく通す。
        if frames.is_empty() {
            return self.inner.push_nv12_frames(frames);
        }
        self.check_cancel()?;
        let span = tracing::info_span!("export.encode", frames = frames.len());
        let _g = span.enter();
        self.inner.push_nv12_frames(frames)
    }

    fn push_audio(&mut self, block: &mm_audio_api::AudioBlock) -> Result<()> {
        self.inner.push_audio(block)
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        self.inner.finish()
    }
}

#[cfg(test)]
mod tests;
