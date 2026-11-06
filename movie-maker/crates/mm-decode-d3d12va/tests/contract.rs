//! d3d12va のデコーダーを、`mm-testkit` の契約と実機の GPU で確かめる。
//! 素材は `fixtures/media/generate.sh`（無ければ作る）。GPU と NVIDIA のドライバーが要る。
#![cfg(feature = "d3d12va")]

use mm_decode_hw::testing as common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    busy_gpu, distinct_in_row, fixtures, gpu, read_frame_number, FrameReader, ReadbackFactory,
};
use mm_decode_api::{Decoder, DecoderFactory};
use mm_decode_d3d12va::{gpu_request, D3d12vaDecoder, D3d12vaFactory, PROBE_ENV};
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
    std::env::set_var(PROBE_ENV, env!("CARGO_BIN_EXE_mm-d3d12va-probe"));
    gpu(gpu_request(None).expect("D3D12 のアダプター"))
}

fn factory() -> Arc<dyn DecoderFactory> {
    Arc::new(ReadbackFactory::new(D3d12vaFactory, &ctx()))
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
        assert_eq!(
            read_frame_number(f.image.as_ref().unwrap(), 1),
            target,
            "シーク先の絵"
        );
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
            // 傾きは 320 画素で 876 段。8 bit なら 219 段に潰れる。
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
        D3d12vaFactory.open(&path, None),
        Err(MmError::Unsupported(_))
    ));
}

/// `take_frame` のような取り出しをしなくても、コマを捨てるだけでプールが尽きない。
#[test]
fn コマを捨て続けてもプールが尽きない() {
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

/// 借りを長く持っても、面は変換の完了で返るので復号が止まらない。
#[test]
fn コマを持ち続けても復号が止まらない() {
    let ctx = ctx();
    let path = fixtures(&FILES).join("cfr.mp4");
    let mut dec = D3d12vaDecoder::open(&path, &ctx).expect("開ける");
    let mut held = Vec::new();
    for round in 0..3 {
        dec.seek(RationalTime::ZERO).expect("seek");
        while let Some(f) = dec.next_frame().expect("復号") {
            held.push(f);
        }
        assert_eq!(held.len(), 60 * (round + 1));
    }
    assert!(dec.pending_surfaces() <= 8, "握る面は上限まで");
}

/// 変換を GPU が遅れて実行しても、面が先に上書きされて別のコマが混ざることがない。
fn mixed_frames(wait_for_gpu: bool) -> usize {
    let ctx = ctx();
    let path = fixtures(&FILES).join("cfr.mp4");
    let mut dec = D3d12vaDecoder::open(&path, &ctx).expect("開ける");
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

/// NVIDIA の d3d12va が落とす素材（NVENC の 4K HEVC）は、描画器のデバイスで復号する前に別プロセスで断られ、
/// デバイスが消えない。ドライバーが直って復号できるなら開けてもよい。
#[test]
fn 落ちる素材は別プロセスで断られデバイスを守る() {
    let ctx = ctx();
    let dir = fixtures(&FILES);
    let nvenc = dir.join("nvenc4k.mp4");
    if !nvenc.exists() {
        eprintln!("nvenc4k.mp4 が無いので飛ばす");
        return;
    }
    let opened = factory().open(&nvenc, None);
    eprintln!("nvenc4k.mp4 の d3d12va: {:?}", opened.as_ref().err());
    if let Err(e) = &opened {
        assert!(matches!(e, MmError::Unsupported(_)), "{e}");
    }
    drop(opened);
    // SAFETY: as_hal の参照はこの文の間だけ使う。
    let hal = unsafe { ctx.device.as_hal::<wgpu::hal::api::Dx12>() }.expect("D3D12");
    assert!(
        unsafe { hal.raw_device().GetDeviceRemovedReason() }.is_ok(),
        "描画器のデバイスが消えていない"
    );
    let mut dec = open("cfr.mp4");
    assert!(dec.next_frame().expect("復号").is_some());
}

#[test]
fn 待ちを外すと別のコマが混ざる_対照() {
    let mixed = mixed_frames(false);
    assert!(
        mixed > 0,
        "待ちを外しても混ざらないなら、この負荷では検査にならない"
    );
}
