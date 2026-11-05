//! `mm-testkit` のデコーダーの契約を、本物の素材で通す。
#![cfg(feature = "ffmpeg")]

mod common;

use std::sync::Arc;

use mm_decode_api::DecoderFactory;
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_testkit::decode::{contract, DecodeFixture};
use mm_types::{Rational, RationalTime};

#[test]
fn ffmpeg_デコーダーが契約を通る() {
    let dir = common::fixtures();
    let fx = DecodeFixture {
        path: dir.join("cfr.mp4"),
        duration: RationalTime::seconds(2),
        fps: Rational::integer(30),
        broken_path: Some(dir.join("broken.mp4")),
    };
    contract(|| Arc::new(FfmpegDecoderFactory), &fx);
}

#[test]
fn 契約の素材でも焼いた番号がコマの番号と一致する() {
    let dir = common::fixtures();
    let mut dec = FfmpegDecoderFactory
        .open(&dir.join("cfr.mp4"), None)
        .unwrap();
    let frame = dec.next_frame().unwrap().unwrap();
    assert_eq!(common::read_frame_number(frame.image.as_ref().unwrap()), 0);
}
