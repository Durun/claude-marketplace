//! 2 面（NV12 か P010）を線形の `Rgba16Float` に変えるパス。描画器が読める形式で渡すための変換で、
//! 10 bit は符号語のまま浮動小数にして 8 bit に落とさない。

use mm_gpu::FRAME_FORMAT;

use crate::shared::yuv::{YuvParams, PARAMS_BYTES, YUV_WGSL};
use crate::wrap::PlaneViews;

const FRAGMENT: &str = r"
@group(0) @binding(0) var tex_y: texture_2d<f32>;
@group(0) @binding(1) var tex_uv: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<i32>(pos.xy);
    let y = textureLoad(tex_y, xy, 0).r * params.code_scale;
    let uv = textureLoad(tex_uv, xy / 2, 0).rg * params.code_scale;
    return yuv_to_linear(y, uv.x, uv.y);
}
";

/// YUV → `Rgba16Float` の描画パス。
pub struct YuvConverter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// 直近のパラメーターと、その一様バッファ。素材の途中で変わることは少ないので使い回す。
    uniform: Option<(YuvParams, wgpu::Buffer)>,
}

impl YuvConverter {
    /// パイプラインを作る。
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv420 to linear rgba16f"),
            source: wgpu::ShaderSource::Wgsl(format!("{YUV_WGSL}{FRAGMENT}").into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv planes"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv420 to linear rgba16f"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(FRAME_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            layout,
            uniform: None,
        }
    }

    fn uniform_for(&mut self, params: YuvParams) -> wgpu::Buffer {
        if let Some((p, buf)) = &self.uniform {
            if *p == params {
                return buf.clone();
            }
        }
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv params"),
            size: PARAMS_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buf, 0, &params.to_bytes());
        self.uniform = Some((params, buf.clone()));
        buf
    }

    /// 2 面を読んで `target` に描き、キューに出す。
    pub fn submit(
        &mut self,
        planes: &PlaneViews,
        params: YuvParams,
        target: &wgpu::Texture,
    ) -> wgpu::SubmissionIndex {
        let uniform = self.uniform_for(params);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
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
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yuv to linear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()])
    }
}
