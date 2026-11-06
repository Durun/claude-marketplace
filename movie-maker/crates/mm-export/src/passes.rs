//! 映像を描いてエンコーダーに送る工程。2 パスのコーデックは同じ描画を 2 回通し、並行の描画は別のエンジンで描く。
//! 音は映像と同じ取り消しで止まるよう、短い区間ずつ混ぜてつなぐ。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mm_audio_api::AudioBlock;
use mm_decode_api::{EncodeReport, EncodeSettings, Encoder, ExtraTracks, Nv12Frame};
use mm_doc::Project;
use mm_node_api::Quality;
use mm_render_api::RangeRequest;
use mm_runtime::{Engine, RuntimeConfig};
use mm_types::{
    CompositionId, MmError, RationalTime, RenderTargetId, Result, RgbaImage, TimeRange,
};

use crate::{
    is_cancel_error, parallel, ExportControl, ExportProgress, CANCELLED_MARK, EXPORT_BATCH_FRAMES,
};

/// 音を一度に混ぜる長さ（秒）。取り消しを確かめる間隔で、ミキサーは続きの区間で状態を持ち越すのでつなぎ目は出ない。
const AUDIO_CHUNK: RationalTime = RationalTime::seconds(10);
/// NVENC のコーデックの名前の末尾（`h264_nvenc`、`hevc_nvenc`）。
const NVENC_SUFFIX: &str = "_nvenc";

/// 映像を通す何回目か（2 パスのエンコーダーは 2 回通す）。
#[derive(Clone, Copy)]
pub(crate) struct PassPosition {
    pub index: u32,
    pub of: u32,
}

/// 何をどの区間で描くか。
#[derive(Clone, Copy)]
pub(crate) struct Plan<'a> {
    pub target: &'a RenderTargetId,
    pub composition: &'a CompositionId,
    pub range: TimeRange,
    pub control: &'a ExportControl,
    pub parallel: usize,
    pub parallel_min_frames: Option<u64>,
    pub pass: PassPosition,
}

impl Plan<'_> {
    /// 区間を並行に描くか。判定はここだけで行い、描く手段（[`Drawer`]）を選ぶ。
    /// NVENC は 1 つのセッションが律速で、並行にすると描画のワーカーと取り合って遅くなるので並行にしない。
    pub(crate) fn concurrent(&self, settings: &EncodeSettings) -> bool {
        let frames = frames_in(self.range, settings) as u64;
        let nvenc = settings.codec.ends_with(NVENC_SUFFIX);
        if nvenc && self.parallel > 1 {
            tracing::info!(codec = %settings.codec, "NVENC は並行にしない");
        }
        !nvenc && parallel::worth_it(self.parallel, frames, self.parallel_min_frames)
    }
}

/// 区間のフレーム数。
pub(crate) fn frames_in(range: TimeRange, settings: &EncodeSettings) -> usize {
    let indices = range.frame_indices(settings.fps);
    (indices.end - indices.start).max(0) as usize
}

/// 映像を描く手段。並行の描画は別のエンジンを組むので、設定だけでよい。
pub(crate) enum Drawer<'a> {
    Engine(&'a mut Engine),
    Workers(&'a RuntimeConfig),
}

/// 映像をエンコーダーに送り、音を混ぜる。並行の描画なら音を映像と同時に混ぜる。取り消されたら `None`。
/// 送った映像のフレームの数と、混ぜた音を返す。
pub(crate) fn render_video_and_audio(
    engine: &mut Engine,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    audio_range: TimeRange,
    encoder: &mut dyn Encoder,
) -> Result<Option<(usize, AudioBlock)>> {
    let (video, audio) = if plan.concurrent(settings) {
        // 並行の描画は別のエンジンを使うので、呼び出し側のエンジンは映像と同時に音を混ぜられる。
        let config = engine.config().clone();
        let video_failed = AtomicBool::new(false);
        let stop = || plan.control.is_cancelled() || video_failed.load(Ordering::Relaxed);
        std::thread::scope(|scope| {
            let audio = scope.spawn(|| {
                mix_audio(
                    engine,
                    project,
                    plan.composition,
                    audio_range,
                    AUDIO_CHUNK,
                    &stop,
                )
            });
            let video = export_passes_with(
                &mut Drawer::Workers(&config),
                project,
                plan,
                settings,
                encoder,
            );
            // 映像が止まったら、音を混ぜ終えるのを待たずに抜けられるよう音も止める
            video_failed.store(video.is_err(), Ordering::Relaxed);
            let audio = audio.join().unwrap_or_else(|_| {
                Err(MmError::Internal("音を混ぜるスレッドがパニックした".into()))
            });
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
            Ok(_) => mix_audio(
                engine,
                project,
                plan.composition,
                audio_range,
                AUDIO_CHUNK,
                &|| plan.control.is_cancelled(),
            ),
            Err(_) => Err(MmError::Internal("音は混ぜない".into())),
        };
        (video, audio)
    };
    finish_video_and_audio(plan, frames_in(plan.range, settings), video, audio)
}

/// 映像と音の結果をまとめる。取り消しは `None`、映像の失敗を音の失敗より先に返す。
fn finish_video_and_audio(
    plan: &Plan<'_>,
    total: usize,
    video: Result<usize>,
    audio: Result<Option<AudioBlock>>,
) -> Result<Option<(usize, AudioBlock)>> {
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
    Ok(audio?.map(|audio| (frames, audio)))
}

/// 区間の音を `step` ずつ混ぜてつなぐ。区間の間で `stop` を確かめ、立っていたら `None`。
/// 区間はつながっているので、レンダラーはミキサーの状態を持ち越して 1 回で混ぜたのと同じ音を返す。
pub(crate) fn mix_audio(
    engine: &mut Engine,
    project: &Project,
    composition: &CompositionId,
    range: TimeRange,
    step: RationalTime,
    stop: &dyn Fn() -> bool,
) -> Result<Option<AudioBlock>> {
    let mut mixed: Option<AudioBlock> = None;
    let mut start = range.start;
    while start < range.end {
        if stop() {
            return Ok(None);
        }
        let end = start.checked_add(step)?.min(range.end);
        let block = engine.render_audio(project, composition, TimeRange::new(start, end)?)?;
        match &mut mixed {
            Some(all) => all.samples.extend_from_slice(&block.samples),
            None => mixed = Some(block),
        }
        start = end;
    }
    match mixed {
        Some(all) => Ok(Some(all)),
        // 長さ 0 の区間でも、チャンネル数とサンプルレートを持った空の音を返す
        None => engine.render_audio(project, composition, range).map(Some),
    }
}

/// エンコーダーが求める回数（2 パスは 2 回）、区間を描いて送る。最後の 1 回に送ったフレームの数を返す。
#[cfg(test)]
pub(crate) fn export_passes(
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
/// エンコーダーに送る。送ったフレームの数を返す。
#[cfg(test)]
pub(crate) fn export_frames(
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

/// `export_frames` の本体。レンダラーは区間を一息に描くので、1 フレームの描画の時間はフレームが出てくる間隔で測り
/// `export.render` に載せる。エンコーダーが失敗したらそこで描画を止め、その `Err` をそのまま返す。
fn export_frames_with(
    drawer: &mut Drawer<'_>,
    project: &Project,
    plan: &Plan<'_>,
    settings: &EncodeSettings,
    encoder: &mut dyn Encoder,
) -> Result<usize> {
    let mut progress = Progress::new(plan, frames_in(plan.range, settings) as u64);
    let mut metered = Metered {
        inner: encoder,
        control: plan.control,
    };
    let engine = match drawer {
        Drawer::Workers(config) => {
            let job = parallel::ParallelJob {
                project,
                composition: plan.composition,
                range: plan.range,
                settings,
                workers: plan.parallel,
                config: (*config).clone(),
                chunk_frames: None,
            };
            return parallel::export_parallel(&job, &mut metered, &mut |_, elapsed| {
                progress.frame(elapsed)
            });
        }
        Drawer::Engine(engine) => engine,
    };
    let registry = engine.registry().clone();
    let mut last = Instant::now();
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
            progress.frame(last.elapsed());
            last = Instant::now();
        },
    )
}

/// 進み具合の数え。2 パスでは 2 回分を全体として数える。
struct Progress<'a> {
    plan: &'a Plan<'a>,
    started: Instant,
    done: u64,
    total: u64,
}

impl<'a> Progress<'a> {
    fn new(plan: &'a Plan<'a>, frames_in_pass: u64) -> Self {
        Self {
            plan,
            started: Instant::now(),
            done: u64::from(plan.pass.index - 1) * frames_in_pass,
            total: frames_in_pass * u64::from(plan.pass.of),
        }
    }

    /// 1 フレーム送った。描画の時間を `export.render` に載せ、進み具合を知らせる。
    fn frame(&mut self, elapsed: Duration) {
        mm_trace::record_span("export.render", elapsed);
        self.done += 1;
        self.plan.control.report(&ExportProgress {
            target: self.plan.target.clone(),
            frames_done: self.done,
            frames_total: self.total,
            elapsed: self.started.elapsed(),
        });
    }
}

/// 送信の時間をスパン `export.encode` に載せ、取り消されていたら送らずに止めるエンコーダー。
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
        // 空のスライスは NV12 に対応するかの問い合わせなので、取り消しとは関係なく通す。
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

    fn push_audio(&mut self, block: &AudioBlock) -> Result<()> {
        self.inner.push_audio(block)
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        self.inner.finish()
    }
}
