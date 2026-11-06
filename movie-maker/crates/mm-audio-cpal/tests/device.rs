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

    out.run();
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_millis(600));
    let elapsed = t0.elapsed().as_secs_f64();
    let played = out.position_secs() - 5.0;
    assert!(
        (played - elapsed).abs() < 0.25,
        "鳴った長さ {played:.3} 秒は経過 {elapsed:.3} 秒に近い"
    );

    let held = out.halt();
    std::thread::sleep(Duration::from_millis(100));
    let frozen = out.position_secs();
    assert_eq!(frozen, held, "止めた位置は後続のコールバックで跳ばない");
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        (out.position_secs() - frozen).abs() < 1e-9,
        "止めたら進まない"
    );
}

#[test]
fn 足した音を鳴らし切ると時計は足した長さに届く() {
    let Some(out) = open() else { return };
    const CHUNK: usize = 4_800;
    const CHUNKS: usize = 20;
    out.reset(0.0);
    for _ in 0..CHUNKS {
        let block = AudioBlock::silence(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS, CHUNK);
        out.push(&block).unwrap();
    }
    out.run();
    let total = (CHUNK * CHUNKS) as f64 / f64::from(DEFAULT_SAMPLE_RATE);
    let deadline = Instant::now() + Duration::from_secs_f64(total + 2.0);
    while out.position_secs() < total - 0.001 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let end = out.position_secs();
    assert!(end >= total - 0.001, "位置 {end:.4} が長さ {total} に届く");
}

#[test]
fn 形の違うブロックは受けない() {
    let Some(out) = open() else { return };
    let bad = AudioBlock::silence(44_100, 1, 10);
    assert!(matches!(out.push(&bad), Err(MmError::InvalidArgument(_))));
}
