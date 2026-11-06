//! `project.mmdb` の取り消しとやり直しの文言。取り消しは発行元を問わずトランザクションごとに戻すので
//! （`08-ui.md` の「取り消し」）、Claude や CLI の操作を戻したときは、誰の何を戻したかを画面に出す。
//!
//! Store は戻すトランザクションの発行元を返さないので、変更の通知から取り消しのスタックをコピーして持ち、戻す前に先頭を見る。

use std::sync::Mutex;

use mm_serve::Change;

/// 人の操作（アプリの WebView、キー、ドラッグ）の発行元の種類。`mm_serve::Caller::webview` と同じ。
const GUI_ACTOR: &str = "gui";

/// コピーしたトランザクション 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    actor: String,
    name: String,
}

/// 変更の通知からコピーした、取り消しとやり直しのスタック。開く前からの履歴は知らないので、空なら発行元は分からない。
#[derive(Default)]
pub struct HistoryMirror {
    stacks: Mutex<Stacks>,
}

#[derive(Default)]
struct Stacks {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// コピーした最後のトランザクションの ID。適用した側と変更の通知の両方から届くので、2 度目を捨てる。
    last_txn: i64,
}

impl HistoryMirror {
    /// 変更を 1 つコピーする。コピーしたことのあるトランザクションは捨てる。
    pub fn record(&self, change: &Change) {
        let mut guard = self.stacks.lock().expect("history");
        if change.txn_id <= guard.last_txn {
            return;
        }
        guard.last_txn = change.txn_id;
        let Stacks { undo, redo, .. } = &mut *guard;
        match change.kind {
            "undo" => move_top(undo, redo),
            "redo" => move_top(redo, undo),
            _ => {
                undo.push(Entry {
                    actor: change.actor.clone(),
                    name: change.name.clone(),
                });
                redo.clear();
            }
        }
    }

    /// 取り消し（`undo` が偽ならやり直し）の結果の文言。`done` は Store が返した結果。
    /// `before` は戻す前に [`Self::next`] で見た発行元と名前。
    pub fn message(
        undo: bool,
        done: Result<Option<Change>, String>,
        before: Option<(String, String)>,
    ) -> String {
        let verb = if undo { "取り消" } else { "やり直" };
        match done {
            Err(e) => format!("{verb}せない: {e}"),
            Ok(None) => format!("{verb}すものが無い"),
            Ok(Some(change)) => match before {
                Some((actor, name)) if actor != GUI_ACTOR => {
                    format!("{actor} の操作を{verb}した: {name}")
                }
                Some((_, name)) => format!("{verb}した: {name}"),
                None => format!("{verb}した: {}", change.name),
            },
        }
    }

    /// 次に取り消す（`undo` が偽ならやり直す）トランザクションの発行元と名前。知らなければ `None`。
    pub fn next(&self, undo: bool) -> Option<(String, String)> {
        let guard = self.stacks.lock().expect("history");
        let stack = if undo { &guard.undo } else { &guard.redo };
        stack.last().map(|e| (e.actor.clone(), e.name.clone()))
    }
}

fn move_top(from: &mut Vec<Entry>, to: &mut Vec<Entry>) {
    if let Some(e) = from.pop() {
        to.push(e);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    /// 試験ごとに増えるトランザクションの ID。コピーする側は前より大きい ID だけを受ける。
    static NEXT_TXN: AtomicI64 = AtomicI64::new(1);

    fn change(kind: &'static str, actor: &str, name: &str) -> Change {
        Change {
            version: 1,
            txn_id: NEXT_TXN.fetch_add(1, Ordering::SeqCst),
            kind,
            name: name.into(),
            actor: actor.into(),
            touched: Vec::new(),
            of: None,
        }
    }

    #[test]
    fn claude_の操作を取り消すと書き手と名前を出す() {
        let h = HistoryMirror::default();
        h.record(&change("apply", "gui", "WebView の操作"));
        h.record(&change("apply", "mcp", "extract k2"));
        let before = h.next(true);
        let done = Ok(Some(change("undo", "gui", "取り消し: extract k2")));
        assert_eq!(
            HistoryMirror::message(true, done, before),
            "mcp の操作を取り消した: extract k2"
        );
        h.record(&change("undo", "gui", "取り消し: extract k2"));
        assert_eq!(h.next(true), Some(("gui".into(), "WebView の操作".into())));
        assert_eq!(h.next(false), Some(("mcp".into(), "extract k2".into())));
    }

    #[test]
    fn 戻すものが無いときと失敗したときも文言を返す() {
        assert_eq!(
            HistoryMirror::message(true, Ok(None), None),
            "取り消すものが無い"
        );
        assert_eq!(
            HistoryMirror::message(false, Err("壊れた".into()), None),
            "やり直せない: 壊れた"
        );
        let done = Ok(Some(change("undo", "gui", "取り消し: 前の起動の操作")));
        assert_eq!(
            HistoryMirror::message(true, done, None),
            "取り消した: 取り消し: 前の起動の操作"
        );
    }

    #[test]
    fn 同じトランザクションを_2_度写しても列は_1_つ分だけ動く() {
        let h = HistoryMirror::default();
        let a = change("apply", "cli", "a");
        h.record(&a);
        h.record(&a);
        let undone = change("undo", "gui", "取り消し: a");
        h.record(&undone);
        h.record(&undone);
        assert_eq!((h.next(true), h.next(false).is_some()), (None, true));
    }

    #[test]
    fn 新しい操作はやり直しの列を捨てる() {
        let h = HistoryMirror::default();
        h.record(&change("apply", "gui", "a"));
        h.record(&change("undo", "gui", "取り消し: a"));
        h.record(&change("apply", "cli", "b"));
        assert_eq!(h.next(false), None);
    }
}
