//! チャプター。コンポジションのマーカーを MP4 のチャプターと、YouTube の説明欄に貼る文字列にする。
//!
//! YouTube の決まり: 説明欄の最初のチャプターは `0:00`、3 つ以上、各チャプターは 10 秒以上、時刻は昇順
//! （YouTube ヘルプ「動画にチャプターを追加する」）。マーカーの長さ（`duration`）は使わず、点として扱う。

use mm_decode_api::ChapterMark;
use mm_doc::Composition;
use mm_types::{MmError, RationalTime, Result, TimeRange};

/// YouTube のチャプターの最少の個数。
pub const YOUTUBE_MIN_CHAPTERS: usize = 3;
/// YouTube のチャプターの最短の長さ（秒）。
pub const YOUTUBE_MIN_SECONDS: i64 = 10;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;

/// `range` に入るマーカーを、区間の始まりを 0 にしたチャプターにする。終わりは次のチャプターの始まりで、
/// 最後は区間の終わり。
pub fn chapter_marks(comp: &Composition, range: TimeRange) -> Result<Vec<ChapterMark>> {
    let inside: Vec<_> = comp
        .markers
        .iter()
        .filter(|m| range.contains(m.time))
        .collect();
    let mut out = Vec::with_capacity(inside.len());
    for (i, m) in inside.iter().enumerate() {
        let end = inside.get(i + 1).map_or(range.end, |n| n.time);
        out.push(ChapterMark {
            start: m.time.checked_sub(range.start)?,
            end: end.checked_sub(range.start)?,
            title: m.name.clone(),
        });
    }
    Ok(out)
}

/// 経過の秒（切り捨て）。
fn whole_seconds(t: RationalTime) -> i64 {
    t.to_f64().floor() as i64
}

/// YouTube の説明欄に貼る「00:00 タイトル」の行。1 時間以上の動画は全行を `H:MM:SS` にする。
/// 決まりを満たさなければ `InvalidArgument`（理由を書く）。
pub fn youtube_description(chapters: &[ChapterMark]) -> Result<String> {
    if chapters.len() < YOUTUBE_MIN_CHAPTERS {
        return Err(invalid(format!(
            "YouTube のチャプターは {YOUTUBE_MIN_CHAPTERS} つ以上が要る（今は {} つ）",
            chapters.len()
        )));
    }
    if chapters[0].start != RationalTime::ZERO {
        return Err(invalid(
            "YouTube の最初のチャプターは 00:00 から始める（先頭にマーカーを置く）".into(),
        ));
    }
    let min = RationalTime::seconds(YOUTUBE_MIN_SECONDS);
    for c in chapters {
        if c.end.checked_sub(c.start)? < min {
            return Err(invalid(format!(
                "チャプター「{}」が {YOUTUBE_MIN_SECONDS} 秒より短い（YouTube は 10 秒以上）",
                c.title
            )));
        }
    }
    let hours = chapters
        .last()
        .is_some_and(|c| whole_seconds(c.end) >= SECONDS_PER_HOUR);
    let lines: Vec<String> = chapters
        .iter()
        .map(|c| {
            let title = c.title.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("{} {title}", clock(whole_seconds(c.start), hours))
        })
        .collect();
    Ok(lines.join("\n") + "\n")
}

fn clock(secs: i64, hours: bool) -> String {
    let (m, s) = (
        secs % SECONDS_PER_HOUR / SECONDS_PER_MINUTE,
        secs % SECONDS_PER_MINUTE,
    );
    if hours {
        format!("{}:{m:02}:{s:02}", secs / SECONDS_PER_HOUR)
    } else {
        format!("{:02}:{s:02}", secs / SECONDS_PER_MINUTE)
    }
}

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::Marker;
    use mm_types::{Id, Rational};

    fn comp(markers: &[(i64, &str)]) -> Composition {
        Composition::new(
            Id::new("c"),
            16,
            9,
            Rational::integer(30),
            RationalTime::seconds(100),
        )
        .with_markers(
            markers
                .iter()
                .enumerate()
                .map(|(i, (s, name))| Marker {
                    id: Id::new(format!("m{i}")),
                    time: RationalTime::seconds(*s),
                    duration: RationalTime::ZERO,
                    name: (*name).into(),
                })
                .collect(),
        )
    }

    fn whole(end: i64) -> TimeRange {
        TimeRange::new(RationalTime::ZERO, RationalTime::seconds(end)).unwrap()
    }

    #[test]
    fn マーカーは次のマーカーまでのチャプターになり_最後は区間の終わりまで() {
        let marks = chapter_marks(
            &comp(&[(0, "導入"), (20, "本題"), (65, "まとめ")]),
            whole(100),
        )
        .unwrap();
        assert_eq!(marks.len(), 3);
        assert_eq!(marks[0].end, RationalTime::seconds(20));
        assert_eq!(marks[2].end, RationalTime::seconds(100));
    }

    #[test]
    fn 区間を指すと_区間の外のマーカーを除き_始まりを_0_にする() {
        let range = TimeRange::new(RationalTime::seconds(10), RationalTime::seconds(70)).unwrap();
        let marks =
            chapter_marks(&comp(&[(0, "a"), (20, "b"), (65, "c"), (80, "d")]), range).unwrap();
        let starts: Vec<_> = marks.iter().map(|m| m.start).collect();
        assert_eq!(
            starts,
            [RationalTime::seconds(10), RationalTime::seconds(55)]
        );
        assert_eq!(marks[1].end, RationalTime::seconds(60));
    }

    #[test]
    fn 説明欄の文字列は_00_00_から始まる行になる() {
        let marks = chapter_marks(
            &comp(&[(0, "導入"), (20, "本題  の\n話"), (65, "まとめ")]),
            whole(100),
        )
        .unwrap();
        assert_eq!(
            youtube_description(&marks).unwrap(),
            "00:00 導入\n00:20 本題 の 話\n01:05 まとめ\n"
        );
    }

    #[test]
    fn 一時間を超えると全行が_h_mm_ss() {
        let marks = chapter_marks(&comp(&[(0, "a"), (30, "b"), (3700, "c")]), whole(4000)).unwrap();
        assert_eq!(
            youtube_description(&marks).unwrap(),
            "0:00:00 a\n0:00:30 b\n1:01:40 c\n"
        );
    }

    #[test]
    fn youtube_の決まりを破ると理由つきで拒否する() {
        let few = chapter_marks(&comp(&[(0, "a"), (30, "b")]), whole(100)).unwrap();
        assert!(youtube_description(&few)
            .unwrap_err()
            .to_string()
            .contains("3 つ以上"));
        let late = chapter_marks(&comp(&[(5, "a"), (30, "b"), (60, "c")]), whole(100)).unwrap();
        assert!(youtube_description(&late)
            .unwrap_err()
            .to_string()
            .contains("00:00"));
        let short = chapter_marks(&comp(&[(0, "a"), (5, "b"), (60, "c")]), whole(100)).unwrap();
        assert!(youtube_description(&short)
            .unwrap_err()
            .to_string()
            .contains("10 秒"));
    }
}
