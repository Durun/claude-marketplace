//! 書き出しの途中の制御。取り消し、進み具合、書きかけのファイル。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mm_types::RenderTargetId;

/// 進み具合の通知の関数。描画の途中で呼ばれるので、重い処理を置かない。
pub type ProgressFn = Arc<dyn Fn(&ExportProgress) + Send + Sync>;

/// 1 フレーム押すごとの進み具合。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportProgress {
    pub target: RenderTargetId,
    pub frames_done: u64,
    pub frames_total: u64,
    pub elapsed: Duration,
}

/// 取り消しの旗と進み具合の通知。どちらも無くてよい。
#[derive(Clone, Default)]
pub struct ExportControl {
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<ProgressFn>,
}

impl std::fmt::Debug for ExportControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportControl")
            .field("cancel", &self.cancel)
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

impl ExportControl {
    /// 旗が立ったら、次にエンコーダーへ押す所で書き出しを止める。
    pub fn with_cancel(mut self, flag: Arc<AtomicBool>) -> Self {
        self.cancel = Some(flag);
        self
    }

    /// 進み具合を受け取る関数を付ける。
    pub fn with_progress(mut self, f: ProgressFn) -> Self {
        self.progress = Some(f);
        self
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
    }

    pub(crate) fn report(&self, progress: &ExportProgress) {
        if let Some(f) = &self.progress {
            f(progress);
        }
    }
}

/// 書きかけの出力の置き場所。`out.mp4` なら `out.partial.mp4`（エンコーダーが拡張子で形式を選ぶので拡張子は残す）。
pub(crate) fn partial_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .map_or_else(|| "out".to_owned(), |s| s.to_string_lossy().into_owned());
    let name = match output.extension() {
        Some(ext) => format!("{stem}.partial.{}", ext.to_string_lossy()),
        None => format!("{stem}.partial"),
    };
    output.with_file_name(name)
}

/// 書きかけのファイル。完成して `commit` するまでに落ちたら（エラー、取り消し、パニック）、ドロップで消す。
pub(crate) struct PartialFile {
    path: PathBuf,
    done: bool,
}

impl PartialFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, done: false }
    }

    /// 完成した出力の場所へ移す。同じ名前のファイルがあれば置き換える。
    pub(crate) fn commit(mut self, output: &Path) -> std::io::Result<()> {
        std::fs::rename(&self.path, output)?;
        self.done = true;
        Ok(())
    }
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        if !self.done {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 書きかけの名前は拡張子を残す() {
        assert_eq!(
            partial_path(Path::new("a/out.mp4")),
            Path::new("a/out.partial.mp4")
        );
        assert_eq!(partial_path(Path::new("out")), Path::new("out.partial"));
    }

    #[test]
    fn 完成しなかった書きかけは消え_完成したものは置き換わる() {
        let dir = tempfile::tempdir().unwrap();
        let (out, part) = (dir.path().join("o.bin"), dir.path().join("o.partial.bin"));
        std::fs::write(&out, b"old").unwrap();
        std::fs::write(&part, b"new").unwrap();
        PartialFile::new(part.clone()).commit(&out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert!(!part.exists());

        std::fs::write(&part, b"half").unwrap();
        drop(PartialFile::new(part.clone()));
        assert!(!part.exists(), "ドロップで消える");
        assert_eq!(std::fs::read(&out).unwrap(), b"new", "前の出力は触らない");
    }
}
