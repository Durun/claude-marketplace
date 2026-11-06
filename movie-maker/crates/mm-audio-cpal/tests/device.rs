//! 実機の出力デバイスを開く試験。デバイスが無い環境（CI）では何もせず通る。鳴らすのは無音だけ。

use std::time::{Duration, Instant};

use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_audio_cpal::AudioOutput;
use mm_types::MmError;

fn open() -> Option<AudioOutput> {
    match AudioOutput::open(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS) {
        Ok(out) => Some(out),
        Err(MmError::Unsupported(why)) => {
            eprintln!("出力デバイスが無いので飛ばす: {why}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn 時計は実時間で進み_止めると止まる() {
    let Some(out) = open() else { return };
    out.reset(5.0);
    out.push(&AudioBlock::silence(
        DEFAULT_SAMPLE_RATE,
        DEFAULT_CHANNELS,
        DEFAULT_SAMPLE_RATE as usize * 2,
    ))
    .unwrap();
    assert_eq!(out.position_secs(), 5.0, "鳴らす前は起点");

    out.set_running(true);
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_millis(600));
    let elapsed = t0.elapsed().as_secs_f64();
    let played = out.position_secs() - 5.0;
    assert!(
        (played - elapsed).abs() < 0.25,
        "鳴った長さ {played:.3} 秒は経過 {elapsed:.3} 秒に近い"
    );

    out.set_running(false);
    std::thread::sleep(Duration::from_millis(100));
    let frozen = out.position_secs();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        (out.position_secs() - frozen).abs() < 1e-9,
        "止めたら進まない"
    );
}

#[test]
fn 形の違うブロックは受けない() {
    let Some(out) = open() else { return };
    let bad = AudioBlock::silence(44_100, 1, 10);
    assert!(matches!(out.push(&bad), Err(MmError::InvalidArgument(_))));
}
