//! 再生スレッドの本体。時計の位置から出すフレームを決め、音を先読みして出力に足す。

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_audio_api::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_audio_cpal::AudioOutput;
use mm_doc::{Composition, Project};
use mm_node_api::Quality;
use mm_ops::{apply_transaction, Entry, Op, OpContext};
use mm_runtime::{assemble, Backend, Engine, RuntimeConfig};
use mm_types::{CompositionId, Rational, RationalTime, Result, Size, TimeRange};

use super::intake::{collect_batch, fit_chunk};
use super::{Command, PlaybackInfo};
use crate::win::clock::{AudioClock, PlayClock, SwitchClock, WallClock};
use crate::win::edit::EditOutcome;
use crate::win::history::HistoryMirror;
use crate::win::state::{FrameImage, Shared};

/// 先読みする音の長さ（秒）。映像 1 フレームの描画が数十 ms かかっても音が途切れない長さ。
const AUDIO_AHEAD_SECS: f64 = 0.4;
/// 1 回に描く音の長さ（サンプル）。0.1 秒。
const AUDIO_CHUNK: i64 = 4_800;
/// 次のフレームまでの待ちの下限と上限（ms）。上限は音の先読みを回す間隔でもある。
const WAIT_MIN_MS: f64 = 0.5;
const WAIT_MAX_MS: f64 = 4.0;
/// 映像を音より先のフレームで出す量（秒）。フレームを描いて取り込み、present するまでに要る時間（実測で約 25 ms）を
/// 見込まないと、映像が音より 1 フレーム近く遅れて見える。
const VIDEO_LEAD_SECS: f64 = 0.025;
/// 終わりの判定に許す誤差（秒）。
const END_EPSILON: f64 = 1e-3;

/// 再生スレッドの設定。
#[derive(Debug, Clone, Default)]
pub struct PlayerOptions {
    /// 音の出力を開かず、壁時計で進める。
    pub no_audio: bool,
    /// 実装の組み合わせ。`None` なら [`runtime_config`]。
    pub config: Option<RuntimeConfig>,
    /// `project.mmdb` のフォルダー。字幕の焼き込みの文字起こしの単語を読む。JSON のプロジェクトでは `None`。
    pub store_dir: Option<PathBuf>,
}

/// 実際の実装の組み合わせ。FFmpeg が切ってあるときは素材をモックのデコーダーで読む。
pub fn runtime_config() -> RuntimeConfig {
    let real = RuntimeConfig {
        present: true,
        ..RuntimeConfig::real()
    };
    if cfg!(feature = "ffmpeg") {
        real
    } else {
        RuntimeConfig {
            decode: Backend::Sample,
            encode: Backend::Sample,
            ..real
        }
    }
}

/// 時計の位置から出すフレームの番号と、ドロップした数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePlan {
    pub index: i64,
    pub dropped: i64,
}

/// 位置 `pos`（秒）に出すフレームを決める。`last` は直前に出した番号。まだ同じなら `None`。
pub fn plan_frame(pos: f64, last: Option<i64>, fps: f64, total: i64) -> Option<FramePlan> {
    let index = crate::win::timebase::frame_index(pos, fps).clamp(0, (total - 1).max(0));
    match last {
        Some(l) if index <= l => None,
        Some(l) => Some(FramePlan {
            index,
            dropped: index - l - 1,
        }),
        None => Some(FramePlan { index, dropped: 0 }),
    }
}

/// 逆向き再生。始めた位置と時刻から、壁時計で戻る。
struct Reverse {
    from: f64,
    since: Instant,
}

pub struct Player {
    engine: Engine,
    project: Arc<Project>,
    comp: CompositionId,
    fps: Rational,
    fps_f: f64,
    frames_total: i64,
    duration: f64,
    size: Size,
    clock: Arc<SwitchClock>,
    audio: Option<Arc<AudioOutput>>,
    shared: Arc<Shared>,
    playing: bool,
    last_frame: Option<i64>,
    audio_cursor: i64,
    reverse: Option<Reverse>,
    /// 適用した編集の逆操作。`Undo` で後ろから適用する。
    undo: Vec<Vec<Op>>,
    /// 取り消した編集の元の操作。`Redo` で後ろから適用する。
    redo: Vec<Vec<Op>>,
    store_dir: Option<PathBuf>,
    /// 字幕を焼き込んだプロジェクトのデータ。元の `project` が同じ間は作り直さない。
    burned: Option<(Arc<Project>, Arc<Project>)>,
}

impl Player {
    pub fn new(
        shared: Arc<Shared>,
        project: Arc<Project>,
        comp_id: CompositionId,
        options: PlayerOptions,
    ) -> Result<(Self, PlaybackInfo)> {
        let mut engine = assemble(&options.config.clone().unwrap_or_else(runtime_config))?;
        *shared.gpu.lock().expect("gpu") = engine.gpu_context();
        let comp = project.composition(&comp_id)?;
        let (fps, duration) = (comp.fps, comp.duration.to_f64());
        let size = Size::new(comp.width, comp.height);
        let audio = (!options.no_audio).then(open_audio).flatten();
        let inner: Arc<dyn PlayClock> = match &audio {
            Some(a) => Arc::new(AudioClock(a.clone())),
            None => Arc::new(WallClock::default()),
        };
        let clock = Arc::new(SwitchClock::new(inner));
        let _ = shared.clock.set(clock.clone());
        match shared.core.get().and_then(|c| c.version().ok()) {
            Some(version) => {
                shared.publish_saved(version, project.clone(), comp_id.clone());
            }
            None => shared.publish_doc(project.clone(), comp_id.clone()),
        }
        let fps_f = fps.to_f64();
        let info = describe(&engine, comp, clock.as_ref(), audio.as_deref());
        let player = Self {
            engine,
            project,
            comp: comp_id,
            fps,
            fps_f,
            frames_total: (duration * fps_f).round() as i64,
            duration,
            size,
            clock,
            audio,
            shared,
            playing: false,
            last_frame: None,
            audio_cursor: 0,
            reverse: None,
            undo: Vec::new(),
            redo: Vec::new(),
            store_dir: options.store_dir,
            burned: None,
        };
        Ok((player, info))
    }

    /// コマンドを受けながら再生を進める。`Quit` か、送り手が全部いなくなったら返る。
    pub fn run(&mut self, rx: &Receiver<Command>) {
        self.seek_to(0.0);
        'serve: loop {
            let wait = if self.reverse.is_some() {
                Some(Duration::from_secs_f64(1.0 / self.fps_f))
            } else if self.playing {
                Some(self.next_wait())
            } else {
                None
            };
            let received = match wait {
                Some(d) => rx.recv_timeout(d),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Err(RecvTimeoutError::Disconnected) => break,
                Ok(first) => {
                    for cmd in collect_batch(first, rx) {
                        if cmd == Command::Quit {
                            break 'serve;
                        }
                        self.handle(cmd);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.reverse.is_some() {
                self.reverse_tick();
            } else if self.playing {
                self.tick();
            }
        }
        self.clock.halt();
        self.shared.playing.store(false, Ordering::SeqCst);
    }

    fn next_wait(&self) -> Duration {
        let pos = self.clock.position();
        let to_next = ((pos * self.fps_f).floor() + 1.0) / self.fps_f - pos;
        Duration::from_secs_f64((to_next * 1000.0 - 1.0).clamp(WAIT_MIN_MS, WAIT_MAX_MS) / 1000.0)
    }

    fn handle(&mut self, cmd: Command) {
        let keeps_reverse = matches!(
            cmd,
            Command::Reverse | Command::Edit | Command::Undo | Command::Redo | Command::Quit
        );
        if !keeps_reverse {
            self.reverse = None;
        }
        match cmd {
            Command::Play => self.play(),
            Command::Pause => self.pause(),
            Command::Toggle if self.playing => self.pause(),
            Command::Toggle => self.play(),
            Command::Seek(t) => self.seek_to(t),
            Command::Step(n) => {
                let here = self.pause_position();
                let idx = crate::win::timebase::frame_index(here, self.fps_f) + n;
                self.seek_to(idx as f64 / self.fps_f);
            }
            Command::Edit => self.apply_edits(),
            Command::Reverse => self.start_reverse(),
            Command::Undo => self.step_history(true),
            Command::Redo => self.step_history(false),
            Command::Reload => self.reload(),
            Command::Quit => {}
        }
    }

    /// 積まれた操作を順に適用し、結果を返す。適用したら今のフレームを描き直す。
    fn apply_edits(&mut self) {
        let edits = std::mem::take(&mut *self.shared.edits.lock().expect("edits"));
        for edit in edits {
            if edit.claim() {
                let _ = edit.reply.send(self.apply(&edit.ops));
            }
        }
    }

    /// 操作を適用して、取り消しの履歴に積む。新しい編集はやり直しの履歴を捨てる。
    fn apply(&mut self, ops: &[Op]) -> std::result::Result<EditOutcome, String> {
        let (outcome, inverse) = self.commit(ops)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(outcome)
    }

    /// 履歴の最後を適用して、逆の履歴に積む。`undo` が真なら取り消し、偽ならやり直し。
    fn step_history(&mut self, undo: bool) {
        // 保存している間は、ストアの取り消しとやり直し（`project.mmdb` に記録される）を使う。描き直しは変更の通知で行う。
        if let Some(core) = self.shared.core.get() {
            return crate::win::edit::step_saved(&self.shared, core, undo);
        }
        let stack = if undo { &mut self.undo } else { &mut self.redo };
        let Some(ops) = stack.pop() else {
            let message = HistoryMirror::message(undo, Ok(None), None);
            return self.shared.edit.notify(message);
        };
        match self.commit(&ops) {
            Ok((_, inverse)) => {
                let other = if undo { &mut self.redo } else { &mut self.undo };
                other.push(inverse);
                self.shared.edit.notify(String::new());
            }
            Err(e) => self
                .shared
                .edit
                .set_message(HistoryMirror::message(undo, Err(e), None)),
        }
    }

    /// 操作を 1 つのトランザクションとして適用し、触った ID に頼ったキャッシュだけを捨てる。逆操作も返す。
    fn commit(&mut self, ops: &[Op]) -> std::result::Result<(EditOutcome, Vec<Op>), String> {
        let mut project = (*self.project).clone();
        let ops = mm_ops::with_fitted_layers(&project, ops);
        let outcome = apply_transaction(&mut project, &ops, &OpContext::new(Entry::WebView))
            .map_err(|e| e.to_string())?;
        let touched = outcome.touches.to_vec();
        let inverse = outcome.inverse_ops().map_err(|e| e.to_string())?;
        let project = Arc::new(project);
        self.adopt_project(project.clone(), &touched)?;
        self.shared.publish_doc(project, self.comp.clone());
        Ok((EditOutcome { touched }, inverse))
    }

    /// 保存した変更（HTTP サーバーと IPC が書いたもの）を描画に取り込む。データは変更を受けた側が
    /// `Shared::doc` に置き直してから `Reload` を送るので、ここはそれを再生するデータにする。
    fn reload(&mut self) {
        let mut touched = std::mem::take(&mut *self.shared.reload_touched.lock().expect("reload"));
        let Some(doc) = self.shared.doc() else { return };
        let project =
            crate::win::review::previewed(&self.shared, &mut touched).unwrap_or(doc.project);
        if let Err(e) = self.adopt_project(project, &touched) {
            tracing::warn!("保存した変更を描画に取り込めない: {e}");
        }
    }

    /// `project` を再生するプロジェクトのデータにし、触った ID に頼ったキャッシュだけを捨てて描き直す。
    fn adopt_project(
        &mut self,
        project: Arc<Project>,
        touched: &[String],
    ) -> std::result::Result<(), String> {
        let comp = project
            .composition(&self.comp)
            .map_err(|_| "開いているコンポジションを消す操作は適用しない".to_owned())?;
        let (fps, duration) = (comp.fps, comp.duration.to_f64());
        let size = Size::new(comp.width, comp.height);
        self.engine.invalidate_touched(touched);
        self.project = project;
        self.adopt_shape(fps, duration, size);
        self.resync();
        Ok(())
    }

    /// 今の位置から逆向きに戻り始める。音は出さない。
    fn start_reverse(&mut self) {
        let here = self.pause_position();
        self.reverse = Some(Reverse {
            from: here,
            since: Instant::now(),
        });
    }

    /// 壁時計で戻った位置のフレームを出す。先頭に着いたら止まる。
    fn reverse_tick(&mut self) {
        let Some(r) = &self.reverse else { return };
        let pos = (r.from - r.since.elapsed().as_secs_f64()).max(0.0);
        let at_start = pos <= 0.0;
        let idx = crate::win::timebase::frame_index(pos, self.fps_f)
            .clamp(0, (self.frames_total - 1).max(0));
        if self.last_frame != Some(idx) {
            self.clock.seek(pos);
            self.render_frame(idx);
            self.last_frame = Some(idx);
        }
        if at_start {
            self.reverse = None;
        }
    }

    /// コンポジションの fps、長さ、大きさを引き直す。操作で変わっても、再生の計算が古い値のままにならない。
    fn adopt_shape(&mut self, fps: Rational, duration: f64, size: Size) {
        self.fps = fps;
        self.fps_f = fps.to_f64();
        self.duration = duration;
        self.frames_total = (duration * self.fps_f).round() as i64;
        self.size = size;
    }

    /// 内容が変わった後に、今の位置を描き直す。再生中は先読みの音も古いので、位置を置き直して音を作り直す。
    fn resync(&mut self) {
        if self.playing {
            let at = self.clock.position();
            self.seek_to(at);
        } else if let Some(index) = self.last_frame {
            let index = index.clamp(0, (self.frames_total - 1).max(0));
            self.render_frame(index);
            self.last_frame = Some(index);
        }
    }

    fn pause_position(&mut self) -> f64 {
        if self.playing {
            self.pause();
        }
        self.clock.position()
    }

    fn last_start(&self) -> f64 {
        (self.duration - 1.0 / self.fps_f).max(0.0)
    }

    /// 音の出力が壊れていたら、止めた位置から壁時計に替えて音を諦める。
    fn check_audio(&mut self) {
        if self.audio.is_some() && self.clock.fall_back_if_failed() {
            tracing::warn!("音の出力が止まったので、壁時計で進める");
            self.audio = None;
        }
    }

    fn play(&mut self) {
        if self.playing {
            return;
        }
        self.check_audio();
        let here = self.clock.position();
        let from = if here >= self.last_start() { 0.0 } else { here };
        self.seek_to(from);
        self.feed_audio();
        self.clock.run();
        self.playing = true;
        self.shared.playing.store(true, Ordering::SeqCst);
    }

    fn pause(&mut self) {
        if !self.playing {
            return;
        }
        self.clock.halt();
        self.playing = false;
        self.shared.playing.store(false, Ordering::SeqCst);
    }

    /// 位置を置き直して、その位置のフレームを描く。再生中なら音も置き直して続ける。
    fn seek_to(&mut self, t: f64) {
        let was_playing = self.playing;
        if was_playing {
            self.clock.halt();
        }
        let t = t.clamp(0.0, self.last_start());
        self.clock.seek(t);
        self.audio_cursor = (t * f64::from(DEFAULT_SAMPLE_RATE)).round() as i64;
        self.shared.meters.lock().expect("meters").reset();
        let idx = crate::win::timebase::frame_index(t, self.fps_f)
            .clamp(0, (self.frames_total - 1).max(0));
        self.render_frame(idx);
        self.last_frame = Some(idx);
        if was_playing {
            self.feed_audio();
            self.clock.run();
        }
    }

    fn tick(&mut self) {
        self.check_audio();
        self.feed_audio();
        let pos = self.clock.position();
        if pos >= self.duration - END_EPSILON {
            self.finish();
            return;
        }
        let Some(plan) = plan_frame(
            pos + VIDEO_LEAD_SECS,
            self.last_frame,
            self.fps_f,
            self.frames_total,
        ) else {
            return;
        };
        if plan.dropped > 0 {
            self.shared
                .dropped_frames
                .fetch_add(plan.dropped as u64, Ordering::SeqCst);
            mm_trace::counter!("preview.dropped_frames", plan.dropped);
        }
        self.render_frame(plan.index);
        self.last_frame = Some(plan.index);
    }

    /// 終わりまで来た。最後のフレームを出して止める。
    fn finish(&mut self) {
        self.clock.halt();
        self.playing = false;
        self.shared.playing.store(false, Ordering::SeqCst);
        let last = (self.frames_total - 1).max(0);
        if self.last_frame != Some(last) {
            self.render_frame(last);
            self.last_frame = Some(last);
        }
        self.clock.seek(self.last_start());
    }

    fn render_frame(&mut self, index: i64) {
        let Ok(time) = RationalTime::from_frames(index, self.fps) else {
            return;
        };
        let started = std::time::Instant::now();
        let result = {
            let _span = tracing::info_span!("preview.render_frame").entered();
            self.draw(time)
        };
        match result {
            Ok(img) => {
                self.shared.frames.put(index as f64 / self.fps_f, img);
                self.shared.rendered_frames.fetch_add(1, Ordering::SeqCst);
                self.shared
                    .record("render_frame_ms", started.elapsed().as_secs_f64() * 1000.0);
            }
            Err(e) => {
                mm_trace::counter!("preview.render_errors", 1);
                tracing::warn!(error = %e, index, "フレームを描けなかった");
            }
        }
    }

    /// 1 フレーム描く。wgpu のレンダラーはリードバックせずにテクスチャで返し、それ以外は CPU の画像にする。
    fn draw(&mut self, time: RationalTime) -> Result<FrameImage> {
        let shown = self.shown_project();
        let (project, comp, size) = (&*shown, &self.comp, self.size);
        if self.shared.cpu_frames.load(Ordering::SeqCst) || self.engine.gpu_context().is_none() {
            return self
                .engine
                .render_frame(project, comp, time, size, Quality::Preview)
                .map(FrameImage::Cpu);
        }
        self.engine
            .render_frame_texture(project, comp, time, size, Quality::Preview)
            .map(FrameImage::Gpu)
    }

    /// 映像に描くプロジェクトのデータ。字幕を焼き込むレンダーターゲットがあれば、書き出しと同じ関数で字幕を足したもの。
    pub(super) fn shown_project(&mut self) -> Arc<Project> {
        if !mm_export::burn_in_enabled(&self.project, &self.comp) {
            return self.project.clone();
        }
        if let Some((from, shown)) = &self.burned {
            if Arc::ptr_eq(from, &self.project) {
                return shown.clone();
            }
        }
        let words = crate::win::deliver::transcript_words(self.store_dir.as_deref(), &self.project);
        let shaper = self.engine.services().text_shaper.clone();
        let shown = match mm_export::burn_in_captions(&self.project, &self.comp, &words, &*shaper) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                tracing::warn!(error = %e, "字幕を焼き込めない。字幕なしで描く");
                self.project.clone()
            }
        };
        self.burned = Some((self.project.clone(), shown.clone()));
        shown
    }

    /// 音を先読みの長さまで足す。描けなかったチャンクは無音で埋めて時計を止めない。
    fn feed_audio(&mut self) {
        let Some(out) = self.audio.clone() else {
            return;
        };
        let total = (self.duration * f64::from(DEFAULT_SAMPLE_RATE)).round() as i64;
        while self.audio_cursor < total && out.buffered_secs() < AUDIO_AHEAD_SECS {
            let n = AUDIO_CHUNK.min(total - self.audio_cursor);
            let block = self.render_audio_chunk(self.audio_cursor, n);
            // 形の違う音は無音に替えて足す。足せずに止めると、音の位置が進まず時計が終わりに届かない。
            let block = fit_chunk(block, n as usize);
            let start = self.audio_cursor as f64 / f64::from(DEFAULT_SAMPLE_RATE);
            let levels = self.engine.audio_levels();
            let mixers = self.engine.mixers().clone();
            let mut meters = self.shared.meters.lock().expect("meters");
            meters.record(mixers.as_ref(), start, &block, levels);
            drop(meters);
            if let Err(e) = out.push(&block) {
                tracing::warn!(error = %e, "音を足せなかった。時計を進めるためスキップする");
            }
            self.audio_cursor += n;
            if self.audio_cursor >= total {
                // 最後の音を足した時点の数。これ以降の増加は、音を鳴らし切った後の無音。
                self.shared
                    .record("audio_underruns_before_tail", self.clock.underruns() as f64);
            }
        }
    }

    fn render_audio_chunk(&mut self, start: i64, frames: i64) -> mm_audio_api::AudioBlock {
        let rate = DEFAULT_SAMPLE_RATE;
        let range = TimeRange::new(
            RationalTime::new(i128::from(start), rate).unwrap_or(RationalTime::ZERO),
            RationalTime::new(i128::from(start + frames), rate).unwrap_or(RationalTime::ZERO),
        );
        let started = std::time::Instant::now();
        let _span = tracing::info_span!("preview.render_audio").entered();
        let rendered = range.and_then(|r| self.engine.render_audio(&self.project, &self.comp, r));
        self.shared
            .record("render_audio_ms", started.elapsed().as_secs_f64() * 1000.0);
        rendered.unwrap_or_else(|e| {
            mm_trace::counter!("preview.audio_errors", 1);
            tracing::warn!(error = %e, start, "音を描けなかった");
            mm_audio_api::AudioBlock::silence(rate, DEFAULT_CHANNELS, frames as usize)
        })
    }
}

/// 開いたコンポジションと選ばれた実装の情報。
fn describe(
    engine: &Engine,
    comp: &Composition,
    clock: &SwitchClock,
    audio: Option<&AudioOutput>,
) -> PlaybackInfo {
    PlaybackInfo {
        duration: comp.duration.to_f64(),
        fps: comp.fps.to_f64(),
        width: comp.width,
        height: comp.height,
        clock: clock.kind(),
        audio_device: audio.map(|a| {
            let (name, rate, ch) = a.device();
            format!("{name} ({rate} Hz, {ch} ch)")
        }),
        backends: engine
            .backend_names()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
    }
}

fn open_audio() -> Option<Arc<AudioOutput>> {
    match AudioOutput::open(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS) {
        Ok(out) => Some(Arc::new(out)),
        Err(e) => {
            tracing::warn!(error = %e, "音の出力を開けない。壁時計で進める");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同じフレームの間は何も出さない() {
        assert_eq!(plan_frame(0.01, Some(0), 30.0, 300), None);
        assert_eq!(plan_frame(0.034, Some(0), 30.0, 300).unwrap().index, 1);
    }

    #[test]
    fn 追いつけなかった分はドロップした数に数える() {
        let p = plan_frame(0.2, Some(2), 30.0, 300).unwrap();
        assert_eq!((p.index, p.dropped), (6, 3));
    }

    #[test]
    fn 最初のフレームはドロップに数えない() {
        let p = plan_frame(1.0, None, 30.0, 300).unwrap();
        assert_eq!((p.index, p.dropped), (30, 0));
    }

    #[test]
    fn 終わりを超えた位置は最後のフレームに収める() {
        let p = plan_frame(99.0, Some(10), 30.0, 300).unwrap();
        assert_eq!(p.index, 299);
    }
}
