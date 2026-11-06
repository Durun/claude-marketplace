//! Deliver パネルのアプリ側の処理。レンダーターゲットと sequence の一覧、書き出しの列（順に書き出す専用スレッド）、
//! 進み具合の状態。Tauri のコマンドのハンドラーは `deliver_ipc.rs`、名前と引数の形は `ui/sdk/README.md` の「Deliver のコマンド」。
//!
//! 書き出しは再生のスレッドとは別の `Engine`（別の GPU のデバイス）で回すので、再生を止めない。
//! 進み具合は、列全体の状態（`ExportStatus`）をそのまま送る。画面は受けた状態を描くだけで、差分を畳む処理を持たない。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mm_doc::{Project, RenderTarget};
use mm_export::{
    export_run, ExportControl, ExportOptions, ExportOutcome, ExportProgress, ExportSummary,
    Verification,
};
use mm_runtime::{assemble, RuntimeConfig};
use mm_types::{Id, MmError};
use serde::{Deserialize, Serialize};

use crate::playback::runtime_config;
use crate::state::Shared;

/// 進み具合を画面へ送る間隔（ミリ秒）。1 フレームごとに送ると WebView の側が詰まる。
const EMIT_INTERVAL_MS: u64 = 100;

/// コンポジション 1 つ。レンダーターゲットの `composition` の選択肢と、範囲の表示に使う。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompositionInfo {
    pub id: String,
    pub duration: f64,
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

/// 列の 1 項目の状態。
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

/// 列の 1 項目。時間は秒。
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

/// 書き出しの列全体の状態。進み具合のイベントの中身。
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

/// Deliver の状態。アプリに 1 つ持つ。
pub struct DeliverState {
    shared: Arc<Shared>,
    status: Arc<Mutex<ExportStatus>>,
    cancel: Mutex<Arc<AtomicBool>>,
    /// 書き出し終えたファイル。フォルダーを開く先をここに入っているものだけに絞る。
    outputs: Arc<Mutex<HashSet<PathBuf>>>,
}

impl DeliverState {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
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
            targets: project.render_targets.clone(),
            compositions,
            sequences,
            open_composition: self.shared.doc().map(|d| d.comp.to_string()),
            can_verify: cfg!(feature = "ffmpeg"),
        })
    }

    /// いまの列の状態。
    pub fn status(&self) -> ExportStatus {
        self.status.lock().expect("export status").clone()
    }

    /// 列を作って書き出しのスレッドを始め、すぐ返る。書き出し中にもう一度呼ぶと `Err`。
    pub fn start(
        &self,
        request: StartRequest,
        config: RuntimeConfig,
        emit: Emit,
    ) -> Result<(), String> {
        let project = self.project()?;
        let targets = pick(&project, &request)?;
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
            config,
            cancel,
            status: self.status.clone(),
            outputs: self.outputs.clone(),
            emit,
        };
        std::thread::Builder::new()
            .name("export".into())
            .spawn(move || job.run())
            .map_err(|e| {
                self.status.lock().expect("export status").running = false;
                e.to_string()
            })?;
        Ok(())
    }

    /// 書き出しを取り消す。走っている 1 本は次のフレームで止まり、残りは書き出さない。
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

/// 依頼のレンダーターゲットと sequence が在るか、同じ出力先を指す組が無いかを確かめて、列の順に返す。
/// 同じ出力先に順に書くと、先に書いた成果物を黙って上書きするため。
fn pick(project: &Project, request: &StartRequest) -> Result<Vec<RenderTarget>, String> {
    if request.targets.is_empty() {
        return Err("書き出すレンダーターゲットを選んでいない".into());
    }
    if let Some(sequence) = &request.sequence {
        project
            .sequence(&Id::new(sequence.as_str()))
            .map_err(|e| e.to_string())?;
    }
    let mut picked: Vec<RenderTarget> = Vec::new();
    for id in &request.targets {
        let target = project
            .render_target(&Id::new(id.as_str()))
            .map_err(|e| e.to_string())?
            .clone();
        if let Some(other) = picked.iter().find(|t| t.output_path == target.output_path) {
            return Err(format!(
                "レンダーターゲット {} と {} が同じ出力先 {} を指している",
                other.id, target.id, target.output_path
            ));
        }
        picked.push(target);
    }
    Ok(picked)
}

/// 書き出しのスレッドが持つもの。
struct Job {
    project: Arc<Project>,
    request: StartRequest,
    config: RuntimeConfig,
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<ExportStatus>>,
    outputs: Arc<Mutex<HashSet<PathBuf>>>,
    emit: Emit,
}

/// 抜けるとき（パニックも）に、列を止まった状態にして知らせる。
struct Finish<'a>(&'a Job);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        let mut status = self.0.status.lock().expect("export status");
        status.running = false;
        for item in &mut status.items {
            if matches!(item.state, ItemState::Queued | ItemState::Running) {
                item.state = ItemState::Failed;
                item.error
                    .get_or_insert_with(|| "書き出しが途中で止まった".into());
            }
        }
        let stopped = status.clone();
        drop(status);
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
        let status = self.status.clone();
        let emit = self.emit.clone();
        let started = Instant::now();
        let last_ms = AtomicU64::new(0);
        let progress = move |p: &ExportProgress| {
            let snapshot = {
                let mut status = status.lock().expect("export status");
                if let Some(item) = status.items.get_mut(index) {
                    apply_progress(item, p);
                }
                status.clone()
            };
            let now = started.elapsed().as_millis() as u64;
            let finished = p.frames_done >= p.frames_total;
            if finished || now.saturating_sub(last_ms.load(Ordering::Relaxed)) >= EMIT_INTERVAL_MS {
                last_ms.store(now, Ordering::Relaxed);
                emit(&snapshot);
            }
        };
        ExportOptions {
            target: Some(Id::new(self.request.targets[index].as_str())),
            sequence: self.request.sequence.as_deref().map(Id::new),
            verify: self.request.verify,
            control: ExportControl::default()
                .with_cancel(self.cancel.clone())
                .with_progress(Arc::new(progress)),
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

fn apply_progress(item: &mut ExportItem, p: &ExportProgress) {
    item.frames_done = p.frames_done;
    item.frames_total = p.frames_total;
    item.elapsed = p.elapsed.as_secs_f64();
    item.fps = p.frames_done as f64 / item.elapsed.max(f64::EPSILON);
    item.eta = (p.frames_done > 0 && item.fps > 0.0)
        .then(|| p.frames_total.saturating_sub(p.frames_done) as f64 / item.fps);
}

/// 書き出しに使う実装の組み合わせ。再生と同じで、サーフェスには出さない（`present` を切る）。
pub fn export_config() -> RuntimeConfig {
    RuntimeConfig {
        present: false,
        ..runtime_config()
    }
}

#[cfg(test)]
mod tests;
