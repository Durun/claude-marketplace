//! デバイスの消失からの復帰。デバイスを壊して（`Device::destroy`）次のフレームが描けることと、
//! 消失の通知だけを差し込んだ（`mark_lost`）ときも作り直すことを確かめる。
//! 共有のコンテキストを壊すので、ほかのテストと同じプロセスに置かない（別の実行ファイル。docs/ci.md）。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::path::Path;
use std::sync::Arc;

use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, vertical_project};
use mm_testkit::node::MapRegistry;
use mm_types::{RationalTime, RgbaImage};

use mm_node_api::Quality;

fn gpu() -> Option<Arc<GpuContext>> {
    match GpuContext::shared(&GpuRequest::default()) {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
    }
}

fn render(r: &mut WgpuRenderer, nodes: &MapRegistry) -> Arc<RgbaImage> {
    let media = SampleMedia::default();
    let (project, id, _) = vertical_project(&media, Path::new("out.mp4"));
    let frame = r
        .render_frame(&RenderRequest {
            project: &project,
            composition: &id,
            time: RationalTime::from_frames(1, media.fps).expect("表せる"),
            size: media.size,
            quality: Quality::Full,
            nodes,
        })
        .expect("描ける");
    r.gpu().download(&frame).expect("リードバックできる")
}

#[test]
fn デバイスを壊しても次のフレームから描き続ける() {
    let Some(ctx) = gpu() else { return };
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    let mut r = WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        sample_services(),
    );
    let before = render(&mut r, &nodes);
    assert_eq!(r.recoveries(), 0);

    // 実際の消失: wgpu がデバイスを失ったと通知する（NVIDIA の d3d12va の失敗と同じ経路）。
    ctx.device.destroy();
    let after = render(&mut r, &nodes);
    assert_eq!(r.recoveries(), 1, "消失を見つけて作り直す");
    assert!(
        !Arc::ptr_eq(r.context(), &ctx),
        "新しいコンテキストに持ち替える"
    );
    assert_eq!(*before, *after, "作り直した後も同じ画像");
    assert!(
        Arc::ptr_eq(
            &GpuContext::shared(&GpuRequest::default()).expect("共有"),
            r.context()
        ),
        "共有のコンテキストも差し替わる"
    );

    // 通知だけの消失: デコーダーなど、先に気づいた側からの知らせ。
    r.context().mark_lost();
    let again = render(&mut r, &nodes);
    assert_eq!(r.recoveries(), 2);
    assert_eq!(*before, *again);
    assert!(r.take_errors().is_empty());
}
