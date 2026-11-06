//! GPU の文脈とヘルパーの検査。GPU が無い環境では飛ばし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::Arc;

use mm_gpu::{GpuContext, GpuRequest, WgpuGpu};
use mm_node_api::{FrameRef, GpuHelper, GpuTexture};
use mm_types::{GpuAdapterId, MmError, Size};

fn gpu() -> Option<Arc<GpuContext>> {
    match GpuContext::shared(&GpuRequest::default()) {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いので飛ばす: {e}");
            None
        }
    }
}

#[test]
fn shared_は合わない求めを黙って捨てない() {
    let Some(ctx) = gpu() else {
        return;
    };
    let other_adapter = GpuRequest {
        adapter: Some(GpuAdapterId {
            vendor: ctx.adapter.vendor ^ 1,
            device: ctx.adapter.device,
            name: "別の GPU".into(),
            luid: None,
        }),
        ..GpuRequest::default()
    };
    assert!(matches!(
        GpuContext::shared(&other_adapter),
        Err(MmError::InvalidArgument(_))
    ));
    let more_features = GpuRequest {
        optional_features: GpuRequest::default().optional_features | wgpu::Features::SHADER_F16,
        ..GpuRequest::default()
    };
    assert!(matches!(
        GpuContext::shared(&more_features),
        Err(MmError::InvalidArgument(_))
    ));
    let same = GpuRequest {
        adapter: Some(ctx.adapter.clone()),
        ..GpuRequest::default()
    };
    assert!(Arc::ptr_eq(&GpuContext::shared(&same).unwrap(), &ctx));
}

/// 全バックエンドで作った後の、バックエンドを絞った求め（`decode: ffmpeg` の後の `decode: auto`）。
#[test]
fn shared_は選んだアダプターが求めの範囲にあれば同じ文脈を返す() {
    let Some(ctx) = gpu() else {
        return;
    };
    let narrowed = GpuRequest {
        backends: wgpu::Backends::from(ctx.backend()),
        optional_features: wgpu::Features::empty(),
        ..GpuRequest::default()
    };
    assert!(Arc::ptr_eq(&GpuContext::shared(&narrowed).unwrap(), &ctx));
    let elsewhere = GpuRequest {
        backends: wgpu::Backends::all() - wgpu::Backends::from(ctx.backend()),
        ..GpuRequest::default()
    };
    assert!(matches!(
        GpuContext::shared(&elsewhere),
        Err(MmError::InvalidArgument(_))
    ));
}

#[test]
fn 保存した識別子の_luid_が古くても同じ_gpu_を選ぶ() {
    let Some(ctx) = gpu() else {
        return;
    };
    let stale = GpuAdapterId {
        luid: ctx.adapter.luid.map(|l| l ^ 0xffff),
        ..ctx.adapter.clone()
    };
    let ctx2 = GpuContext::with(&GpuRequest {
        adapter: Some(stale),
        ..GpuRequest::default()
    })
    .expect("LUID が古くても機種で見つかる");
    assert!(ctx2.adapter.matches(&ctx.adapter));
}

#[test]
fn 読み戻しは大きさが合わなければ_unsupported() {
    let Some(ctx) = gpu() else {
        return;
    };
    let mut helper = WgpuGpu::new(ctx);
    let FrameRef::Gpu(tex) = helper.allocate(Size::new(8, 4)).unwrap() else {
        panic!("GPU のコマ");
    };
    let lying = FrameRef::Gpu(GpuTexture {
        id: tex.id,
        size: Size::new(16, 4),
    });
    assert!(matches!(
        helper.download(&lying),
        Err(MmError::Unsupported(_))
    ));
    let honest = FrameRef::Gpu(tex);
    assert_eq!(helper.download(&honest).unwrap().size(), Size::new(8, 4));
}
