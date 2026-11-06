//! ディスク上の名前。キーの文字列をそのままパスにせず、パスに使えない文字は `%XX` にする。

use std::path::{Path, PathBuf};

use mm_cache_api::{CacheKey, CacheKind, ContentHash};

/// 書き込み中のファイルを置くフォルダー。キャッシュの項目として数えない。
pub(crate) const TMP_DIR: &str = ".tmp";

/// 種類のフォルダー名。
pub(crate) fn kind_dir(kind: CacheKind) -> &'static str {
    match kind {
        CacheKind::Preview => "preview",
        CacheKind::Frame => "frame",
        CacheKind::Intermediate => "intermediate",
        CacheKind::AudioPcm => "audio_pcm",
        CacheKind::TextRaster => "text_raster",
        CacheKind::Waveform => "waveform",
        CacheKind::Thumbnail => "thumbnail",
    }
}

const KINDS: [CacheKind; 7] = [
    CacheKind::Preview,
    CacheKind::Frame,
    CacheKind::Intermediate,
    CacheKind::AudioPcm,
    CacheKind::TextRaster,
    CacheKind::Waveform,
    CacheKind::Thumbnail,
];

fn is_plain(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')
}

/// パスの 1 区間にする。英数字と `-` `_` 以外は `%XX`。空は `%`（`%` だけの区間は他に作れない）。
fn encode(s: &str) -> String {
    if s.is_empty() {
        return "%".into();
    }
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if is_plain(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> Option<String> {
    if s == "%" {
        return Some(String::new());
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 項目のファイルのパス。
pub(crate) fn entry_path(root: &Path, key: &CacheKey) -> PathBuf {
    root.join(kind_dir(key.kind))
        .join(encode(&key.content_hash.0))
        .join(encode(&key.variant))
}

/// ファイルのパスからキーを戻す。このクレートが作った名前でなければ `None`。
pub(crate) fn parse_entry(path: &Path, root: &Path) -> Option<CacheKey> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = rel.components().map(|c| c.as_os_str().to_str());
    let kind_name = parts.next()??;
    let hash = parts.next()??;
    let variant = parts.next()??;
    if parts.next().is_some() {
        return None;
    }
    let kind = KINDS.iter().copied().find(|k| kind_dir(*k) == kind_name)?;
    Some(CacheKey {
        kind,
        content_hash: ContentHash(decode(hash)?),
        variant: decode(variant)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 符号化は往復し_パスの区切りを含まない() {
        for s in ["", "abc", "a/b", "..", "日本語", "50%", "a b:c"] {
            let e = encode(s);
            assert!(!e.contains(['/', '\\', ':', '.']), "{e}");
            assert_eq!(decode(&e).as_deref(), Some(s));
        }
    }
}
