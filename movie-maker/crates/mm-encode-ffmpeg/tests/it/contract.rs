//! `mm-testkit` のエンコーダーの契約を通す。

use std::sync::Arc;

use mm_encode_ffmpeg::FfmpegEncoderFactory;
use mm_testkit::decode::encoder_contract;

#[test]
fn ffmpeg_エンコーダーが契約を通る() {
    let dir = std::env::temp_dir().join("mm_encode_ffmpeg_contract");
    std::fs::create_dir_all(&dir).unwrap();
    encoder_contract(|| Arc::new(FfmpegEncoderFactory), "h264", &dir);
}
