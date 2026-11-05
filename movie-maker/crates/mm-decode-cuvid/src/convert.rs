//! 共有バッファの NV12 / P010 を線形の `Rgba16Float` に変えるパス。10 bit は符号語のまま浮動小数にする。

use mm_gpu::FRAME_FORMAT;

use crate::share::SharedRing;
use crate::shared::yuv::{YuvParams, PARAMS_BYTES, YUV_WGSL};

const FRAGMENT: &str = r"
struct Layout { pitch: u32, uv_offset: u32, bytes_per_sample: u32, divisor: f32 }
@group(0) @binding(0) var<storage, read> yuv: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<uniform> geom: Layout;

fn code_at(byte: u32) -> f32 {
    let word = yuv[byte >> 2u];
    if (geom.bytes_per_sample == 1u) {
        return f32((word >> ((byte & 3u) * 8u)) & 0xffu);
    }
    return f32((word >> ((byte & 2u) * 8u)) & 0xffffu) / geom.divisor;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(pos.x);
    let y = u32(pos.y);
    let bps = geom.bytes_per_sample;
    let luma = code_at(y * geom.pitch + x * bps);
    let o = geom.uv_offset + (y / 2u) * geom.pitch + (x / 2u) * 2u * bps;
    return yuv_to_linear(luma, code_at(o), code_at(o + bps));
}
";

/// 共有バッファ → `Rgba16Float` の描画パス。
pub struct BufferConverter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    binds: Vec<wgpu::BindGroup>,
    params: wgpu::Buffer,
    last: Option<YuvParams>,
}

impl BufferConverter {
    /// 輪の枠ごとに束縛を作る。`bits` は 1 成分のビット深度。
    pub fn new(queue: wgpu::Queue, ring: &SharedRing, bits: u32) -> Self {
        let device = ring.device().clone();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv420 buffer to linear rgba16f"),
            source: wgpu::ShaderSource::Wgsl(format!("{YUV_WGSL}{FRAGMENT}").into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, uniform),
                entry(2, uniform),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv420 buffer to linear rgba16f"),
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
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv params"),
            size: PARAMS_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes_per_sample = if bits == 8 { 1u32 } else { 2 };
        let divisor = if bits == 8 {
            1.0
        } else {
            (1u32 << (16 - bits)) as f32
        };
        let mut layout_bytes = [0u8; 16];
        layout_bytes[0..4].copy_from_slice(&ring.pitch().to_le_bytes());
        layout_bytes[4..8].copy_from_slice(&ring.uv_offset().to_le_bytes());
        layout_bytes[8..12].copy_from_slice(&bytes_per_sample.to_le_bytes());
        layout_bytes[12..16].copy_from_slice(&divisor.to_le_bytes());
        let layout_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv layout"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&layout_buf, 0, &layout_bytes);
        let binds = (0..ring.len())
            .map(|i| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: ring.slot(i).buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: layout_buf.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        Self {
            device,
            queue,
            pipeline,
            binds,
            params,
            last: None,
        }
    }

    /// 枠の YUV を読んで `target` に描き、キューに出す。
    pub fn submit(&mut self, slot: usize, params: YuvParams, target: &wgpu::Texture) {
        if self.last != Some(params) {
            self.queue.write_buffer(&self.params, 0, &params.to_bytes());
            self.last = Some(params);
        }
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
            pass.set_bind_group(0, &self.binds[slot], &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
    }
}
