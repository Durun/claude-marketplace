//! 書き出した結果の検証。ffprobe でフレーム数、映像と音の長さ、大きさを読み、期待と突き合わせる。
//!
//! フレーム数はデコードせずパケットの数（`-count_packets`）で数える。H.264 と AAC の MP4 では映像のパケットが
//! フレームと 1 対 1 で、1080p でも数十ミリ秒で済む。

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use mm_types::{MmError, Result, Size};

/// ffprobe の場所を指す環境変数（`mm-ingest` と同じ名前）。
const ENV_FFPROBE: &str = "MM_FFPROBE";
/// 映像の長さの許容。フレームの長さ 1 つ分に届かない揺れは MP4 の時間の丸めで起きる。
const VIDEO_TOLERANCE_FRAMES: f64 = 1.0;
/// 音の長さの許容（秒）。AAC のプライミング（先頭の無音）とフレームの区切りで 0.05 秒ほど動く。
const AUDIO_TOLERANCE_SECONDS: f64 = 0.1;

/// 出力に期待すること。
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub frames: u64,
    pub size: Size,
    pub fps: f64,
    /// 音を入れたなら、その長さ（秒）。
    pub audio_seconds: Option<f64>,
}

/// ffprobe が読んだ出力の中身。
#[derive(Debug, Clone, PartialEq)]
pub struct Verification {
    pub video_codec: String,
    pub size: Size,
    pub video_frames: u64,
    pub video_seconds: f64,
    pub audio_seconds: Option<f64>,
}

/// ffprobe で `path` を読み、`expected` と合わなければ `Corrupt`。
pub fn verify_output(path: &Path, expected: &Expected) -> Result<Verification> {
    let probed = probe(path)?;
    check(&probed, expected)?;
    Ok(probed)
}

fn probe(path: &Path) -> Result<Verification> {
    let exe = std::env::var_os(ENV_FFPROBE).unwrap_or_else(|| "ffprobe".into());
    let out = Command::new(&exe)
        .args(["-v", "error", "-count_packets", "-print_format", "json"])
        .args(["-show_streams", "-show_format"])
        .arg(path)
        .output()
        .map_err(|e| MmError::NotFound(format!("ffprobe を起動できない（{e}）")))?;
    if !out.status.success() {
        return Err(MmError::Corrupt(format!(
            "ffprobe が出力を読めない: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

/// ffprobe の JSON から検証に要る値を取る。
pub(crate) fn parse(json: &str) -> Result<Verification> {
    let root: Value = serde_json::from_str(json)?;
    let streams = root["streams"].as_array().map_or(&[][..], Vec::as_slice);
    let stream_of = |kind: &str| streams.iter().find(|s| s["codec_type"] == kind);
    let video = stream_of("video").ok_or_else(|| MmError::Corrupt("出力に映像が無い".into()))?;
    let format_seconds = number(&root["format"]["duration"]);
    let seconds = |s: &Value| number(&s["duration"]).or(format_seconds);
    Ok(Verification {
        video_codec: video["codec_name"].as_str().unwrap_or_default().to_owned(),
        size: Size::new(
            video["width"].as_u64().unwrap_or(0) as u32,
            video["height"].as_u64().unwrap_or(0) as u32,
        ),
        video_frames: number(&video["nb_read_packets"]).unwrap_or(0.0) as u64,
        video_seconds: seconds(video).unwrap_or(0.0),
        audio_seconds: stream_of("audio").and_then(seconds),
    })
}

/// ffprobe は数を文字列で返すことがある。
fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

fn check(got: &Verification, want: &Expected) -> Result<()> {
    let mut problems = Vec::new();
    if got.video_frames != want.frames {
        problems.push(format!(
            "フレーム数 {} （期待 {}）",
            got.video_frames, want.frames
        ));
    }
    if got.size != want.size {
        problems.push(format!(
            "大きさ {}x{} （期待 {}x{}）",
            got.size.width, got.size.height, want.size.width, want.size.height
        ));
    }
    let want_video = want.frames as f64 / want.fps;
    if (got.video_seconds - want_video).abs() > VIDEO_TOLERANCE_FRAMES / want.fps {
        problems.push(format!(
            "映像の長さ {:.3} 秒（期待 {want_video:.3}）",
            got.video_seconds
        ));
    }
    match (want.audio_seconds, got.audio_seconds) {
        (Some(w), Some(g)) if (g - w).abs() > AUDIO_TOLERANCE_SECONDS => {
            problems.push(format!("音の長さ {g:.3} 秒（期待 {w:.3}）"));
        }
        (Some(_), None) => problems.push("音のストリームが無い".into()),
        _ => {}
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(MmError::Corrupt(format!(
            "書き出しの検証に失敗: {}",
            problems.join("、")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_JSON: &str = r#"{
      "streams": [
        {"codec_type": "video", "codec_name": "h264", "width": 1280, "height": 720,
         "nb_read_packets": "60", "duration": "2.000000"},
        {"codec_type": "audio", "codec_name": "aac", "duration": "2.021000"}
      ],
      "format": {"duration": "2.021000"}
    }"#;

    fn expected() -> Expected {
        Expected {
            frames: 60,
            size: Size::new(1280, 720),
            fps: 30.0,
            audio_seconds: Some(2.0),
        }
    }

    #[test]
    fn ffprobe_の出力から値を読む() {
        let v = parse(PROBE_JSON).unwrap();
        assert_eq!(v.video_codec, "h264");
        assert_eq!(v.video_frames, 60);
        assert_eq!(v.size, Size::new(1280, 720));
        assert_eq!(v.audio_seconds, Some(2.021));
    }

    #[test]
    fn 期待と合えば通り_ずれは理由つきで落ちる() {
        let ok = parse(PROBE_JSON).unwrap();
        check(&ok, &expected()).unwrap();

        let short = Verification {
            video_frames: 59,
            audio_seconds: Some(1.5),
            ..ok.clone()
        };
        let err = check(&short, &expected()).unwrap_err().to_string();
        assert!(err.contains("フレーム数 59"), "{err}");
        assert!(err.contains("音の長さ"), "{err}");

        let silent = Verification {
            audio_seconds: None,
            ..ok
        };
        assert!(check(&silent, &expected()).is_err(), "音が無い");
    }

    #[test]
    fn 映像の無い出力は壊れている() {
        assert!(parse(r#"{"streams": [], "format": {}}"#).is_err());
    }
}
