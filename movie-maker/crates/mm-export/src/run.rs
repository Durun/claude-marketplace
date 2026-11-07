//! レンダーターゲット 1 つの書き出し。出力先とコーデックを整え、描いて送り、検証して置き換え、納品物を書く。

use std::path::{Path, PathBuf};
use std::time::Instant;

use mm_audio_api::{NormalizeReport, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{AudioStreamInfo, EncodeReport, EncodeSettings};
use mm_doc::{Project, RenderTarget};
use mm_runtime::Engine;
use mm_types::{MmError, RationalTime, RenderTargetId, Result, Size, TimeRange};

use crate::control::{partial_path, PartialFile};
use crate::passes::{frames_in, render_video_and_audio, PassPosition, Plan};
use crate::{
    deliver, ensure_parent, pick_target, sequence, verify_output, Expected, ExportOptions,
    ExportOutcome, ExportSummary,
};

/// MP4 にしか書けないコーデックの名前の接頭辞（x264、NVENC の H.264 と HEVC）。
const MP4_CODEC_PREFIXES: [&str; 3] = ["h264", "libx264", "hevc"];
/// MP4 として開ける拡張子。
const MP4_EXTENSIONS: [&str; 2] = ["mp4", "m4v"];
/// MOV にしか書けないコーデックの名前の接頭辞（ProRes と DNxHR）。
const MOV_CODEC_PREFIXES: [&str; 2] = ["prores", "dnx"];

/// 書き出しが終わったときのエンコーダーの報告と、出力が満たすべきこと。
struct Finished {
    report: EncodeReport,
    expected: Expected,
    prepared: deliver::Prepared,
    normalization: Option<NormalizeReport>,
    implementation: Option<String>,
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
    let (project, output_path) = prepare_run(project, &target, options)?;
    let project = apply_burn_in(engine, project, &target, options)?;
    let partial = PartialFile::new(partial_path(&output_path));
    let started = Instant::now();
    let Some(done) = export_target(engine, &project, &target, options)? else {
        return Ok(ExportOutcome::Cancelled);
    };
    // 検証は一時ファイルのまま行う。失敗したら一時ファイルだけが消え、前の正常な出力は置き換わらない。
    let verification = options
        .verify
        .then(|| verify_output(&partial_path(&output_path), &done.expected))
        .transpose()?;
    partial.commit(&output_path)?;
    let mut delivery = deliver::write_sidecars(&done.prepared, &output_path)?;
    delivery.normalization = done.normalization;
    let elapsed = started.elapsed();
    let fps = done.report.frames as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    mm_trace::gauge!("export.fps", fps);
    mm_trace::counter!("export.frames", done.report.frames);
    Ok(ExportOutcome::Completed(ExportSummary {
        target,
        output_path,
        frames: done.report.frames,
        audio_frames: done.report.audio_frames,
        bytes_written: done.report.bytes_written,
        elapsed,
        fps,
        verification,
        delivery: Box::new(delivery),
        encoder: done.implementation,
    }))
}

/// 書き出す形のプロジェクトのデータ（sequence の差し替え、一時ファイルの出力先、コーデックの上書き）と、
/// 最後に置き換える出力先。コーデックと拡張子が合わなければ、何も書かずに拒否する。
fn prepare_run(
    project: &Project,
    target: &RenderTargetId,
    options: &ExportOptions,
) -> Result<(Project, PathBuf)> {
    let mut project = project.clone();
    if let Some(sequence) = &options.sequence {
        project = sequence::retarget_to_sequence(project, target, sequence)?.0;
    }
    let t = target_mut(&mut project, target)?;
    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(&t.output_path));
    if let Some(codec) = &options.codec {
        t.codec = codec.clone();
    }
    check_container(&t.codec, &output_path)?;
    ensure_parent(&output_path)?;
    t.output_path = partial_path(&output_path).to_string_lossy().into_owned();
    Ok((project, output_path))
}

/// 字幕を焼き込む設定なら、字幕のレイヤーを足したプロジェクトのデータにする。文字起こしが渡されていなければ拒否する
/// （黙って字幕なしにしない）。
fn apply_burn_in(
    engine: &Engine,
    project: Project,
    target: &RenderTargetId,
    options: &ExportOptions,
) -> Result<Project> {
    let t = project.render_target(target)?;
    if !t.delivery.burn_in_captions {
        return Ok(project);
    }
    let words = options.transcript.as_deref().ok_or_else(|| {
        MmError::InvalidArgument(
            "レンダーターゲットが字幕を焼き込む設定だが、文字起こしが渡されていない".into(),
        )
    })?;
    crate::burn_in_captions(
        &project,
        &t.composition,
        words,
        engine.services().text_shaper.as_ref(),
    )
}

fn target_mut<'a>(
    project: &'a mut Project,
    target: &RenderTargetId,
) -> Result<&'a mut RenderTarget> {
    project
        .render_targets
        .iter_mut()
        .find(|t| t.id == *target)
        .ok_or_else(|| MmError::NotFound(format!("render target {target}")))
}

/// コーデックが書けるコンテナと出力先の拡張子が合わなければ、書き出す前に拒否する
/// （拡張子と中身の形式が食い違うと、プレーヤーや納品先が開けない）。
pub(crate) fn check_container(codec: &str, output: &Path) -> Result<()> {
    let has_ext = |exts: &[&str]| {
        output
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
    };
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| codec.starts_with(p));
    let wanted = if starts(&MOV_CODEC_PREFIXES) && !has_ext(&["mov"]) {
        ".mov"
    } else if starts(&MP4_CODEC_PREFIXES) && !has_ext(&MP4_EXTENSIONS) {
        ".mp4 か .m4v"
    } else {
        return Ok(());
    };
    Err(MmError::InvalidArgument(format!(
        "コーデック {codec} の出力先の拡張子は {wanted} にする（{}）",
        output.display()
    )))
}

/// 区間を描き、バッチでエンコーダーに送り、音を付けて閉じる。取り消されたら `None`。
fn export_target(
    engine: &mut Engine,
    project: &Project,
    target: &RenderTargetId,
    options: &ExportOptions,
) -> Result<Option<Finished>> {
    let target = project.render_target(target)?;
    let comp = project.composition(&target.composition)?;
    let range = resolve_range(comp.range()?, options.range)?;
    let settings = encode_settings(target);
    let prepared = deliver::prepare(project, target, comp, range, options.transcript.as_deref())?;
    let mut encoder = open_encoder(engine, target, &prepared)?;
    encoder.begin(&settings)?;
    let plan = Plan {
        target: &target.id,
        composition: &target.composition,
        range,
        control: &options.control,
        parallel: options.parallel,
        parallel_min_frames: options.parallel_min_frames,
        pass: PassPosition { index: 1, of: 1 },
    };
    let audio_range = audio_range_of(range, &settings, frames_in(range, &settings))?;
    let rendered = render_video_and_audio(
        engine,
        project,
        &plan,
        &settings,
        audio_range,
        encoder.as_mut(),
    )?;
    let Some((frames, mut audio)) = rendered else {
        return Ok(None);
    };
    let normalization = deliver::normalize(engine, target, &mut audio)?;
    encoder.push_audio(&audio)?;
    let implementation = encoder.implementation();
    let report = encoder.finish()?;
    let expected = Expected {
        frames: frames as u64,
        size: settings.size,
        fps: settings.fps.to_f64(),
        audio_seconds: Some(audio_range.duration()?.to_f64()),
    };
    Ok(Some(Finished {
        report,
        expected,
        prepared,
        normalization,
        implementation,
    }))
}

/// レンダーターゲットのコーデックのエンコーダーを作り、字幕とチャプターのトラックを渡す。
fn open_encoder(
    engine: &Engine,
    target: &RenderTarget,
    prepared: &deliver::Prepared,
) -> Result<Box<dyn mm_decode_api::Encoder>> {
    let encoders = engine.encoders().clone();
    if !encoders.supports(&target.codec) {
        return Err(MmError::Unsupported(format!(
            "コーデック {} は {} が扱えない",
            target.codec,
            encoders.id()
        )));
    }
    let mut encoder = encoders.create()?;
    if !prepared.tracks.is_empty() {
        encoder.set_extras(prepared.tracks.clone())?;
    }
    Ok(encoder)
}

fn encode_settings(target: &RenderTarget) -> EncodeSettings {
    EncodeSettings {
        size: Size::new(target.width, target.height),
        fps: target.fps,
        codec: target.codec.clone(),
        output_path: PathBuf::from(&target.output_path),
        audio: Some(AudioStreamInfo {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
        }),
    }
}

/// 音の区間。29.97 fps の 1 秒のように映像の長さが半端なフレームで切れるとき、音も同じ長さで切って揃える。
fn audio_range_of(range: TimeRange, settings: &EncodeSettings, frames: usize) -> Result<TimeRange> {
    let first = range.frame_indices(settings.fps).start;
    TimeRange::new(
        RationalTime::from_frames(first, settings.fps)?,
        RationalTime::from_frames(first + frames as i64, settings.fps)?,
    )
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
