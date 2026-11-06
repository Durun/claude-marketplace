//! 書き出し。レンダーターゲットを選び、出力先を整え、区間を描いてエンコーダーに押し、速さを測って返す。
//!
//! インターフェースの実装を選ぶのは mm-runtime だけという契約のため、このクレートは実装を知らず、`Engine` から
//! レンダラーとエンコーダーを取り出して使う（[`Engine::renderer`]、[`Engine::encoders`]）。
//! 段ごとの時間はスパン `export.render`（1 フレームの描画とリードバック）と `export.encode`（1 バッチの押し込み）に
//! 出す。
//!
//! - レンダーターゲットごと（[`export_targets`]）、範囲の指定（[`ExportOptions::range`]）、
//!   sequence のまま（[`ExportOptions::sequence`]）の書き出し。
//! - 出力は `<名前>.partial.<拡張子>` に書き、完成してから置き換える。失敗と取り消しでは一時ファイルを消す。
//! - 進み具合と取り消しは [`ExportControl`]。結果の検証は [`verify`]（ffprobe）。
//! - 並行の描画（[`ExportOptions::parallel`]）。区間を短いチャンクに分けて複数のレンダラーで描き、フレームの順に
//!   エンコーダーへ押す。2 パスのコーデックは、同じ描画を 2 回通す。
//! - 納品物（`deliver`、`captions`、`chapters`）: 目標のラウドネスへの正規化（2 パス、トゥルーピークの上限つき）、
//!   字幕（書き出しの隣の SRT と WebVTT、または MP4 の字幕のトラック）、チャプター（MP4 と YouTube の説明欄の文字列）。

mod captions;
mod chapters;
mod control;
mod cue_text;
mod deliver;
mod ordered;
mod parallel;
mod sequence;
mod subtitle_format;
pub mod verify;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_audio_api::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{
    AudioStreamInfo, EncodeReport, EncodeSettings, Encoder, ExtraTracks, Nv12Frame,
};
use mm_doc::{Project, SequenceId};
use mm_node_api::Quality;
use mm_render_api::RangeRequest;
use mm_runtime::{Engine, RuntimeConfig};
use mm_types::{MmError, RationalTime, RenderTargetId, Result, RgbaImage, Size, TimeRange};

pub use captions::{sequence_cues, CaptionWord, TranscriptWords};
pub use chapters::{chapter_marks, youtube_description};
pub use control::{ExportControl, ExportProgress, ProgressFn};
pub use deliver::DeliveryReport;
pub use subtitle_format::{srt, vtt};
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
    /// プロジェクトのデータのレンダーターゲットの `codec` を上書きする（`h264_nvenc`、`prores_422hq` など）。
    pub codec: Option<String>,
    /// 並行に描くレンダラーの数。0 と 1 は並行にしない（`Engine` のレンダラー 1 つで描く）。
    /// 2 以上は、レンダラーを作れる設定で組んだ別の `Engine` をその数だけ作り、区間をチャンクに分けて描かせる。
    pub parallel: usize,
    /// 並行にする最短のフレーム数。`None` は `parallel` 本 x 30 フレーム以上のとき（それより短いと全ワーカーに
    /// チャンクが回らないので、1 つのレンダラーで描く）。
    pub parallel_min_frames: Option<u64>,
    /// 素材ごとの字幕にする単語（消した単語を除き、直した文字を反映したもの）。レンダーターゲットが字幕を出す設定のとき要る。
    pub transcript: Option<Arc<TranscriptWords>>,
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
    /// ラウドネスの正規化の結果と、書き出しの隣に置いた字幕・チャプターのファイル。
    pub delivery: Box<DeliveryReport>,
}

/// 1 つのレンダーターゲットの書き出しの終わり方。
#[derive(Debug, Clone, PartialEq)]
pub enum ExportOutcome {
    Completed(ExportSummary),
    /// 取り消した。一時ファイルは消してあり、前の出力はそのまま。
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

/// 出力先と一時ファイルのパスのどれかが他のレンダーターゲットと重なる組を拒否する
/// （A の一時ファイル `x.partial.mp4` が B の出力 `x.mp4` を指す場合も、互いに壊し合う）。
fn reject_shared_outputs(project: &Project, targets: &[RenderTargetId]) -> Result<()> {
    let mut seen: Vec<([PathBuf; 2], &RenderTargetId)> = Vec::new();
    for id in targets {
        let path = PathBuf::from(&project.render_target(id)?.output_path);
        let names = [partial_path(&path), path];
        if let Some((other_names, other)) = seen
            .iter()
            .find(|(others, _)| others.iter().any(|o| names.contains(o)))
        {
            let shared = names
                .iter()
                .find(|n| other_names.contains(n))
                .unwrap_or(&names[1]);
            return Err(MmError::InvalidArgument(format!(
                "レンダーターゲット {other} と {id} が同じ出力先 {} を指している",
                shared.display()
            )));
        }
        seen.push((names, id));
    }
    Ok(())
}

/// 取り消しで止めたことを表すエラーか。旗が立っていても、別の原因の失敗は取り消しにしない。
fn is_cancel_error(e: &MmError) -> bool {
    matches!(e, MmError::Internal(m) if m == CANCELLED_MARK)
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
    let codec = options.codec.as_deref().or_else(|| {
        project
            .render_target(&target)
            .ok()
            .map(|t| t.codec.as_str())
    });
    if let Some(codec) = codec {
        check_container(codec, &output_path)?;
    }
    ensure_parent(&output_path)?;
    let partial = PartialFile::new(partial_path(&output_path));
    set_output(&mut project, &target, &partial_path(&output_path))?;
    if let Some(codec) = &options.codec {
        set_codec(&mut project, &target, codec)?;
    }

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

/// ProRes と DNxHR は MOV に書くので、出力先の拡張子が `.mov` でなければ書き出す前に断る
/// （MP4 の拡張子の中身が MOV になって、プレーヤーや納品先が開けなくなるのを防ぐ）。
fn check_container(codec: &str, output: &Path) -> Result<()> {
    let needs_mov = codec.starts_with("prores") || codec.starts_with("dnx");
    let is_mov = output
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mov"));
    if needs_mov && !is_mov {
        return Err(MmError::InvalidArgument(format!(
            "コーデック {codec} は MOV に書く。出力先の拡張子を .mov にする（{}）",
            output.display()
        )));
    }
    Ok(())
}

fn set_codec(project: &mut Project, target: &RenderTargetId, codec: &str) -> Result<()> {
    let t = project
        .render_targets
        .iter_mut()
        .find(|t| t.id == *target)
        .ok_or_else(|| MmError::NotFound(format!("render target {target}")))?;
    t.codec = codec.to_owned();
    Ok(())
}

/// 書き出しが終わったときのエンコーダーの報告と、出力が満たすべきこと。
struct Finished {
    report: EncodeReport,
    expected: Expected,
    prepared: deliver::Prepared,
    normalization: Option<mm_audio_api::NormalizeReport>,
}

/// 映像をエンコーダーに押し、音を混ぜる。並行の描画なら音を映像と同時に混ぜる。取り消されたら `None`。
/// 押した映像のフレームの数と、混ぜた音を返す。
fn render_video_and_audio(
    engine: &mut Engine,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    audio_range: TimeRange,
    encoder: &mut dyn Encoder,
) -> Result<Option<(usize, mm_audio_api::AudioBlock)>> {
    let total = frames_in(plan.range, settings);
    let concurrent = parallel::worth_it(plan.parallel, total as u64, plan.parallel_min_frames);
    let composition = plan.composition;
    let (video, audio) = if concurrent {
        // 並行の描画は別のエンジンを使うので、呼び出し側のエンジンは映像と同時に音を混ぜられる。
        let config = engine.config().clone();
        std::thread::scope(|scope| {
            let audio = scope.spawn(|| engine.render_audio(project, composition, audio_range));
            let video = export_passes_with(
                &mut Drawer::Workers(&config),
                project,
                plan,
                settings,
                encoder,
            );
            let audio = audio
                .join()
                .unwrap_or_else(|_| Err(MmError::Internal("音を混ぜるスレッドが落ちた".into())));
            (video, audio)
        })
    } else {
        let video = export_passes_with(
            &mut Drawer::Engine(&mut *engine),
            project,
            plan,
            settings,
            encoder,
        );
        let audio = match &video {
            Ok(_) if !plan.control.is_cancelled() => {
                engine.render_audio(project, composition, audio_range)
            }
            _ => Err(MmError::Internal("音は混ぜない".into())),
        };
        (video, audio)
    };
    let frames = match video {
        Ok(frames) => frames,
        Err(e) if is_cancel_error(&e) && plan.control.is_cancelled() => return Ok(None),
        Err(e) => return Err(e),
    };
    if plan.control.is_cancelled() {
        return Ok(None);
    }
    if frames != total {
        return Err(MmError::Internal(format!(
            "描いたフレームの数 {frames} が区間の数 {total} と違う"
        )));
    }
    Ok(Some((frames, audio?)))
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
    let settings = encode_settings(target);
    let prepared = deliver::prepare(project, target, comp, range, options.transcript.as_deref())?;
    let mut encoder = encoders.create()?;
    if !prepared.tracks.is_empty() {
        encoder.set_extras(prepared.tracks.clone())?;
    }
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
    let Some((frames, audio)) = render_video_and_audio(
        engine,
        project,
        &plan,
        &settings,
        audio_range,
        encoder.as_mut(),
    )?
    else {
        return Ok(None);
    };
    let mut audio = audio;
    let normalization = deliver::normalize(engine, target, &mut audio)?;
    encoder.push_audio(&audio)?;
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
    }))
}

/// 区間のフレーム数。
fn frames_in(range: TimeRange, settings: &EncodeSettings) -> usize {
    let indices = range.frame_indices(settings.fps);
    (indices.end - indices.start).max(0) as usize
}

fn encode_settings(target: &mm_doc::RenderTarget) -> EncodeSettings {
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

/// 映像を通す何回目か（2 パスのエンコーダーは 2 回通す）。
#[derive(Clone, Copy)]
struct PassPosition {
    index: u32,
    of: u32,
}

/// 何をどの区間で描くか。
#[derive(Clone, Copy)]
struct Plan<'a> {
    target: &'a RenderTargetId,
    composition: &'a mm_types::CompositionId,
    range: TimeRange,
    control: &'a ExportControl,
    parallel: usize,
    parallel_min_frames: Option<u64>,
    pass: PassPosition,
}

/// エンコーダーが求める回数（2 パスは 2 回）、区間を描いて押す。最後の 1 回に押したフレームの数を返す。
#[cfg(test)]
fn export_passes(
    engine: &mut Engine,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<usize> {
    export_passes_with(
        &mut Drawer::Engine(engine),
        project,
        plan,
        settings,
        encoder,
    )
}

/// 映像を描く手段。並行の描画は別のエンジンを組むので、設定だけでよい。
enum Drawer<'a> {
    Engine(&'a mut Engine),
    Workers(&'a RuntimeConfig),
}

/// `export_passes` の本体。
fn export_passes_with(
    drawer: &mut Drawer<'_>,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<usize> {
    let passes = encoder.passes().max(1);
    let mut frames = 0;
    for pass in 1..=passes {
        if passes > 1 {
            encoder.begin_pass(pass)?;
        }
        let pass_plan = Plan {
            pass: PassPosition {
                index: pass,
                of: passes,
            },
            ..*plan
        };
        frames = export_frames_with(drawer, project, &pass_plan, settings, encoder)?;
    }
    Ok(frames)
}

/// 区間を描き、レンダラーの `export_range`（`EXPORT_BATCH_FRAMES` ずつ、または GPU で YUV にした経路）で
/// エンコーダーに押す。レンダラーは区間を一息に描くので、1 フレームの描画の時間はフレームが出てくる間隔で測り
/// `export.render` に載せる。エンコーダーが失敗したらそこで描画を止め、その `Err` をそのまま返す。押したフレームの数を返す。
#[cfg(test)]
fn export_frames(
    engine: &mut Engine,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<usize> {
    export_frames_with(
        &mut Drawer::Engine(engine),
        project,
        plan,
        settings,
        encoder,
    )
}

/// `export_frames` の本体。
fn export_frames_with(
    drawer: &mut Drawer<'_>,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<usize> {
    let indices = plan.range.frame_indices(settings.fps);
    let frames_in_pass = (indices.end - indices.start).max(0) as u64;
    let started = Instant::now();
    let mut last = Instant::now();
    let mut done = u64::from(plan.pass.index - 1) * frames_in_pass;
    let total = frames_in_pass * u64::from(plan.pass.of);
    let mut metered = Metered {
        inner: encoder,
        control: plan.control,
    };
    let mut on_frame = |elapsed: Duration| {
        mm_trace::record_span("export.render", elapsed);
        done += 1;
        plan.control.report(&ExportProgress {
            target: plan.target.clone(),
            frames_done: done,
            frames_total: total,
            elapsed: started.elapsed(),
        });
    };
    let config = match drawer {
        Drawer::Workers(config) => Some(*config),
        Drawer::Engine(engine) => {
            parallel::worth_it(plan.parallel, frames_in_pass, plan.parallel_min_frames)
                .then(|| engine.config())
        }
    };
    if let Some(config) = config {
        let job = parallel::ParallelJob {
            project,
            composition: plan.composition,
            range: plan.range,
            settings,
            workers: plan.parallel,
            config: config.clone(),
            chunk_frames: None,
        };
        return parallel::export_parallel(&job, &mut metered, &mut |_, elapsed| on_frame(elapsed));
    }
    let Drawer::Engine(engine) = drawer else {
        return Err(MmError::Internal("並行の描画が要るのに設定が無い".into()));
    };
    let registry = engine.registry().clone();
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
            on_frame(last.elapsed());
            last = Instant::now();
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
    fn set_extras(&mut self, extras: ExtraTracks) -> Result<()> {
        self.inner.set_extras(extras)
    }

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

    fn passes(&self) -> u32 {
        self.inner.passes()
    }

    fn begin_pass(&mut self, pass: u32) -> Result<()> {
        self.inner.begin_pass(pass)
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
#[cfg(test)]
mod tests_parallel;

#[cfg(test)]
mod tests_deliver;
