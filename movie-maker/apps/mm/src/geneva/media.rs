//! 素材の大きさと長さを調べる。PNG はヘッダーを読み、ほかは `ffprobe` に聞く（`mm render` の検証と同じく
//! `MM_FFPROBE` か PATH の `ffprobe`）。FFmpeg を切ったビルドでも変換できるよう、ライブラリには依らない。

use std::io::Read;
use std::path::Path;
use std::process::Command;

use mm_types::{MmError, Rational, RationalTime, Result};
use serde_json::Value;

use super::syntax::decimal;

/// `ffprobe` の場所を上書きする環境変数。
const ENV_FFPROBE: &str = "MM_FFPROBE";
/// PNG の先頭（署名 8 バイト、IHDR の長さと名前 8 バイト、幅と高さ 8 バイト）。
const PNG_HEAD: usize = 24;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// 素材の性質。
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    /// 映像の大きさ。音だけなら `None`。
    pub size: Option<[u32; 2]>,
    pub duration: RationalTime,
    /// 映像の fps（`r_frame_rate`）。音だけと静止画は `None`。
    pub fps: Option<Rational>,
    /// 静止画か。
    pub still: bool,
}

/// 素材を調べる。拡張子が `.png` ならヘッダーだけ読む。
pub fn probe(path: &Path) -> Result<Media> {
    let png = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    if png {
        return png_size(path).map(|size| Media {
            size: Some(size),
            duration: RationalTime::ZERO,
            fps: None,
            still: true,
        });
    }
    ffprobe(path)
}

/// PNG の IHDR の幅と高さ。
fn png_size(path: &Path) -> Result<[u32; 2]> {
    let mut head = [0u8; PNG_HEAD];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| MmError::NotFound(format!("{}: {e}", path.display())))?;
    if &head[..PNG_SIGNATURE.len()] != PNG_SIGNATURE || &head[12..16] != b"IHDR" {
        return Err(MmError::Corrupt(format!("PNG でない: {}", path.display())));
    }
    let be = |i: usize| u32::from_be_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
    Ok([be(16), be(20)])
}

fn ffprobe(path: &Path) -> Result<Media> {
    let exe = std::env::var_os(ENV_FFPROBE).unwrap_or_else(|| "ffprobe".into());
    let out = Command::new(&exe)
        .args(["-v", "error", "-of", "json", "-show_entries"])
        .arg("format=duration:stream=codec_type,width,height,r_frame_rate")
        .arg(path)
        .output()
        .map_err(|e| MmError::NotFound(format!("ffprobe を起動できない: {e}")))?;
    if !out.status.success() {
        return Err(MmError::Corrupt(format!(
            "{}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let json: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| MmError::Corrupt(format!("ffprobe の出力: {e}")))?;
    parse_ffprobe(&json)
}

/// `ffprobe -of json` の出力を読む。
pub fn parse_ffprobe(json: &Value) -> Result<Media> {
    let duration = json
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .map(decimal)
        .transpose()?
        .unwrap_or(RationalTime::ZERO);
    let video = json.get("streams").and_then(Value::as_array).and_then(|s| {
        s.iter()
            .find(|s| s.get("codec_type").and_then(Value::as_str) == Some("video"))
    });
    let size = video.and_then(|v| {
        let dim = |k: &str| v.get(k).and_then(Value::as_u64).map(|n| n as u32);
        Some([dim("width")?, dim("height")?])
    });
    let fps = video
        .and_then(|v| v.get("r_frame_rate")?.as_str())
        .and_then(parse_rate);
    Ok(Media {
        size,
        duration,
        fps,
        still: false,
    })
}

fn parse_rate(s: &str) -> Option<Rational> {
    let (n, d) = s.split_once('/')?;
    Rational::new(n.parse().ok()?, d.parse().ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ffprobe_の映像と音だけを読み分ける() {
        let video = json!({
            "streams": [
                {"codec_type": "video", "width": 3840, "height": 2160, "r_frame_rate": "60/1"},
                {"codec_type": "audio", "r_frame_rate": "0/0"}
            ],
            "format": {"duration": "6981.674000"}
        });
        let m = parse_ffprobe(&video).unwrap();
        assert_eq!(m.size, Some([3840, 2160]));
        assert_eq!(m.fps, Some(Rational::integer(60)));
        assert_eq!(m.duration, RationalTime::new(6_981_674, 1000).unwrap());

        let audio = json!({
            "streams": [{"codec_type": "audio", "r_frame_rate": "0/0"}],
            "format": {"duration": "1.5"}
        });
        let m = parse_ffprobe(&audio).unwrap();
        assert_eq!(m.size, None);
        assert_eq!(m.fps, None);
    }

    #[test]
    fn png_のヘッダーから大きさを読む() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&640u32.to_be_bytes());
        bytes.extend_from_slice(&120u32.to_be_bytes());
        std::fs::write(&path, bytes).unwrap();
        let m = probe(&path).unwrap();
        assert_eq!(m.size, Some([640, 120]));
        assert!(m.still);
    }
}
