//! Review パネルの裏側。提案の一覧と決定は `mm_serve::Core` を通し、Program のプレビューの切り替え
//! （提案を適用したフレームと元のフレーム）は、再生スレッドが描くプロジェクトのデータだけを差し替える。
//!
//! プレビュー中も保存したデータと編集のキューは元のまま。提案を適用したデータは描画だけに使い、
//! 保存した変更が届くたびに適用し直す（後の編集と重なって適用できなくなれば、元のフレームに戻す）。

use std::sync::{Arc, Mutex};

use mm_doc::Project;
use mm_serve::Core;
use mm_store::ProposalState;

use crate::win::playback::Command;
use crate::win::state::Shared;

/// Program に適用して見せている提案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub id: i64,
    /// 適用する操作の番号。`None` は全部。
    pub ops: Option<Vec<usize>>,
}

/// プレビューの状態と、最後に適用した操作が触った所（元のフレームに戻すときにキャッシュを捨てる範囲）。
#[derive(Debug, Default)]
pub struct PreviewSlot {
    current: Mutex<Option<(Preview, Vec<String>)>>,
}

impl PreviewSlot {
    fn take(&self) -> Option<(Preview, Vec<String>)> {
        self.current.lock().expect("preview").take()
    }

    /// いま適用している提案。
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

    /// `preview` がまだ適用している提案のときだけ、触った所を置き直す。再生スレッドが適用し直している間に
    /// Review が切り替えた・外したプレビューを、古い値で書き戻さないため。
    fn refresh(&self, preview: &Preview, touched: Vec<String>) -> bool {
        let mut current = self.current.lock().expect("preview");
        match current.as_mut() {
            Some((p, t)) if p == preview => {
                *t = touched;
                true
            }
            _ => false,
        }
    }

    /// `preview` がまだ適用している提案のときだけ外し、それが触った所を返す。
    fn drop_if(&self, preview: &Preview) -> Vec<String> {
        let mut current = self.current.lock().expect("preview");
        match current.as_ref() {
            Some((p, _)) if p == preview => current.take().map(|(_, t)| t).unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

/// 保留中の提案だけを適用する。承認した提案はデータが既に含むので、適用すると操作が二重に効く。
fn apply_pending(core: &Core, preview: &Preview) -> Result<(Project, Vec<String>), String> {
    let proposal = core.proposal(preview.id).map_err(|e| e.to_string())?;
    if proposal.state != ProposalState::Pending {
        return Err(format!("提案 {} は保留中でない", preview.id));
    }
    core.preview_project(preview.id, preview.ops.as_deref())
        .map_err(|e| e.to_string())
}

/// 再生スレッドが描くプロジェクトのデータ。プレビュー中なら提案を適用したもの、適用できなければ `None`（元のフレーム）。
/// 適用できなくなったプレビューを外したときは、それが触った所を `touched` に足す（提案を適用したフレームのキャッシュを捨てる）。
pub fn previewed(shared: &Shared, touched: &mut Vec<String>) -> Option<Arc<Project>> {
    let preview = shared.review_preview.get()?;
    let core = shared.core.get()?;
    match apply_pending(core, &preview) {
        Ok((project, now)) => {
            touched.extend(now.iter().cloned());
            shared
                .review_preview
                .refresh(&preview, now)
                .then(|| Arc::new(project))
        }
        Err(e) => {
            tracing::warn!(
                "提案 {} を適用できないので元のフレームに戻す: {e}",
                preview.id
            );
            touched.extend(shared.review_preview.drop_if(&preview));
            None
        }
    }
}

/// プレビューを切り替えて描き直させる。`None` で元のフレームに戻す。前後の両方の提案が触る所のキャッシュを捨てる。
/// 新しい提案を適用できなければ、今のプレビューのまま失敗を返す。
pub fn set_preview(shared: &Shared, preview: Option<Preview>) -> Result<(), String> {
    let next = match preview {
        Some(p) => {
            let core = shared.core.get().ok_or_else(no_store)?;
            let (_, now) = apply_pending(core, &p)?;
            Some((p, now))
        }
        None => None,
    };
    let mut touched = shared
        .review_preview
        .take()
        .map(|(_, t)| t)
        .unwrap_or_default();
    if let Some((p, now)) = next {
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

/// 承認で適用したトランザクションを取り消す（Ctrl+Z と同じ経路）。確かめてから取り消すまでに別の編集が
/// 積まれたときは、そちらを戻したことになるので、戻した後も提案が適用されたままなら失敗を返す。
pub fn undo_approved(
    shared: &Shared,
    core: &Core,
    id: i64,
) -> Result<mm_serve::ProposalView, String> {
    let before = core.proposal(id).map_err(|e| e.to_string())?;
    if !before.undoable {
        return Err(format!("提案 {id} の承認は、いま取り消せる位置に無い"));
    }
    crate::win::edit::step_saved(shared, core, true);
    let after = core.proposal(id).map_err(|e| e.to_string())?;
    if after.applied {
        return Err(format!(
            "提案 {id} の承認を取り消せなかった（先に別の編集が積まれた）"
        ));
    }
    Ok(after)
}

/// 提案は `project.mmdb` の表にあるので、JSON のプロジェクトでは使えない。
pub fn no_store() -> String {
    "提案は project.mmdb のプロジェクトでだけ使える".to_owned()
}

#[cfg(test)]
mod tests;
