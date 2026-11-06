//! `cache/` の下の置き場所（`docs/design/09-ingest-storage.md` の「保存の形」）。
//! 素材のコンテンツハッシュごとにフォルダーを分けるので、プロジェクトを別の場所へコピーしても当たる。

use std::path::{Path, PathBuf};

/// プレビュー動画のファイル名（高さ 540 の既定）。
pub const PREVIEW_FILE: &str = "540p.mp4";
/// 波形の付属ファイル名。
pub const WAVEFORM_FILE: &str = "waveform.json";
/// サムネイルのファイル名。
pub const THUMBNAIL_FILE: &str = "thumb.jpg";

/// `cache/` のルート。
#[derive(Debug, Clone)]
pub struct CacheLayout {
    root: PathBuf,
}

impl CacheLayout {
    /// `root` は `<project>/cache`。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `cache/` のルート。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// プレビュー動画。
    pub fn preview(&self, hash: &str) -> PathBuf {
        self.root.join("preview").join(hash).join(PREVIEW_FILE)
    }

    /// 波形（付属ファイルの形）。
    pub fn waveform(&self, hash: &str) -> PathBuf {
        self.root.join("audio").join(hash).join(WAVEFORM_FILE)
    }

    /// サムネイル。
    pub fn thumbnail(&self, hash: &str) -> PathBuf {
        self.root.join("thumbs").join(hash).join(THUMBNAIL_FILE)
    }

    /// 付属ファイルのパス。`kind` は付属ファイルの種類（`transcript`、`preview_media`、`scenes`）。
    pub fn sidecar(&self, hash: &str, kind: &str) -> PathBuf {
        self.root.join("ingest").join(format!("{hash}.{kind}.json"))
    }

    /// `cache/` からの相対パス（`/` 区切り）。付属ファイルに書く。
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

/// 出力先の親フォルダーを作る。
pub(crate) fn ensure_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(dir) => std::fs::create_dir_all(dir),
        None => Ok(()),
    }
}

/// 書きかけのファイルの名前。完成してから `rename` で置くので、途中で止まった出力を完成品と取り違えない。
pub(crate) fn partial_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 置き場所は設計書どおり() {
        let c = CacheLayout::new("p/cache");
        assert_eq!(c.preview("h"), Path::new("p/cache/preview/h/540p.mp4"));
        assert_eq!(c.waveform("h"), Path::new("p/cache/audio/h/waveform.json"));
        assert_eq!(c.thumbnail("h"), Path::new("p/cache/thumbs/h/thumb.jpg"));
        assert_eq!(
            c.sidecar("h", "transcript"),
            Path::new("p/cache/ingest/h.transcript.json")
        );
        assert_eq!(c.relative(&c.preview("h")), "preview/h/540p.mp4");
    }
}
