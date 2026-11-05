//! cuvid のデコーダーを、`mm-testkit` の契約と実機の GPU で確かめる。
//! 素材は `fixtures/media/generate.sh`（無ければ作る）。NVIDIA の GPU とドライバーが要る。
#![cfg(feature = "cuvid")]

#[path = "../../mm-decode-d3d12va/tests/common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    busy_gpu, distinct_in_row, fixtures, gpu, read_frame_number, FrameReader, ReadbackFactory,
};
use mm_decode_api::{Decoder, DecoderFactory};
use mm_decode_cuvid::{gpu_request, CuvidDecoder, CuvidFactory};
use mm_gpu::GpuContext;
use mm_testkit::decode::{contract, DecodeFixture};
use mm_testkit::decode_vfr::vfr_contract;
use mm_types::{MmError, Rational, RationalTime};

const FILES: [&str; 6] = [
    "cfr.mp4",
    "ntsc.mp4",
    "vfr.mp4",
    "vfr_bf.mp4",
    "offset_bf.mp4",
    "p10.mp4",
];

fn ctx() -> Arc<GpuContext> {
    gpu(gpu_request(None).expect("D3D12 のアダプター"))
}

fn factory() -> Arc<dyn DecoderFactory> {
    Arc::new(ReadbackFactory::new(CuvidFactory, &ctx()))
}

fn open(name: &str) -> Box<dyn Decoder> {
    let path = fixtures(&FILES).join(name);
    factory().open(&path, None).expect("開ける")
}

fn drain_numbers(dec: &mut dyn Decoder) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    while let Some(f) = dec.next_frame().expect("復号") {
        out.push((
            f.index,
            read_frame_number(f.image.as_ref().expect("CPU の絵"), 1),
        ));
    }
    out
}

#[test]
fn デコーダーが契約を通る() {
    let dir = fixtures(&FILES);
    let fx = DecodeFixture {
        path: dir.join("cfr.mp4"),
        duration: RationalTime::seconds(2),
        fps: Rational::integer(30),
        broken_path: Some(dir.join("broken.mp4")),
    };
    contract(factory, &fx);
}

#[test]
fn vfr_の契約を通る() {
    vfr_contract(|| open("vfr.mp4"));
    vfr_contract(|| open("vfr_bf.mp4"));
}

#[test]
fn 焼いた番号がコマの番号と一致する() {
    for name in ["cfr.mp4", "ntsc.mp4", "vfr.mp4", "vfr_bf.mp4"] {
        let mut dec = open(name);
        let frames = drain_numbers(dec.as_mut());
        assert_eq!(frames.len() as i64, dec.info().frame_count(), "{name}");
        for (index, number) in frames {
            assert_eq!(index, number, "{name} のコマ {index}");
        }
    }
}

#[test]
fn 頭がずれた素材でも番号が先頭から数える() {
    let mut dec = open("offset_bf.mp4");
    let frames = drain_numbers(dec.as_mut());
    let first = frames[0].1;
    for (index, number) in frames {
        assert_eq!(number, first + index, "コマ {index}");
    }
}

#[test]
fn 飛び飛びのシークで目的のコマが出る() {
    let mut dec = open("cfr.mp4");
    let fps = dec.info().fps.expect("CFR");
    for target in [0, 59, 30, 13, 14, 45, 3, 3, 58] {
        dec.seek(RationalTime::from_frames(target, fps).unwrap())
            .expect("seek");
        let f = dec.next_frame().expect("復号").expect("コマ");
        assert_eq!(f.index, target);
        assert_eq!(read_frame_number(f.image.as_ref().unwrap(), 1), target);
    }
}

#[test]
fn 十ビットの素材は八ビットに落とさず運ぶ() {
    let mut dec = open("p10.mp4");
    let mut checked = 0;
    while let Some(f) = dec.next_frame().expect("復号") {
        let image = f.image.as_ref().expect("CPU の絵");
        assert_eq!(read_frame_number(image, 1), f.index);
        if f.index % 10 == 5 {
            let kinds = distinct_in_row(image, 100);
            assert!(kinds > 280, "コマ {}: 値が {kinds} 種類しかない", f.index);
            checked += 1;
        }
    }
    assert_eq!(checked, 3);
}

#[test]
fn gpu_の文脈なしでは断る() {
    let path = fixtures(&FILES).join("cfr.mp4");
    assert!(matches!(
        CuvidFactory.open(&path, None),
        Err(MmError::Unsupported(_))
    ));
}

#[test]
fn コマを捨て続けても輪が詰まらない() {
    let mut dec = open("cfr.mp4");
    let mut total = 0;
    for _ in 0..8 {
        dec.seek(RationalTime::ZERO).expect("seek");
        while dec.next_frame().expect("復号").is_some() {
            total += 1;
        }
    }
    assert_eq!(total, 8 * 60);
}

#[test]
fn コマを持ち続けても復号が止まらない() {
    let ctx = ctx();
    let path = fixtures(&FILES).join("cfr.mp4");
    let mut dec = CuvidDecoder::open(&path, &ctx).expect("開ける");
    let mut held = Vec::new();
    for round in 0..3 {
        dec.seek(RationalTime::ZERO).expect("seek");
        while let Some(f) = dec.next_frame().expect("復号") {
            held.push(f);
        }
        assert_eq!(held.len(), 60 * (round + 1));
    }
}

/// GPU が読み取りを遅らせても、CUDA が枠を先に上書きして別のコマが混ざることがない。
fn mixed_frames(wait_for_gpu: bool) -> usize {
    let ctx = ctx();
    let path = fixtures(&FILES).join("cfr.mp4");
    let mut dec = CuvidDecoder::open(&path, &ctx).expect("開ける");
    dec.set_gpu_wait(wait_for_gpu);
    busy_gpu(&ctx, Duration::from_millis(600));
    let mut frames = Vec::new();
    while let Some(f) = dec.next_frame().expect("復号") {
        frames.push(f);
    }
    let mut reader = FrameReader::new(&ctx);
    frames
        .iter()
        .filter(|f| read_frame_number(&reader.read(f), 1) != f.index)
        .count()
}

#[test]
fn gpu_が遅れても別のコマが混ざらない() {
    for _ in 0..3 {
        assert_eq!(mixed_frames(true), 0);
    }
}

#[test]
fn 待ちを外すと別のコマが混ざる_対照() {
    let mixed = mixed_frames(false);
    assert!(
        mixed > 0,
        "待ちを外しても混ざらないなら、この負荷では検査にならない"
    );
}

/// NVENC で作った 4K の HEVC（d3d12va が落とす形）を cuvid が復号できる。NVENC が無い PC では素材が無く飛ばす。
#[test]
fn nvenc_の_4k_hevc_を復号できる() {
    let path = fixtures(&FILES).join("nvenc4k.mp4");
    if !path.exists() {
        eprintln!("nvenc4k.mp4 が無いので飛ばす");
        return;
    }
    let mut dec = factory().open(&path, None).expect("開ける");
    assert_eq!(dec.info().size.width, 3840);
    let frames = drain_numbers_scaled(dec.as_mut());
    assert_eq!(frames.len(), 24);
    for (index, number) in frames {
        assert_eq!(index, number, "コマ {index}");
    }
}

fn drain_numbers_scaled(dec: &mut dyn Decoder) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    while let Some(f) = dec.next_frame().expect("復号") {
        out.push((
            f.index,
            read_frame_number(f.image.as_ref().expect("CPU の絵"), 12),
        ));
    }
    out
}
