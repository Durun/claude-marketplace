//! Review パネルの裏側。提案の一覧と決定は `mm_serve::Core` を通し、Program のプレビューの切り替え
//! （提案を当てた絵と元の絵）は、再生スレッドが描くプロジェクトのデータだけを差し替える。
//!
//! プレビュー中も保存したデータと編集のキューは元のまま。提案を当てたデータは描画だけに使い、
//! 保存した変更が届くたびに当て直す（後の編集と重なって当てられなくなれば、元の絵に戻す）。

use std::sync::{Arc, Mutex};

use mm_doc::Project;

use crate::playback::Command;
use crate::state::Shared;

/// Program に当てて見せている提案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub id: i64,
    /// 当てる操作の番号。`None` は全部。
    pub ops: Option<Vec<usize>>,
}

/// プレビューの状態と、最後に当てた操作が触った所（元の絵に戻すときにキャッシュを捨てる範囲）。
#[derive(Debug, Default)]
pub struct PreviewSlot {
    current: Mutex<Option<(Preview, Vec<String>)>>,
}

impl PreviewSlot {
    fn take(&self) -> Option<(Preview, Vec<String>)> {
        self.current.lock().expect("preview").take()
    }

    /// いま当てている提案。
    pub fn get(&self) -> Option<Preview> {
        self.current
            .lock()
            .expect("preview")
            .as_ref()
            .map(|(p, _)| p.clone())
    }

    fn set(&self, preview: Preview, touched: Vec<String>) {
        *self.current.lock().expect("preview") = Some((preview, touched));
    }
}

/// 再生スレッドが描くプロジェクトのデータ。プレビュー中なら提案を当てたもの、当てられなければ `None`（元の絵）。
pub fn previewed(shared: &Shared) -> Option<Arc<Project>> {
    let preview = shared.review_preview.get()?;
    let core = shared.core.get()?;
    match core.preview_project(preview.id, preview.ops.as_deref()) {
        Ok((project, touched)) => {
            shared.review_preview.set(preview, touched);
            Some(Arc::new(project))
        }
        Err(e) => {
            tracing::warn!("提案 {} を当てられないので元の絵に戻す: {e}", preview.id);
            shared.review_preview.take();
            None
        }
    }
}

/// プレビューを切り替えて描き直させる。`None` で元の絵に戻す。前後の両方の提案が触る所のキャッシュを捨てる。
pub fn set_preview(shared: &Shared, preview: Option<Preview>) -> Result<(), String> {
    let mut touched = shared
        .review_preview
        .take()
        .map(|(_, t)| t)
        .unwrap_or_default();
    if let Some(p) = preview {
        let core = shared.core.get().ok_or_else(no_store)?;
        let (_, now) = core
            .preview_project(p.id, p.ops.as_deref())
            .map_err(|e| e.to_string())?;
        touched.extend(now.iter().cloned());
        shared.review_preview.set(p, now);
    }
    shared
        .reload_touched
        .lock()
        .expect("reload")
        .extend(touched);
    shared.send(Command::Reload);
    Ok(())
}

/// 提案は `project.mmdb` の表にあるので、JSON のプロジェクトでは使えない。
pub fn no_store() -> String {
    "提案は project.mmdb のプロジェクトでだけ使える".to_owned()
}
