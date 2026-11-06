//! コンテンツハッシュ。素材のバイト列の BLAKE3（小文字の 16 進、64 文字）。
//!
//! BLAKE3 を選ぶ理由: SHA-256 より桁違いに速く（SIMD と木構造で、ディスクの速度が先に頭打ちになる）、
//! mmap と rayon で大きな素材を複数コアで読める。暗号学的な強度は SHA-256 と同じ目的（改ざんされない衝突の少なさ）に足りる。
//! 16 進のままキャッシュのフォルダー名に使うので、`:` などのパスに使えない文字を入れない。

use std::path::Path;

use mm_types::Result;

/// ファイル全体のコンテンツハッシュ。
pub fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    if hasher.update_mmap_rayon(path).is_err() {
        // パイプや mmap できない場所の下のファイルは、普通の読み込みに戻す。
        hasher = blake3::Hasher::new();
        hasher.update_reader(std::fs::File::open(path)?)?;
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同じ中身は同じハッシュで_違う中身は違う() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a.bin"),
            dir.path().join("b.bin"),
            dir.path().join("c.bin"),
        );
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"hello").unwrap();
        std::fs::write(&c, b"hellp").unwrap();
        let (ha, hb, hc) = (
            hash_file(&a).unwrap(),
            hash_file(&b).unwrap(),
            hash_file(&c).unwrap(),
        );
        assert_eq!(ha, hb);
        assert_ne!(ha, hc);
        assert_eq!(ha.len(), 64);
        // tools/transcribe/test_transcribe.py が同じ値を確かめる（Python 側と同じハッシュになる）。
        assert_eq!(
            ha,
            "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f"
        );
    }

    #[test]
    fn 無いファイルはエラー() {
        assert!(hash_file(Path::new("no/such/file")).is_err());
    }
}
