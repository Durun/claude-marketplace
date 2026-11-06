//! 書き手の `Store` を 1 本の列で持つ中心。HTTP も、アプリの IPC も、ここを通って操作を適用する。
//!
//! 適用は `Mutex` で順に並べ、書けたら変わった所を購読者（SSE、アプリの描画、WebView）に送る。

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use mm_doc::Project;
use mm_ops::{Entry, OpContext, OpError, Transaction};
use mm_store::{Actor, Store, StoredTxn, TxnKind};
use mm_types::MmError;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::frames::{FrameRequest, FrameSource};

/// 通知を貯めておく数。遅い購読者はこれを超えると取りこぼし、最新のバージョンから読み直す。
const EVENT_BACKLOG: usize = 256;

/// 操作を送ってきた側。操作経路の種類（ロックの扱いが変わる）と、記録に残す名前。
#[derive(Debug, Clone)]
pub struct Caller {
    pub entry: Entry,
    pub actor: Actor,
}

impl Caller {
    /// アプリの WebView（人の操作）。
    pub fn webview() -> Self {
        Self {
            entry: Entry::WebView,
            actor: Actor {
                kind: "gui".into(),
                name: "app".into(),
            },
        }
    }
}

/// 適用したトランザクションと、その結果。応答にも変更の通知にも同じ形を使う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// 適用した後のプロジェクトのデータのバージョン。
    pub version: i64,
    pub txn_id: i64,
    /// `apply`、`undo`、`redo`。
    pub kind: &'static str,
    pub name: String,
    /// 誰が（`gui`、`cli`、`claude` など）。
    pub actor: String,
    /// 触った ID。描き直す範囲を決めるのに使う。
    pub touched: Vec<String>,
}

impl Change {
    fn of(stored: &StoredTxn) -> Self {
        let kind = match stored.kind {
            TxnKind::Normal => "apply",
            TxnKind::Undo { .. } => "undo",
            TxnKind::Redo { .. } => "redo",
        };
        let touched: BTreeSet<&String> = stored
            .ops
            .iter()
            .flat_map(|o| o.record.touches.iter())
            .collect();
        Self {
            version: stored.doc_version,
            txn_id: stored.id,
            kind,
            name: stored.name.clone(),
            actor: stored.actor.kind.clone(),
            touched: touched.into_iter().cloned().collect(),
        }
    }
}

/// 操作の適用に失敗した理由。HTTP の状態コードと対応する。
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// 操作の層が拒否した（衝突、ロック、検証の失敗など）。
    #[error(transparent)]
    Op(OpError),
    /// 要求の形が違う。
    #[error("{0}")]
    BadRequest(String),
    /// 閉じた後、または読み取りだけで開いている。
    #[error("{0}")]
    Unavailable(String),
    /// 保存や描画の失敗。
    #[error("{0}")]
    Internal(String),
}

impl From<OpError> for ServeError {
    fn from(e: OpError) -> Self {
        match e {
            OpError::Doc(MmError::Unsupported(m)) => Self::Unavailable(m),
            other => Self::Op(other),
        }
    }
}

impl From<MmError> for ServeError {
    fn from(e: MmError) -> Self {
        match e {
            MmError::Unsupported(m) => Self::Unavailable(m),
            MmError::InvalidArgument(m) | MmError::NotFound(m) => Self::BadRequest(m),
            other => Self::Internal(other.to_string()),
        }
    }
}

/// `GET /project` の中身。
#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    pub version: i64,
    pub undo_depth: i64,
    pub project: Project,
}

/// 書き手の `Store` と、変更の通知、描画の出し口。
pub struct Core {
    store: Mutex<Option<Store>>,
    events: broadcast::Sender<Change>,
    frames: Option<Arc<dyn FrameSource>>,
}

impl Core {
    /// 書き手として開いた `Store` を預かる。`frames` が無ければ `GET /frame` は 503。
    pub fn new(store: Store, frames: Option<Arc<dyn FrameSource>>) -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_BACKLOG);
        Arc::new(Self {
            store: Mutex::new(Some(store)),
            events,
            frames,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<Store>>, ServeError> {
        self.store
            .lock()
            .map_err(|_| ServeError::Internal("保存の列が壊れた".into()))
    }

    fn with_store<T>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T, ServeError>,
    ) -> Result<T, ServeError> {
        let mut guard = self.lock()?;
        let store = guard
            .as_mut()
            .ok_or_else(|| ServeError::Unavailable("プロジェクトを閉じた".into()))?;
        f(store)
    }

    /// 変更の通知を受ける。`recv` / `blocking_recv` で読む。
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.events.subscribe()
    }

    /// 変更のたびに `f` を呼ぶスレッドを始める。アプリが描き直しや画面への通知に使う。
    /// 取りこぼした分は呼ばない（遅い `f` は `EVENT_BACKLOG` を超えると古い通知を失う）。
    pub fn on_change(
        &self,
        mut f: impl FnMut(Change) + Send + 'static,
    ) -> std::io::Result<std::thread::JoinHandle<()>> {
        let mut rx = self.subscribe();
        std::thread::Builder::new()
            .name("serve-changes".into())
            .spawn(move || loop {
                match rx.blocking_recv() {
                    Ok(change) => f(change),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("変更の通知を {n} 件取りこぼした");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            })
    }

    /// 今のバージョン。
    pub fn version(&self) -> Result<i64, ServeError> {
        self.with_store(|s| Ok(s.doc_version()))
    }

    /// トランザクションを 1 本の列で適用して書き、変わった所を送る。
    /// `txn.base_version` が古ければ、保存した記録の `touches` で衝突を判定する。
    pub fn apply(
        &self,
        txn: Transaction,
        caller: &Caller,
        responds_to: Option<String>,
    ) -> Result<Change, ServeError> {
        let change = self.with_store(|s| {
            let ctx = OpContext::new(caller.entry);
            let stored = s.apply_txn(txn, &ctx, caller.actor.clone(), now_ms(), responds_to)?;
            Ok(self.finish(s, &stored))
        })?;
        Ok(change)
    }

    /// 直前のトランザクションを取り消す。無ければ `None`。
    pub fn undo(&self, caller: &Caller) -> Result<Option<Change>, ServeError> {
        self.with_store(|s| {
            let ctx = OpContext::new(caller.entry);
            let stored = s.undo_last(&ctx, caller.actor.clone(), now_ms())?;
            Ok(stored.map(|t| self.finish(s, &t)))
        })
    }

    /// 取り消したトランザクションをやり直す。無ければ `None`。
    pub fn redo(&self, caller: &Caller) -> Result<Option<Change>, ServeError> {
        self.with_store(|s| {
            let ctx = OpContext::new(caller.entry);
            let stored = s.redo_last(&ctx, caller.actor.clone(), now_ms())?;
            Ok(stored.map(|t| self.finish(s, &t)))
        })
    }

    /// 書いた後の仕事。間隔が過ぎたバックアップ、描画側のキャッシュの無効化、購読者への通知。
    fn finish(&self, store: &mut Store, stored: &StoredTxn) -> Change {
        if let Err(e) = store.backup_if_due(now_ms()) {
            tracing::warn!("バックアップを作れない: {e}");
        }
        let change = Change::of(stored);
        if let Some(frames) = &self.frames {
            frames.invalidate(&change.touched);
        }
        // 購読者がいなければ送り先が無いだけなので、失敗は無視する。
        let _ = self.events.send(change.clone());
        change
    }

    /// 保存したままのプロジェクトのデータ（素材のパスはフォルダーからの相対）。
    pub fn project(&self) -> Result<ProjectView, ServeError> {
        self.with_store(|s| {
            Ok(ProjectView {
                version: s.doc_version(),
                undo_depth: s.undo_depth(),
                project: s.project().clone(),
            })
        })
    }

    /// 描画に使うプロジェクトのデータ。素材のパスをプロジェクトのフォルダーからの絶対に直す。
    pub fn render_project(&self) -> Result<(i64, Project), ServeError> {
        self.with_store(|s| {
            let mut project = s.project().clone();
            mm_store::rebase_project(&mut project, s.dir());
            Ok((s.doc_version(), project))
        })
    }

    /// 1 フレームを PNG で返す。描画は呼び出し側のスレッドで行う（長いので `Store` の列を止めない）。
    pub fn frame_png(&self, request: &FrameRequest) -> Result<Vec<u8>, ServeError> {
        let frames = self
            .frames
            .as_ref()
            .ok_or_else(|| ServeError::Unavailable("この入口は描画を持たない".into()))?;
        let (_, project) = self.render_project()?;
        frames
            .frame_png(&project, request)
            .map_err(ServeError::from)
    }

    /// 閉じる。バックアップと WAL の書き戻しをして、以後の要求は 503 にする。
    pub fn close(&self) -> Result<(), ServeError> {
        let mut guard = self.lock()?;
        if let Some(store) = guard.take() {
            // 公開した入口の情報は、Store を落とすときにロックと一緒に消える。
            let _ = store.unpublish_http();
            store.close(now_ms())?;
        }
        Ok(())
    }

    /// 公開する前後に `Store` を触るための入口（`publish_http_port` など）。
    pub(crate) fn publish(&self, port: u16, token: &str) -> Result<(), ServeError> {
        self.with_store(|s| {
            s.publish_token(token)?;
            s.publish_http_port(port)?;
            Ok(())
        })
    }

    pub(crate) fn unpublish(&self) {
        let _ = self.with_store(|s| Ok(s.unpublish_http()?));
    }
}

/// 今の時刻（UNIX のミリ秒）。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
