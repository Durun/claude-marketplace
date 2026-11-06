//! 2 面（NV12 か P010）を線形の `Rgba16Float` に変えるパス。描画器が読める形式で渡すための変換で、
//! 10 bit は符号語のまま浮動小数にして 8 bit に落とさない。

use mm_decode_hw::render::{
    fragment_entry, texture_binding, uniform_binding, ParamsBuffer, YuvPass,
};
use mm_decode_hw::yuv::YuvParams;

use crate::wrap::PlaneViews;

const FRAGMENT: &str = r"
@group(0) @binding(0) var tex_y: texture_2d<f32>;
@group(0) @binding(1) var tex_uv: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;

fn chroma_at(x: u32, y: u32) -> vec2<f32> {
    let t = chroma_taps(x, y);
    let c00 = textureLoad(tex_uv, vec2<u32>(t.x0, t.y0), 0).rg;
    let c10 = textureLoad(tex_uv, vec2<u32>(t.x1, t.y0), 0).rg;
    let c01 = textureLoad(tex_uv, vec2<u32>(t.x0, t.y1), 0).rg;
    let c11 = textureLoad(tex_uv, vec2<u32>(t.x1, t.y1), 0).rg;
    return chroma_mix(c00, c10, c01, c11, t) * params.code_scale;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<u32>(pos.xy);
    let y = textureLoad(tex_y, xy, 0).r * params.code_scale;
    let uv = chroma_at(xy.x, xy.y);
    return yuv_to_linear(y, uv.x, uv.y);
}
";

/// YUV → `Rgba16Float` の描画パス。
pub struct YuvConverter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pass: YuvPass,
    params: ParamsBuffer,
}

impl YuvConverter {
    /// パイプラインを作る。
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let entries = [
            fragment_entry(0, texture_binding()),
            fragment_entry(1, texture_binding()),
            fragment_entry(2, uniform_binding()),
        ];
        Self {
            pass: YuvPass::new(&device, "yuv420 to linear rgba16f", FRAGMENT, &entries),
            params: ParamsBuffer::new(&device),
            device,
            queue,
        }
    }

    /// 2 面を読んで `target` に描き、キューに出す。
    pub fn submit(
        &mut self,
        planes: &PlaneViews,
        params: YuvParams,
        target: &wgpu::Texture,
    ) -> wgpu::SubmissionIndex {
        self.params.update(&self.queue, params);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pass.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&planes.y),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&planes.uv),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.params.buffer().as_entire_binding(),
                },
            ],
        });
        self.pass.draw(&self.device, &self.queue, &bind, target)
    }
}
