//! 実素材（CFR、NTSC の分数、VFR、先頭が 0 でない素材、音）のフレームの番号と時刻。

use crate::common;

use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory};
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_types::{MmError, Rational, RationalTime, TimeRange};

fn open(name: &str) -> Box<dyn Decoder> {
    let path = common::fixtures().join(name);
    FfmpegDecoderFactory.open(&path, None).expect("開ける")
}

fn number(frame: &DecodedFrame) -> i64 {
    common::read_frame_number(frame.image.as_ref().expect("CPU の画像"))
}

fn drain(dec: &mut dyn Decoder) -> Vec<DecodedFrame> {
    let mut out = Vec::new();
    while let Some(f) = dec.next_frame().expect("デコード") {
        out.push(f);
    }
    out
}

#[test]
fn cfr_の全フレームで焼き込んだ番号と番号と時刻が一致する() {
    let mut dec = open("cfr.mp4");
    let fps = Rational::integer(30);
    assert_eq!(dec.info().fps, Some(fps));
    assert_eq!(dec.info().frame_count(), 60);
    assert!(dec.info().pts_index.is_empty());
    for (i, f) in drain(dec.as_mut()).iter().enumerate() {
        assert_eq!(f.index, i as i64);
        assert_eq!(number(f), i as i64, "フレーム {i} の焼き込んだ番号");
        assert_eq!(f.pts, RationalTime::from_frames(i as i64, fps).unwrap());
    }
}

#[test]
fn シーク後も_in_点から正しい番号が出る() {
    let mut dec = open("cfr.mp4");
    let fps = Rational::integer(30);
    // キーフレーム間隔 12 の前後、先頭、末尾、後戻りを混ぜる
    for target in [59, 0, 30, 31, 13, 12, 11, 45, 44, 1, 58, 24, 23, 25] {
        dec.seek(RationalTime::from_frames(target, fps).unwrap())
            .unwrap();
        let f = dec.next_frame().unwrap().expect("フレームがある");
        assert_eq!(f.index, target);
        assert_eq!(number(&f), target, "シーク先 {target} の焼き込んだ番号");
        if target < 59 {
            let g = dec.next_frame().unwrap().expect("続きがある");
            assert_eq!(number(&g), target + 1, "シーク先の次");
        }
    }
}

#[test]
fn 区間のデコードは_in_点の番号から並ぶ() {
    let mut dec = open("cfr.mp4");
    let fps = Rational::integer(30);
    let range = TimeRange::new(
        RationalTime::from_frames(20, fps).unwrap(),
        RationalTime::from_frames(27, fps).unwrap(),
    )
    .unwrap();
    let frames = dec.decode_range(range).unwrap();
    let numbers: Vec<i64> = frames.iter().map(number).collect();
    assert_eq!(numbers, (20..27).collect::<Vec<_>>());
}

#[test]
fn 範囲の外へのシークは終わりを返す() {
    let mut dec = open("cfr.mp4");
    dec.seek(RationalTime::seconds(10)).unwrap();
    assert!(dec.next_frame().unwrap().is_none());
    dec.seek(RationalTime::ZERO).unwrap();
    assert_eq!(number(&dec.next_frame().unwrap().unwrap()), 0);
}

#[test]
fn 分数の_fps_でも番号がずれない() {
    let mut dec = open("ntsc.mp4");
    let fps = Rational::new(30_000, 1001).unwrap();
    assert_eq!(dec.info().fps, Some(fps));
    assert_eq!(dec.info().frame_count(), 30);
    let frames = drain(dec.as_mut());
    assert_eq!(frames.len(), 30);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(number(f), i as i64);
        assert_eq!(f.pts, RationalTime::from_frames(i as i64, fps).unwrap());
    }
    let t = RationalTime::from_frames(17, fps).unwrap();
    dec.seek(t).unwrap();
    assert_eq!(number(&dec.next_frame().unwrap().unwrap()), 17);
}

#[test]
fn vfr_は_pts_の索引を持ち番号がずれない() {
    let mut dec = open("vfr.mp4");
    let info = dec.info().clone();
    assert_eq!(info.fps, None);
    assert_eq!(info.pts_index.len(), 60);
    assert_eq!(info.frame_count(), 60);
    assert!(info.pts_index.windows(2).all(|w| w[0] < w[1]), "昇順");
    assert!(info.duration > *info.pts_index.last().unwrap());

    let frames = drain(dec.as_mut());
    assert_eq!(frames.len(), 60);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(number(f), i as i64);
        assert_eq!(f.pts, info.pts_index[i]);
    }
    // 時刻で引く: フレームの時刻ちょうどと、次のフレームの直前
    for target in [41usize, 3, 59, 0, 17] {
        let t = info.pts_index[target];
        assert_eq!(info.frame_index_at(t), target as i64);
        dec.seek(t).unwrap();
        assert_eq!(number(&dec.next_frame().unwrap().unwrap()), target as i64);
        if target < 59 {
            let just_before_next = info.pts_index[target + 1]
                .checked_sub(RationalTime::new(1, 1000).unwrap())
                .unwrap();
            dec.seek(just_before_next).unwrap();
            assert_eq!(number(&dec.next_frame().unwrap().unwrap()), target as i64);
        }
    }
}

#[test]
fn 先頭が_0_でない素材も最初のフレームが_t_0_になる() {
    let mut dec = open("offset.mp4");
    let fps = dec.info().fps.expect("CFR");
    assert_eq!(fps, Rational::integer(30));
    let frames = drain(dec.as_mut());
    assert_eq!(frames.len(), 30);
    assert_eq!(frames[0].pts, RationalTime::ZERO);
    assert_eq!(number(&frames[0]), 0);
    assert_eq!(number(&frames[29]), 29);
}

#[test]
fn 壊れた素材と無い素材はクラッシュせずにエラーを返す() {
    let dir = common::fixtures();
    assert!(matches!(
        FfmpegDecoderFactory.open(&dir.join("broken.mp4"), None),
        Err(MmError::Corrupt(_))
    ));
    assert!(matches!(
        FfmpegDecoderFactory.open(&dir.join("no_such_file.mp4"), None),
        Err(MmError::NotFound(_))
    ));
    // 動画ではないファイル
    let garbage = dir.join("garbage.broken.bin");
    std::fs::write(&garbage, vec![0x5Au8; 4096]).unwrap();
    assert!(FfmpegDecoderFactory.open(&garbage, None).is_err());
}

fn zero_crossings(samples: &[f32], channels: usize) -> usize {
    let left: Vec<f32> = samples.iter().step_by(channels).copied().collect();
    left.windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count()
}

#[test]
fn 音のトーンが区間どおりに出る() {
    let mut dec = open("cfr.mp4");
    let audio = dec.info().audio.clone().expect("音がある");
    assert_eq!((audio.sample_rate, audio.channels), (48_000, 2));

    let range = |a: i64, b: i64| {
        TimeRange::new(
            RationalTime::new(a as i128, 10).unwrap(),
            RationalTime::new(b as i128, 10).unwrap(),
        )
        .unwrap()
    };
    // 0.5〜0.6 秒、4800 サンプル。440Hz なら 0.1 秒に約 44 周期。
    let block = dec.decode_audio(range(5, 6)).unwrap();
    assert_eq!(block.frames(), 4_800);
    assert!(!block.is_silent());
    let samples = &block.samples;
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((0.08..0.16).contains(&peak), "振幅 {peak}");
    let crossings = zero_crossings(samples, 2);
    assert!((42..=46).contains(&crossings), "周期の数 {crossings}");

    // 先頭の区間
    let head = dec.decode_audio(range(0, 1)).unwrap();
    assert_eq!(head.frames(), 4_800);
    assert!(!head.is_silent());

    // 素材の外は無音で、長さは区間どおり
    let beyond = dec.decode_audio(range(30, 31)).unwrap();
    assert_eq!(beyond.frames(), 4_800);
    assert!(beyond.is_silent());
}

#[test]
fn 音の無い素材は無音を返す() {
    let mut dec = open("vfr.mp4");
    assert!(dec.info().audio.is_none());
    let range = TimeRange::new(RationalTime::ZERO, RationalTime::new(1, 10).unwrap()).unwrap();
    let block = dec.decode_audio(range).unwrap();
    assert_eq!(block.frames(), 4_800);
    assert!(block.is_silent());
}
