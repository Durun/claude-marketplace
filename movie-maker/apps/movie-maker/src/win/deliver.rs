//! Deliver パネルのアプリ側の処理。レンダーターゲットと sequence の一覧、書き出しのキュー（順に書き出す専用スレッド）、
//! 進み具合の状態。Tauri のコマンドのハンドラーは `deliver_ipc.rs`、名前と引数の形は `ui/sdk/README.md` の「Deliver のコマンド」。
//!
//! 書き出しは再生のスレッドとは別の `Engine`（別の GPU のデバイス）で回すので、再生を止めない。
//! 進み具合は、キュー全体の状態（`ExportStatus`）をそのまま送る。画面は受けた状態を描くだけで、差分を畳む処理を持たない。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mm_doc::{Project, RenderTarget};
use mm_export::{
    export_run, reject_shared_outputs, verify_output, CaptionWord, Expected, ExportControl,
    ExportOptions, ExportOutcome, ExportProgress, ExportSummary, ProgressFn, TranscriptWords,
    Verification,
};
use mm_runtime::{assemble, RuntimeConfig};
use mm_store::Store;
use mm_types::{Id, MmError, RenderTargetId, Size};
use serde::{Deserialize, Serialize};

use crate::win::playback::runtime_config;
use crate::win::state::Shared;

mod guards;
mod progress;

pub use guards::output_extensions;
use guards::{ffprobe_available, pick, stop_all};
use progress::progress_reporter;

/// 進み具合を画面へ送る間隔（ミリ秒）。1 フレームごとに送ると WebView の側が詰まる。
const EMIT_INTERVAL_MS: u64 = 100;

/// 書き出しのレンダラーが VRAM に持つフレームのキャッシュの上限。再生のレンダラーも別に 1 GiB を持つので、
/// 同時に持つ合計が膨らまないよう、順に進める書き出しでは小さくする。
const EXPORT_FRAME_MEMORY_BYTES: u64 = 256 << 20;

/// コンポジション 1 つ。レンダーターゲットの `composition` の選択肢と、範囲の表示に使う。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompositionInfo {
    pub id: String,
    pub duration: f64,
    /// 縦長のレンダーターゲット（Shorts）が、既定で縦長のコンポジションを指すために使う。
    pub width: u32,
    pub height: u32,
}

/// sequence 1 つ。選ぶとレンダーターゲットのコンポジションの代わりに、全長を書き出す。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SequenceInfo {
    pub id: String,
    pub duration: f64,
}

/// Deliver を開いたときに要る一覧。
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub targets: Vec<RenderTarget>,
    pub compositions: Vec<CompositionInfo>,
    pub sequences: Vec<SequenceInfo>,
    /// いま開いているコンポジション。レンダーターゲットを足すときの既定の `composition`。
    pub open_composition: Option<String>,
    /// 書き出したあとの ffprobe の検証が使えるか。モックのエンコーダー（FFmpeg なしのビルド）では使えない。
    pub can_verify: bool,
}

/// キューの 1 項目の状態。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// ffprobe が読んだ出力の中身。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VerificationView {
    pub video_codec: String,
    pub width: u32,
    pub height: u32,
    pub video_frames: u64,
    pub video_seconds: f64,
    pub audio_seconds: Option<f64>,
}

impl From<&Verification> for VerificationView {
    fn from(v: &Verification) -> Self {
        Self {
            video_codec: v.video_codec.clone(),
            width: v.size.width,
            height: v.size.height,
            video_frames: v.video_frames,
            video_seconds: v.video_seconds,
            audio_seconds: v.audio_seconds,
        }
    }
}

/// キューの 1 項目。時間は秒。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExportItem {
    pub target: String,
    pub state: ItemState,
    pub frames_done: u64,
    pub frames_total: u64,
    pub fps: f64,
    pub elapsed: f64,
    /// 残りの時間の見込み。まだ測れないときは `None`。
    pub eta: Option<f64>,
    pub output_path: String,
    pub bytes_written: u64,
    pub verification: Option<VerificationView>,
    pub error: Option<String>,
}

impl ExportItem {
    fn queued(target: &RenderTarget) -> Self {
        Self {
            target: target.id.to_string(),
            state: ItemState::Queued,
            frames_done: 0,
            frames_total: 0,
            fps: 0.0,
            elapsed: 0.0,
            eta: None,
            output_path: target.output_path.clone(),
            bytes_written: 0,
            verification: None,
            error: None,
        }
    }
}

/// 書き出しのキュー全体の状態。進み具合のイベントの中身。
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ExportStatus {
    pub running: bool,
    pub items: Vec<ExportItem>,
}

/// 書き出しの依頼（`export_start` の引数）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StartRequest {
    /// 書き出すレンダーターゲットの ID。この順に書き出す。
    pub targets: Vec<String>,
    /// 指すとレンダーターゲットのコンポジションの代わりに、この sequence を全長で書き出す。
    #[serde(default)]
    pub sequence: Option<String>,
    /// 書き出したあとに ffprobe で検証する。
    #[serde(default)]
    pub verify: bool,
}

/// 状態が変わったときに呼ぶ関数。書き出しのスレッドから呼ばれる。
pub type Emit = Arc<dyn Fn(&ExportStatus) + Send + Sync>;

/// 素材ごとの字幕にする単語。消した単語は除き、直した文字を使う。DB が無い、読めないときは空
/// （空だと書き出しは字幕なしの警告で終わる）。書き出しとプレビューの字幕の焼き込みが同じ元を読む。
pub(crate) fn transcript_words(store_dir: Option<&Path>, project: &Project) -> TranscriptWords {
    let mut out = TranscriptWords::new();
    let Some(store) = store_dir.and_then(|d| Store::open_read_only(d).ok()) else {
        return out;
    };
    for asset in &project.assets {
        let words: Vec<CaptionWord> = store
            .transcript_words(&asset.id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|w| {
                let text = w.shown()?.to_owned();
                Some(CaptionWord {
                    start: w.start,
                    end: w.end,
                    text,
                })
            })
            .collect();
        if !words.is_empty() {
            out.insert(asset.id.clone(), words);
        }
    }
    out
}

/// Deliver の状態。アプリに 1 つ持つ。
pub struct DeliverState {
    shared: Arc<Shared>,
    /// 相対の出力先の基準（`rebase_project` が使ったフォルダー）。画面へ返すときに相対へ戻す。
    base: PathBuf,
    /// `project.mmdb` のフォルダー。文字起こしの単語を読む。JSON のプロジェクトでは `None`。
    store_dir: Option<PathBuf>,
    status: Arc<Mutex<ExportStatus>>,
    cancel: Mutex<Arc<AtomicBool>>,
    /// 書き出し終えたファイル。フォルダーを開く先をここに入っているものだけに絞る。
    outputs: Arc<Mutex<HashSet<PathBuf>>>,
}

impl DeliverState {
    /// `project` は開いたプロジェクトのパス（JSON か `project.mmdb` のフォルダー）。
    pub fn new(shared: Arc<Shared>, project: &Path) -> Self {
        let store_dir = mm_store::store_dir(project);
        let base = store_dir
            .clone()
            .unwrap_or_else(|| project.parent().unwrap_or(Path::new(".")).to_path_buf());
        Self {
            shared,
            base,
            store_dir,
            status: Arc::default(),
            cancel: Mutex::new(Arc::default()),
            outputs: Arc::default(),
        }
    }

    fn project(&self) -> Result<Arc<Project>, String> {
        self.shared
            .doc()
            .map(|d| d.project)
            .ok_or_else(|| "プロジェクトを開いていない".to_owned())
    }

    /// レンダーターゲット、コンポジション、sequence の一覧。
    pub fn overview(&self) -> Result<Overview, String> {
        let project = self.project()?;
        let compositions = project
            .compositions
            .iter()
            .map(|c| CompositionInfo {
                id: c.id.to_string(),
                duration: c.duration.to_f64(),
                width: c.width,
                height: c.height,
            })
            .collect();
        let sequences = project
            .sequences
            .iter()
            .map(|s| SequenceInfo {
                id: s.id.to_string(),
                duration: mm_eval::sequence_duration(s).map_or(0.0, |d| d.to_f64()),
            })
            .collect();
        Ok(Overview {
            targets: project
                .render_targets
                .iter()
                .map(|t| self.relative_target(t))
                .collect(),
            compositions,
            sequences,
            open_composition: self.shared.doc().map(|d| d.comp.to_string()),
            can_verify: cfg!(feature = "ffmpeg") && ffprobe_available(),
        })
    }

    /// 相対の出力先を、開くときに直す前の形へ戻す。画面が編集で書き戻しても、相対のまま保たれる。
    fn relative_target(&self, target: &RenderTarget) -> RenderTarget {
        let mut target = target.clone();
        if let Ok(rest) = Path::new(&target.output_path).strip_prefix(&self.base) {
            target.output_path = rest.to_string_lossy().into_owned();
        }
        target
    }

    /// 字幕を出すレンダーターゲットがキューにあるときだけ、文字起こしの単語を読む。`mm render` と同じ元（DB の単語）。
    fn transcript(
        &self,
        project: &Project,
        targets: &[RenderTarget],
    ) -> Option<Arc<TranscriptWords>> {
        targets
            .iter()
            .any(|t| t.delivery.needs_transcript())
            .then(|| Arc::new(transcript_words(self.store_dir.as_deref(), project)))
    }

    /// いまのキューの状態。
    pub fn status(&self) -> ExportStatus {
        self.status.lock().expect("export status").clone()
    }

    /// キューを作って書き出しのスレッドを始め、すぐ返る。書き出し中にもう一度呼ぶと `Err`。
    pub fn start(
        &self,
        request: StartRequest,
        config: RuntimeConfig,
        emit: Emit,
    ) -> Result<(), String> {
        let project = self.project()?;
        let targets = pick(&project, &request)?;
        let transcript = self.transcript(&project, &targets);
        let mut status = self.status.lock().expect("export status");
        if status.running {
            return Err("書き出し中".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().expect("export cancel") = cancel.clone();
        *status = ExportStatus {
            running: true,
            items: targets.iter().map(ExportItem::queued).collect(),
        };
        let queued = status.clone();
        drop(status);
        emit(&queued);
        let job = Job {
            project,
            request,
            transcript,
            config,
            cancel,
            status: self.status.clone(),
            outputs: self.outputs.clone(),
            emit: emit.clone(),
        };
        std::thread::Builder::new()
            .name("export".into())
            .spawn(move || job.run())
            .map_err(|e| {
                let message = e.to_string();
                let stopped = stop_all(&self.status, &message);
                emit(&stopped);
                message
            })?;
        Ok(())
    }

    /// 書き出しを取り消す。書き出し中の 1 本は次のフレームで止まり、残りは書き出さない。
    pub fn cancel(&self) {
        self.cancel
            .lock()
            .expect("export cancel")
            .store(true, Ordering::Relaxed);
    }

    /// フォルダーを開いてよいパスか確かめて返す。この書き出しで作ったファイルだけを通す。
    pub fn revealable(&self, path: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(path);
        if self.outputs.lock().expect("export outputs").contains(&path) {
            Ok(path)
        } else {
            Err(format!("書き出したファイルではない: {}", path.display()))
        }
    }
}

/// 書き出しのスレッドが持つもの。
struct Job {
    project: Arc<Project>,
    request: StartRequest,
    /// 字幕を出すレンダーターゲットがあるときの文字起こしの単語。
    transcript: Option<Arc<TranscriptWords>>,
    config: RuntimeConfig,
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<ExportStatus>>,
    outputs: Arc<Mutex<HashSet<PathBuf>>>,
    emit: Emit,
}

/// 抜けるとき（パニックも）に、キューを止まった状態にして知らせる。
struct Finish<'a>(&'a Job);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        let stopped = stop_all(&self.0.status, "書き出しが途中で止まった");
        (self.0.emit)(&stopped);
    }
}

impl Job {
    fn run(&self) {
        let _finish = Finish(self);
        let mut engine = match assemble(&self.config) {
            Ok(engine) => engine,
            Err(e) => return self.fail_all(&e),
        };
        for index in 0..self.request.targets.len() {
            if self.cancel.load(Ordering::Relaxed) {
                return self.cancel_rest(index);
            }
            self.update(index, |item| item.state = ItemState::Running);
            let options = self.options(index);
            match export_run(&mut engine, &self.project, &options) {
                Ok(ExportOutcome::Completed(summary)) => self.done(index, &summary),
                Ok(ExportOutcome::Cancelled) => return self.cancel_rest(index),
                Err(e) => self.update(index, |item| {
                    item.state = ItemState::Failed;
                    item.error = Some(e.to_string());
                }),
            }
        }
    }

    fn options(&self, index: usize) -> ExportOptions {
        let progress = progress_reporter(self.status.clone(), self.emit.clone(), index);
        ExportOptions {
            target: Some(Id::new(self.request.targets[index].as_str())),
            sequence: self.request.sequence.as_deref().map(Id::new),
            verify: self.request.verify,
            transcript: self.transcript.clone(),
            control: ExportControl::default()
                .with_cancel(self.cancel.clone())
                .with_progress(progress),
            ..ExportOptions::default()
        }
    }

    fn done(&self, index: usize, summary: &ExportSummary) {
        self.outputs
            .lock()
            .expect("export outputs")
            .insert(summary.output_path.clone());
        self.update(index, |item| {
            item.state = ItemState::Done;
            item.frames_done = summary.frames;
            item.frames_total = summary.frames;
            item.fps = summary.fps;
            item.elapsed = summary.elapsed.as_secs_f64();
            item.eta = Some(0.0);
            item.output_path = summary.output_path.to_string_lossy().into_owned();
            item.bytes_written = summary.bytes_written;
            item.verification = summary.verification.as_ref().map(Into::into);
        });
    }

    /// `from` 番目から後を取り消しにする。
    fn cancel_rest(&self, from: usize) {
        let mut status = self.status.lock().expect("export status");
        for item in status.items.iter_mut().skip(from) {
            item.state = ItemState::Cancelled;
            item.eta = None;
        }
    }

    fn fail_all(&self, e: &MmError) {
        let mut status = self.status.lock().expect("export status");
        for item in &mut status.items {
            item.state = ItemState::Failed;
            item.error = Some(e.to_string());
        }
    }

    fn update(&self, index: usize, f: impl FnOnce(&mut ExportItem)) {
        let snapshot = {
            let mut status = self.status.lock().expect("export status");
            if let Some(item) = status.items.get_mut(index) {
                f(item);
            }
            status.clone()
        };
        (self.emit)(&snapshot);
    }
}

/// 書き出しに使う実装の組み合わせ。再生と同じで、サーフェスには出さない（`present` を切る）。
pub fn export_config() -> RuntimeConfig {
    RuntimeConfig {
        present: false,
        frame_memory_bytes: Some(EXPORT_FRAME_MEMORY_BYTES),
        ..runtime_config()
    }
}

#[cfg(test)]
mod tests;
