//! 素材の中身の調べ方。ffprobe の JSON から、素材の長さ・映像・音の情報を取る。

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use mm_types::{ColorSpace, MmError, Rational, RationalTime, Result};

use crate::tools::{run, Tools};

/// 秒の小数を有理数にするときの分母（マイクロ秒）。
const MICROS: u32 = 1_000_000;

/// 映像ストリームの情報。
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    /// 固定フレームレートなら fps。VFR は `None`。
    pub fps: Option<Rational>,
    /// 最初の PTS。
    pub start: RationalTime,
    pub color_space: ColorSpace,
}

/// 音ストリームの情報。
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    pub channels: u16,
    /// 最初の PTS。
    pub start: RationalTime,
}

/// 素材の調べた結果。
#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbe {
    pub duration: RationalTime,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
}

impl MediaProbe {
    /// 映像の最初の PTS − 音の最初の PTS。どちらかが無ければ 0。
    pub fn timebase_offset(&self) -> Result<RationalTime> {
        match (&self.video, &self.audio) {
            (Some(v), Some(a)) => v.start.checked_sub(a.start),
            _ => Ok(RationalTime::ZERO),
        }
    }
}

/// ffprobe で素材を調べる。映像も音も無ければ取り込めない素材として `Unsupported`。
pub fn probe(tools: &Tools, path: &Path) -> Result<MediaProbe> {
    let mut cmd = Command::new(&tools.ffprobe);
    cmd.args([
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
    ])
    .arg(path);
    let out = run(&mut cmd, &format!("ffprobe {}", path.display()))?;
    let json: Value = serde_json::from_slice(&out.stdout)?;
    parse_probe(&json, path)
}

/// ffprobe の JSON を読む。
pub fn parse_probe(json: &Value, path: &Path) -> Result<MediaProbe> {
    let streams = json["streams"].as_array().map_or(&[][..], Vec::as_slice);
    let first = |kind: &str| streams.iter().find(|s| s["codec_type"] == kind);
    // 動画のサムネイル（attached_pic）は映像ストリームとして出るので除く。
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .map(video_info)
        .transpose()?;
    let audio = first("audio").map(audio_info).transpose()?;
    if video.is_none() && audio.is_none() {
        return Err(MmError::Unsupported(format!(
            "{}: 映像も音も無い",
            path.display()
        )));
    }
    let duration = duration_of(json, streams)
        .ok_or_else(|| MmError::Corrupt(format!("{}: 長さが分からない", path.display())))??;
    Ok(MediaProbe {
        duration,
        video,
        audio,
    })
}

fn duration_of(json: &Value, streams: &[Value]) -> Option<Result<RationalTime>> {
    let from_format = json["format"]["duration"].as_str();
    let from_streams = streams.iter().filter_map(|s| s["duration"].as_str()).next();
    from_format.or(from_streams).map(parse_seconds)
}

fn video_info(s: &Value) -> Result<VideoInfo> {
    let dim = |k: &str| s[k].as_u64().and_then(|v| u32::try_from(v).ok());
    let (Some(width), Some(height)) = (dim("width"), dim("height")) else {
        return Err(MmError::Corrupt("映像の大きさが分からない".into()));
    };
    Ok(VideoInfo {
        width,
        height,
        fps: constant_fps(s),
        start: start_of(s)?,
        color_space: color_space_of(s),
    })
}

fn audio_info(s: &Value) -> Result<AudioInfo> {
    let channels = s["channels"]
        .as_u64()
        .and_then(|c| u16::try_from(c).ok())
        .filter(|c| *c > 0)
        .ok_or_else(|| MmError::Corrupt("音のチャンネル数が分からない".into()))?;
    Ok(AudioInfo {
        channels,
        start: start_of(s)?,
    })
}

fn start_of(s: &Value) -> Result<RationalTime> {
    s["start_time"]
        .as_str()
        .map_or(Ok(RationalTime::ZERO), parse_seconds)
}

/// `r_frame_rate`（最小公倍のレート）と `avg_frame_rate` が同じなら固定フレームレート。
fn constant_fps(s: &Value) -> Option<Rational> {
    let r = parse_rate(s["r_frame_rate"].as_str()?)?;
    let avg = parse_rate(s["avg_frame_rate"].as_str()?)?;
    (r == avg).then_some(r)
}

fn parse_rate(text: &str) -> Option<Rational> {
    let (n, d) = text.split_once('/')?;
    let (n, d) = (n.parse::<u32>().ok()?, d.parse::<u32>().ok()?);
    if n == 0 || d == 0 {
        return None;
    }
    Rational::new(n, d).ok()
}

/// 原色で決める。PQ と HLG の伝達特性は色空間の名前に出さず、デコーダー側が拒否する（RFC 0007）。
fn color_space_of(s: &Value) -> ColorSpace {
    match s["color_primaries"].as_str() {
        Some("bt2020") => ColorSpace::bt2020(),
        Some("smpte170m" | "bt470bg" | "smpte240m") => ColorSpace::bt601(),
        _ => ColorSpace::bt709(),
    }
}

/// `"12.345678"` のような十進の秒をマイクロ秒の精度の有理数にする。
pub fn parse_seconds(text: &str) -> Result<RationalTime> {
    let bad = || MmError::Corrupt(format!("秒として読めない: {text}"));
    let (neg, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let micros_digits: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
    let whole: i128 = int.parse().map_err(|_| bad())?;
    let micros: i128 = micros_digits.parse().map_err(|_| bad())?;
    let num = whole * i128::from(MICROS) + micros;
    RationalTime::new(if neg { -num } else { num }, MICROS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 十進の秒を有理数にする() {
        assert_eq!(
            parse_seconds("1.5").unwrap(),
            RationalTime::new(3, 2).unwrap()
        );
        assert_eq!(
            parse_seconds("-0.012").unwrap(),
            RationalTime::new(-3, 250).unwrap()
        );
        assert_eq!(parse_seconds("60").unwrap(), RationalTime::seconds(60));
        assert!(parse_seconds("abc").is_err());
    }

    fn sample() -> Value {
        json!({
            "format": {"duration": "2.000000"},
            "streams": [
                {"codec_type": "video", "width": 320, "height": 180,
                 "r_frame_rate": "30/1", "avg_frame_rate": "30/1", "start_time": "0.500000",
                 "color_primaries": "bt709"},
                {"codec_type": "audio", "channels": 2, "start_time": "0.000000"}
            ]
        })
    }

    #[test]
    fn 映像と音の情報を取る() {
        let p = parse_probe(&sample(), Path::new("a.mp4")).unwrap();
        let v = p.video.as_ref().unwrap();
        assert_eq!((v.width, v.height), (320, 180));
        assert_eq!(v.fps, Some(Rational::integer(30)));
        assert_eq!(p.audio.as_ref().unwrap().channels, 2);
        assert_eq!(p.duration, RationalTime::seconds(2));
        assert_eq!(
            p.timebase_offset().unwrap(),
            RationalTime::new(1, 2).unwrap()
        );
    }

    #[test]
    fn 平均と最小公倍のレートが違えば_vfr() {
        let mut j = sample();
        j["streams"][0]["avg_frame_rate"] = json!("2997/100");
        let p = parse_probe(&j, Path::new("a.mp4")).unwrap();
        assert_eq!(p.video.unwrap().fps, None);
    }

    #[test]
    fn 映像も音も無い素材は拒否する() {
        let j = json!({"format": {"duration": "1.0"}, "streams": []});
        assert!(matches!(
            parse_probe(&j, Path::new("a.bin")),
            Err(MmError::Unsupported(_))
        ));
    }
}
