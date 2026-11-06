//! `.cube` の 3D LUT の取り込み。映像の素材と同じく `contentHash`（BLAKE3）で素材を決め、プレビューなどは作らない。
//!
//! `color.lut` のキャッシュのキーは素材の `contentHash` に頼るので、中身が変われば別の素材（別の ID）になる。
//! 色の計算は `.cube` を読む側が持つ。ここは壊れたファイルを取り込む前に弾く最小の検査だけを持つ。

use std::path::Path;

use mm_doc::Asset;
use mm_types::{ColorSpace, MmError, RationalTime, Result};

use crate::asset_id;

/// LUT のファイルの拡張子。
const CUBE_EXTENSION: &str = "cube";
/// 3D LUT の 1 辺の点の数を宣言する行の先頭。
const SIZE_KEYWORD: &str = "LUT_3D_SIZE";
/// 1 辺の点の数の下限と上限。`size^3` 行を数えるので、桁違いに大きい値で溢れさせない。
const MIN_LUT_SIZE: usize = 2;
const MAX_LUT_SIZE: usize = 256;

/// 拡張子が `.cube` か。
pub fn is_cube(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(CUBE_EXTENSION))
}

/// `.cube` の中身が 3D LUT として最低限整っているか確かめる（`LUT_3D_SIZE n` と `n^3` 行の RGB）。
pub fn validate_cube(text: &str) -> Result<()> {
    let mut size: Option<usize> = None;
    let mut rows = 0usize;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix(SIZE_KEYWORD) {
            size = rest
                .trim()
                .parse()
                .ok()
                .filter(|n| (MIN_LUT_SIZE..=MAX_LUT_SIZE).contains(n));
            if size.is_none() {
                return Err(corrupt(&format!("{SIZE_KEYWORD} の値が不正: {line}")));
            }
        } else if line.split_whitespace().all(|t| t.parse::<f32>().is_ok()) {
            rows += 1;
        }
    }
    let n = size.ok_or_else(|| corrupt(&format!("{SIZE_KEYWORD} が無い（1D LUT は未対応）")))?;
    let expected = n * n * n;
    if rows != expected {
        return Err(corrupt(&format!(
            "{n}^3 = {expected} 行の RGB が要るのに {rows} 行"
        )));
    }
    Ok(())
}

fn corrupt(reason: &str) -> MmError {
    MmError::Corrupt(format!(".cube: {reason}"))
}

/// プロジェクトに足す LUT の素材。ID はコンテンツハッシュから決まり、色空間は作業色空間（LUT 側が入力の空間を持つ）。
pub fn build_lut_asset(path: &Path, hash: &str) -> Asset {
    Asset {
        id: asset_id(hash),
        path: path.to_string_lossy().into_owned(),
        content_hash: hash.to_owned(),
        color_space: ColorSpace::working(),
        duration: RationalTime::ZERO,
        fps: None,
        timebase_offset: RationalTime::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVERT: &str = include_str!("../../../fixtures/luts/invert.cube");

    #[test]
    fn 拡張子は大文字小文字を区別しない() {
        assert!(is_cube(Path::new("a/Look.CUBE")));
        assert!(!is_cube(Path::new("a/look.mp4")));
        assert!(!is_cube(Path::new("cube")));
    }

    #[test]
    fn 整った_cube_は通り_壊れたものは弾く() {
        validate_cube(INVERT).unwrap();
        assert!(
            validate_cube("LUT_3D_SIZE 2\n0 0 0\n").is_err(),
            "行が足りない"
        );
        assert!(validate_cube("0 0 0\n").is_err(), "サイズが無い");
        assert!(validate_cube("LUT_3D_SIZE x\n").is_err());
        assert!(validate_cube("LUT_3D_SIZE 99999999\n").is_err());
        assert!(
            validate_cube("LUT_1D_SIZE 4\n0 0 0\n").is_err(),
            "1D は未対応"
        );
    }

    #[test]
    fn 中身が変わると別の素材になる() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.cube"), dir.path().join("b.cube"));
        std::fs::write(&a, INVERT).unwrap();
        std::fs::write(&b, INVERT.replace("TITLE \"invert\"", "TITLE \"other\"")).unwrap();
        let tools = crate::Tools::default();
        let opts = crate::IngestOptions {
            cache: crate::CacheLayout::new(dir.path().join("cache")),
            tools,
            encoder: crate::PreviewEncoder::Auto,
        };
        let ra = crate::ingest(&opts, &a).unwrap();
        let rb = crate::ingest(&opts, &b).unwrap();
        assert_ne!(ra.asset.id, rb.asset.id);
        assert_eq!(ra.asset.content_hash.len(), 64);
        assert!(ra.preview.is_none() && ra.thumbnail.is_none() && ra.waveform.is_none());
        let again = crate::ingest(&opts, &a).unwrap();
        assert_eq!(ra.asset.id, again.asset.id, "同じ中身は同じ素材");
        assert!(!dir.path().join("cache").exists(), "キャッシュは作らない");
    }
}
