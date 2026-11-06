//! 付属ファイル（`cache/ingest/<hash>.<kind>.json`）の読み書き。外のツール（`tools/`）が書き、ここで検証して読む。

use std::path::Path;

use mm_ingest_api::{
    parse, PreviewMedia, Sidecar, SidecarBody, Transcript, SIDECAR_SCHEMA_VERSION,
};
use mm_types::{MmError, Result};

use crate::layout::{ensure_parent, partial_path, CacheLayout};
use crate::probe::VideoInfo;
use crate::PRODUCER;

/// 付属ファイルを読む。スキーマのバージョンを上げ、素材のコンテンツハッシュが合うことを確かめる。
pub fn read_sidecar(path: &Path, expect_hash: &str) -> Result<Sidecar> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let sidecar = parse(value).map_err(|e| {
        MmError::Corrupt(format!("{}: 付属ファイルが読めない: {e}", path.display()))
    })?;
    if sidecar.content_hash != expect_hash {
        return Err(MmError::InvalidArgument(format!(
            "{}: コンテンツハッシュが違う（{} ではなく {}）",
            path.display(),
            expect_hash,
            sidecar.content_hash
        )));
    }
    Ok(sidecar)
}

/// 素材の文字起こしがあれば読む。無ければ `None`。
pub fn load_transcript(layout: &CacheLayout, hash: &str) -> Result<Option<Transcript>> {
    let path = layout.sidecar(hash, "transcript");
    if !path.is_file() {
        return Ok(None);
    }
    match read_sidecar(&path, hash)?.body {
        SidecarBody::Transcript(t) => Ok(Some(t)),
        _ => Err(MmError::Corrupt(format!(
            "{}: 文字起こしの付属ファイルでない",
            path.display()
        ))),
    }
}

/// プレビュー動画の記録を付属ファイルの控えに書く。`video` はプレビュー動画の映像（大きさを書く）。
pub fn write_preview_record(
    layout: &CacheLayout,
    hash: &str,
    preview: &Path,
    video: &VideoInfo,
) -> Result<()> {
    let sidecar = Sidecar {
        schema_version: SIDECAR_SCHEMA_VERSION,
        content_hash: hash.to_owned(),
        producer: PRODUCER.to_owned(),
        body: SidecarBody::PreviewMedia(PreviewMedia {
            path: layout.relative(preview),
            width: video.width,
            height: video.height,
        }),
    };
    let out = layout.sidecar(hash, "preview_media");
    ensure_parent(&out)?;
    let part = partial_path(&out);
    std::fs::write(&part, serde_json::to_vec_pretty(&sidecar)?)?;
    std::fs::rename(&part, &out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_ingest_api::TranscriptSegment;
    use mm_types::RationalTime;

    fn write_transcript(layout: &CacheLayout, file_hash: &str, content_hash: &str) {
        let s = Sidecar {
            schema_version: SIDECAR_SCHEMA_VERSION,
            content_hash: content_hash.into(),
            producer: "test".into(),
            body: SidecarBody::Transcript(Transcript {
                language: "ja".into(),
                segments: vec![TranscriptSegment {
                    start: RationalTime::ZERO,
                    end: RationalTime::seconds(1),
                    text: "こんにちは".into(),
                    words: Vec::new(),
                }],
            }),
        };
        let path = layout.sidecar(file_hash, "transcript");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(&s).unwrap()).unwrap();
    }

    #[test]
    fn 無ければ_none_あれば読む() {
        let dir = tempfile::tempdir().unwrap();
        let layout = CacheLayout::new(dir.path());
        assert!(load_transcript(&layout, "h").unwrap().is_none());
        write_transcript(&layout, "h", "h");
        let t = load_transcript(&layout, "h").unwrap().unwrap();
        assert_eq!(t.segments[0].text, "こんにちは");
    }

    #[test]
    fn 別の素材のハッシュは拒否する() {
        let dir = tempfile::tempdir().unwrap();
        let layout = CacheLayout::new(dir.path());
        write_transcript(&layout, "h", "other");
        assert!(matches!(
            load_transcript(&layout, "h"),
            Err(MmError::InvalidArgument(_))
        ));
    }
}
