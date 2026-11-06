//! レンダラーの契約テスト（`mm_testkit::render::contract`）。GPU が無い環境では飛ばし、
//! `MM_REQUIRE_GPU` があれば失敗にする。

use std::path::Path;
use std::sync::Arc;

use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::Renderer;
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, vertical_project};
use mm_testkit::node::MapRegistry;
use mm_testkit::render::{contract, RenderFixture};

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

#[test]
fn wgpu_のレンダラーは契約を満たす() {
    let Some(ctx) = gpu() else {
        return;
    };
    let media = SampleMedia::default();
    let (project, comp, _target) = vertical_project(&media, Path::new("out.mp4"));
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    contract(
        || {
            Box::new(WgpuRenderer::new(
                ctx.clone(),
                Arc::new(SampleDecoderFactory::default()),
                Arc::new(SampleMixerFactory),
                sample_services(),
            )) as Box<dyn Renderer>
        },
        &RenderFixture {
            project: &project,
            composition: &comp,
            nodes: &nodes,
            size: media.size,
        },
    );
}
