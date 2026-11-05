//! 実機の GPU と素材が要るテスト。素材は環境変数 `MM_SPIKE_SAMPLE`（H.264 の CFR）で渡す。無ければ何もしない。

#![cfg(feature = "d3d12va")]

use std::path::PathBuf;

use mm_decode_api::{Decoder, DecoderFactory};
use mm_decode_d3d12va::{D3d12vaDecoder, D3d12vaFactory, GpuContext};
use mm_types::RationalTime;

fn sample() -> Option<PathBuf> {
    std::env::var_os("MM_SPIKE_SAMPLE").map(PathBuf::from)
}

#[test]
fn コマは_gpu_の参照で返り_シーク先の番号が出る() {
    let Some(path) = sample() else {
        eprintln!("MM_SPIKE_SAMPLE が無いので飛ばす");
        return;
    };
    let gpu = GpuContext::new().expect("D3D12 の GPU");
    let factory = D3d12vaFactory::new(gpu);
    assert_eq!(factory.id(), "d3d12va");
    let mut dec = factory.open(&path).expect("open");
    let fps = dec.info().fps.expect("CFR");
    let first = dec.next_frame().expect("next").expect("frame");
    assert_eq!(first.index, 0);
    assert!(first.image.is_none() && first.gpu.is_some());

    let target = 90;
    dec.seek(RationalTime::from_frames(target, fps))
        .expect("seek");
    let f = dec.next_frame().expect("next").expect("frame");
    assert_eq!(f.index, target);
}

#[test]
fn 取り出したコマは_nv12_の_2_面を持つ() {
    let Some(path) = sample() else {
        return;
    };
    let gpu = GpuContext::new().expect("D3D12 の GPU");
    let mut dec = D3d12vaDecoder::open(&path, gpu).expect("open");
    let f = dec.next_frame().expect("next").expect("frame");
    let handle = f.gpu.expect("gpu");
    let held = dec.take_frame(&handle).expect("held");
    assert_eq!(
        (held.nv12.width, held.nv12.height),
        (handle.size.width, handle.size.height)
    );
    assert!(dec.take_frame(&handle).is_none(), "取り出した後は残らない");
}
