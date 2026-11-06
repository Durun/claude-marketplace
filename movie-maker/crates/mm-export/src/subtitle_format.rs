//! 字幕のファイルの文字列。SRT（`00:00:01,000 --> ...`）と WebVTT（`00:00:01.000 --> ...`）。

use mm_decode_api::SubtitleCue;
use mm_types::RationalTime;

const MS_PER_SECOND: i64 = 1000;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;

/// 時刻を `HH:MM:SS<sep>mmm` にする。ms に丸める。
fn timestamp(t: RationalTime, sep: char) -> String {
    let total = (t.to_f64() * 1000.0).round().max(0.0) as i64;
    let (secs, ms) = (total / MS_PER_SECOND, total % MS_PER_SECOND);
    format!(
        "{:02}:{:02}:{:02}{sep}{ms:03}",
        secs / SECONDS_PER_HOUR,
        secs % SECONDS_PER_HOUR / SECONDS_PER_MINUTE,
        secs % SECONDS_PER_MINUTE
    )
}

/// SRT。番号は 1 から。
pub fn srt(cues: &[SubtitleCue]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            timestamp(c.start, ','),
            timestamp(c.end, ','),
            c.text
        ));
    }
    out
}

/// WebVTT。`&` と `<` は文字の参照にする（タグとして読まれないように）。
pub fn vtt(cues: &[SubtitleCue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for c in cues {
        let text = c.text.replace('&', "&amp;").replace('<', "&lt;");
        out.push_str(&format!(
            "{} --> {}\n{text}\n\n",
            timestamp(c.start, '.'),
            timestamp(c.end, '.')
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(s: (i128, u32), e: (i128, u32), text: &str) -> SubtitleCue {
        SubtitleCue {
            start: RationalTime::new(s.0, s.1).unwrap(),
            end: RationalTime::new(e.0, e.1).unwrap(),
            text: text.into(),
        }
    }

    #[test]
    fn srt_は番号と_カンマの区切りの時刻を持つ() {
        let text = srt(&[
            cue((1, 1), (5, 2), "一行目\n二行目"),
            cue((3661, 1), (3662, 1), "b"),
        ]);
        assert_eq!(
            text,
            "1\n00:00:01,000 --> 00:00:02,500\n一行目\n二行目\n\n2\n01:01:01,000 --> 01:01:02,000\nb\n\n"
        );
    }

    #[test]
    fn vtt_はヘッダーと_ドットの区切りの時刻を持ち_タグになる記号を避ける() {
        let text = vtt(&[cue((0, 1), (1, 1), "a<b & c")]);
        assert_eq!(
            text,
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\na&lt;b &amp; c\n\n"
        );
    }
}
