//! 癖のある素材（edit list、B フレーム入りの頭のずれと VFR、44.1 kHz・モノラル・5.1 の音）と、
//! 飛ばしながらのシーク、区間をまたぐ音の読み出し。
#![cfg(feature = "ffmpeg")]

mod common;

use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory};
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_types::{RationalTime, TimeRange};

fn open(name: &str) -> Box<dyn Decoder> {
    let path = common::fixtures().join(name);
    FfmpegDecoderFactory.open(&path, None).expect("開ける")
}

fn number(frame: &DecodedFrame) -> i64 {
    common::read_frame_number(frame.image.as_ref().expect("CPU の絵"))
}

fn drain(dec: &mut dyn Decoder) -> Vec<DecodedFrame> {
    let mut out = Vec::new();
    while let Some(f) = dec.next_frame().expect("デコード") {
        out.push(f);
    }
    out
}

fn tenths(a: i128, b: i128) -> TimeRange {
    TimeRange::new(
        RationalTime::new(a, 10).unwrap(),
        RationalTime::new(b, 10).unwrap(),
    )
    .unwrap()
}

/// 時刻 `t` の 1 フレーム目の焼いた番号が `want` になる。
fn assert_seek(dec: &mut dyn Decoder, t: RationalTime, want: i64) {
    dec.seek(t).unwrap();
    let f = dec.next_frame().unwrap().expect("フレームがある");
    assert_eq!(number(&f), want, "{t:?} の番号");
}

#[test]
fn edit_list_の外のパケットは索引に入らずフレーム数が出るフレームと一致する() {
    let mut dec = open("editlist.mp4");
    let info = dec.info().clone();
    let frames = drain(dec.as_mut());
    assert_eq!(frames.len() as i64, info.frame_count());
    // 0.5 秒（フレーム 15）で切った素材。出るフレームは 45 で、先頭は元のフレーム 15
    assert_eq!(frames.len(), 45);
    let first = number(&frames[0]);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f.index, i as i64);
        assert_eq!(number(f), first + i as i64, "フレーム {i}");
    }
    let fps = info.fps.expect("CFR");
    for i in 0..5 {
        assert_seek(
            dec.as_mut(),
            RationalTime::from_frames(i, fps).unwrap(),
            first + i,
        );
    }
}

#[test]
fn b_フレームと頭のずれの組み合わせでも番号が合う() {
    let mut dec = open("offset_bf.mp4");
    let fps = dec.info().fps.expect("CFR");
    assert_eq!(dec.info().frame_count(), 30);
    let frames = drain(dec.as_mut());
    assert_eq!(frames.len(), 30);
    assert_eq!(frames[0].pts, RationalTime::ZERO);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(number(f), i as i64);
    }
    for target in [20, 3, 10, 9, 29, 0, 11] {
        assert_seek(
            dec.as_mut(),
            RationalTime::from_frames(target, fps).unwrap(),
            target,
        );
    }
}

#[test]
fn b_フレーム入りの_vfr_でも番号と時刻が合う() {
    let mut dec = open("vfr_bf.mp4");
    let info = dec.info().clone();
    assert_eq!(info.fps, None);
    assert_eq!(info.pts_index.len(), 60);
    let frames = drain(dec.as_mut());
    assert_eq!(frames.len(), 60);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(number(f), i as i64);
        assert_eq!(f.pts, info.pts_index[i]);
    }
    for target in [41usize, 3, 4, 59, 0, 17, 18, 30] {
        assert_seek(dec.as_mut(), info.pts_index[target], target as i64);
    }
}

#[test]
fn 飛ばしながら読んでも番号が合う() {
    // 30 fps の素材を 15 fps に置く（1 フレームおき）、速度 2 の再生、同じ GOP 内の小さな前進、後戻りを混ぜる
    for name in ["cfr.mp4", "vfr_bf.mp4"] {
        let mut dec = open(name);
        let info = dec.info().clone();
        let at = |i: i64| match info.fps {
            Some(fps) => RationalTime::from_frames(i, fps).unwrap(),
            None => info.pts_index[i as usize],
        };
        for target in (0..60)
            .step_by(2)
            .chain([58, 3, 4, 7, 30, 31, 45, 12, 13, 59, 0])
        {
            assert_seek(dec.as_mut(), at(target), target);
        }
        for target in (0..60).step_by(5) {
            assert_seek(dec.as_mut(), at(target), target);
        }
    }
}

#[test]
fn 音は素材のレートとチャンネル数によらず_48khz_ステレオで返る() {
    for name in ["mono44.mp4", "surround.mp4"] {
        let mut dec = open(name);
        let audio = dec.info().audio.clone().expect("音がある");
        assert_eq!((audio.sample_rate, audio.channels), (48_000, 2), "{name}");
        // 0.2〜0.3 秒。440 Hz なら 0.1 秒に約 44 周期
        let block = dec.decode_audio(tenths(2, 3)).unwrap();
        assert_eq!((block.sample_rate, block.channels), (48_000, 2), "{name}");
        assert_eq!(block.frames(), 4_800, "{name}");
        assert!(!block.is_silent(), "{name}");
        let left: Vec<f32> = block.samples.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        assert!(
            (42..=46).contains(&crossings),
            "{name} の周期の数 {crossings}"
        );
        // 素材の外は無音で、長さは区間どおり
        assert!(dec.decode_audio(tenths(30, 31)).unwrap().is_silent());
    }
}

#[test]
fn 区間を続けて読んでも一度に読んだ音と一致する() {
    for name in ["cfr.mp4", "mono44.mp4"] {
        let mut whole = open(name);
        let expect = whole.decode_audio(tenths(0, 9)).unwrap().samples;
        let mut parts = open(name);
        let mut got = Vec::new();
        for i in 0..9 {
            got.extend(parts.decode_audio(tenths(i, i + 1)).unwrap().samples);
        }
        assert_eq!(got.len(), expect.len(), "{name}");
        let worst = got
            .iter()
            .zip(&expect)
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(worst < 1e-6, "{name} の最大の差 {worst}");
    }
}

#[test]
fn 区間が飛んでも後戻りしても同じ音が出る() {
    for name in ["cfr.mp4", "mono44.mp4"] {
        let mut dec = open(name);
        let fresh = |range: TimeRange| open(name).decode_audio(range).unwrap().samples;
        // 続き、少し先、後戻り、遠い先の順
        for range in [
            tenths(2, 3),
            tenths(3, 4),
            tenths(5, 6),
            tenths(1, 2),
            tenths(8, 9),
        ] {
            let got = dec.decode_audio(range).unwrap().samples;
            let want = fresh(range);
            let worst = got
                .iter()
                .zip(&want)
                .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(worst < 5e-3, "{name} {range:?} の最大の差 {worst}");
        }
    }
}

#[test]
fn 素材の終わりを越えて続きを読んでも無音になる() {
    let mut dec = open("mono44.mp4");
    let total = dec.info().duration;
    let tail = TimeRange::new(
        total
            .checked_sub(RationalTime::new(1, 10).unwrap())
            .unwrap(),
        total
            .checked_add(RationalTime::new(1, 10).unwrap())
            .unwrap(),
    )
    .unwrap();
    let block = dec.decode_audio(tail).unwrap();
    assert_eq!(block.frames(), 9_600);
    let more = TimeRange::new(
        tail.end,
        tail.end
            .checked_add(RationalTime::new(1, 10).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(dec.decode_audio(more).unwrap().is_silent());
}
