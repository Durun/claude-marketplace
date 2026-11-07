//! cuvid のデコーダーを、`mm-testkit` の契約と実機の GPU で確かめる。
//! 素材は `fixtures/media/generate.sh`（無ければ作る）。NVIDIA の GPU とドライバーが要る。

use mm_decode_hw::testing as common;

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

/// GPU のコンテキストを作って基本の素材を開けるか。GPU や CUDA が無い環境（CI）ではテストをスキップする。
fn available() -> bool {
    common::gpu_available(|| {
        let ctx = GpuContext::shared(&gpu_request(None)?)?;
        CuvidDecoder::open(&fixtures(&FILES).join("cfr.mp4"), &ctx).map(drop)
    })
}

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
    while let Some(f) = dec.next_frame().expect("デコード") {
        out.push((
            f.index,
            read_frame_number(f.image.as_ref().expect("CPU の画像"), 1),
        ));
    }
    out
}

#[test]
fn デコーダーが契約を通る() {
    if !available() {
        return;
    }
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
    if !available() {
        return;
    }
    vfr_contract(|| open("vfr.mp4"));
    vfr_contract(|| open("vfr_bf.mp4"));
}

#[test]
fn 焼き込んだ番号がフレームの番号と一致する() {
    if !available() {
        return;
    }
    for name in ["cfr.mp4", "ntsc.mp4", "vfr.mp4", "vfr_bf.mp4"] {
        let mut dec = open(name);
        let frames = drain_numbers(dec.as_mut());
        assert_eq!(frames.len() as i64, dec.info().frame_count(), "{name}");
        for (index, number) in frames {
            assert_eq!(index, number, "{name} のフレーム {index}");
        }
    }
}

#[test]
fn 先頭がずれた素材でも番号が先頭から数える() {
    if !available() {
        return;
    }
    let mut dec = open("offset_bf.mp4");
    let frames = drain_numbers(dec.as_mut());
    let first = frames[0].1;
    for (index, number) in frames {
        assert_eq!(number, first + index, "フレーム {index}");
    }
}

#[test]
fn 飛び飛びのシークで目的のフレームが出る() {
    if !available() {
        return;
    }
    let mut dec = open("cfr.mp4");
    let fps = dec.info().fps.expect("CFR");
    for target in [0, 59, 30, 13, 14, 45, 3, 3, 58] {
        dec.seek(RationalTime::from_frames(target, fps).unwrap())
            .expect("seek");
        let f = dec.next_frame().expect("デコード").expect("フレーム");
        assert_eq!(f.index, target);
        assert_eq!(read_frame_number(f.image.as_ref().unwrap(), 1), target);
    }
}

#[test]
fn 十ビットの素材は八ビットに下げずに運ぶ() {
    if !available() {
        return;
    }
    let mut dec = open("p10.mp4");
    let mut checked = 0;
    while let Some(f) = dec.next_frame().expect("デコード") {
        let image = f.image.as_ref().expect("CPU の画像");
        assert_eq!(read_frame_number(image, 1), f.index);
        if f.index % 10 == 5 {
            let kinds = distinct_in_row(image, 100);
            assert!(
                kinds > 280,
                "フレーム {}: 値が {kinds} 種類しかない",
                f.index
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 3);
}

#[test]
fn gpu_のコンテキストなしでは拒否する() {
    if !available() {
        return;
    }
    let path = fixtures(&FILES).join("cfr.mp4");
    assert!(matches!(
        CuvidFactory.open(&path, None),
        Err(MmError::Unsupported(_))
    ));
}

#[test]
fn フレームを破棄し続けてもリングバッファがストールしない() {
    if !available() {
        return;
    }
    let mut dec = open("cfr.mp4");
    let mut total = 0;
    for _ in 0..8 {
        dec.seek(RationalTime::ZERO).expect("seek");
        while dec.next_frame().expect("デコード").is_some() {
            total += 1;
        }
    }
    assert_eq!(total, 8 * 60);
}

#[test]
fn フレームを持ち続けてもデコードが止まらない() {
    if !available() {
        return;
    }
    let ctx = ctx();
    let path = fixtures(&FILES).join("cfr.mp4");
    let mut dec = CuvidDecoder::open(&path, &ctx).expect("開ける");
    let mut held = Vec::new();
    for round in 0..3 {
        dec.seek(RationalTime::ZERO).expect("seek");
        while let Some(f) = dec.next_frame().expect("デコード") {
            held.push(f);
        }
        assert_eq!(held.len(), 60 * (round + 1));
    }
}

/// GPU が読み取りを遅らせても、CUDA がスロットを先に上書きして別のフレームが混ざることがない。
fn mixed_frames(wait_for_gpu: bool) -> usize {
    let ctx = ctx();
    let path = fixtures(&FILES).join("cfr.mp4");
    let mut dec = CuvidDecoder::open(&path, &ctx).expect("開ける");
    dec.set_gpu_wait(wait_for_gpu);
    busy_gpu(&ctx, Duration::from_millis(600));
    let mut frames = Vec::new();
    while let Some(f) = dec.next_frame().expect("デコード") {
        frames.push(f);
    }
    let mut reader = FrameReader::new(&ctx);
    frames
        .iter()
        .filter(|f| read_frame_number(&reader.read(f), 1) != f.index)
        .count()
}

#[test]
fn gpu_が遅れても別のフレームが混ざらない() {
    if !available() {
        return;
    }
    for _ in 0..3 {
        assert_eq!(mixed_frames(true), 0);
    }
}

#[test]
fn pq_と_hlg_の素材は拒否する() {
    if !available() {
        return;
    }
    for trc in ["smpte2084", "arib-std-b67"] {
        let path = common::hdr_transfer_fixture(trc);
        let opened = factory().open(&path, None);
        assert!(
            matches!(opened, Err(MmError::Unsupported(_))),
            "{trc}: {:?}",
            opened.err()
        );
    }
}

#[test]
fn 色差は隣の画素へ線形に補間される() {
    if !available() {
        return;
    }
    let path = common::chroma_ramp_fixture();
    let mut dec = factory().open(&path, None).expect("開ける");
    let frame = dec.next_frame().expect("デコード").expect("フレーム");
    let ratio = common::equal_neighbor_ratio(frame.image.as_ref().expect("CPU の画像"), 100);
    assert!(
        ratio < 0.15,
        "隣と同じ値の画素が {ratio:.2} の割合で並ぶ（最近傍の読み）"
    );
}

/// 別のスレッドが同じキューに提出し続けても、変換がコピーの完了を待たずに走って別のフレームが混ざることがない。
#[test]
fn 別のスレッドの提出が挟まってもコピーの完了を待つ() {
    if !available() {
        return;
    }
    let ctx = ctx();
    let mut dec = open("cfr.mp4");
    let frames = common::with_background_submits(&ctx, || drain_numbers(dec.as_mut()));
    assert_eq!(frames.len() as i64, dec.info().frame_count());
    for (index, number) in frames {
        assert_eq!(index, number, "フレーム {index}");
    }
}

#[test]
fn 待ちを外すと別のフレームが混ざる_対照() {
    if !available() {
        return;
    }
    let mixed = mixed_frames(false);
    assert!(
        mixed > 0,
        "待ちを外しても混ざらないなら、この負荷では検査にならない"
    );
}

/// NVENC で作った 4K の HEVC（d3d12va がクラッシュさせる形）を cuvid がデコードできる。NVENC が無い PC では素材が無くスキップする。
#[test]
fn nvenc_の_4k_hevc_をデコードできる() {
    if !available() {
        return;
    }
    let path = fixtures(&FILES).join("nvenc4k.mp4");
    if !path.exists() {
        eprintln!("nvenc4k.mp4 が無いのでスキップする");
        return;
    }
    let mut dec = factory().open(&path, None).expect("開ける");
    assert_eq!(dec.info().size.width, 3840);
    let frames = drain_numbers_scaled(dec.as_mut());
    assert_eq!(frames.len(), 24);
    for (index, number) in frames {
        assert_eq!(index, number, "フレーム {index}");
    }
}

fn drain_numbers_scaled(dec: &mut dyn Decoder) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    while let Some(f) = dec.next_frame().expect("デコード") {
        out.push((
            f.index,
            read_frame_number(f.image.as_ref().expect("CPU の画像"), 12),
        ));
    }
    out
}
