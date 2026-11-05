//! 共有バッファの NV12 を RGBA8 に変えるパス。描画器が読む所の代わり。

use mm_types::{MmError, Result, Size};

use crate::gpu::GpuContext;
use crate::share::SharedRing;

const SHADER: &str = r"
struct Params { width: u32, uv_offset: u32 }
@group(0) @binding(0) var<storage, read> nv12: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;

fn byte_at(i: u32) -> f32 {
    return f32((nv12[i >> 2u] >> ((i & 3u) * 8u)) & 0xffu) / 255.0;
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(pos.x);
    let y = u32(pos.y);
    let luma = (byte_at(y * params.width + x) - 16.0 / 255.0) * (255.0 / 219.0);
    let o = params.uv_offset + (y / 2u) * params.width + (x / 2u) * 2u;
    let u = (byte_at(o) - 128.0 / 255.0) * (255.0 / 224.0);
    let v = (byte_at(o + 1u) - 128.0 / 255.0) * (255.0 / 224.0);
    let rgb = vec3<f32>(luma + 1.5748 * v, luma - 0.1873 * u - 0.4681 * v, luma + 1.8556 * u);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
";

const OUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// 共有バッファ → RGBA8 の描画パス。
pub struct BufferConverter {
    pipeline: wgpu::RenderPipeline,
    binds: Vec<wgpu::BindGroup>,
    target_view: wgpu::TextureView,
    target: wgpu::Texture,
    size: Size,
}

impl BufferConverter {
    /// 輪の枠ごとに束縛を作る。
    pub fn new(gpu: &GpuContext, size: Size, ring: &SharedRing) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12 buffer to rgba"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
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
                entry(
                    1,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12 buffer to rgba"),
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
                targets: &[Some(OUT_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let mut params = [0u8; 8];
        params[..4].copy_from_slice(&size.width.to_le_bytes());
        params[4..].copy_from_slice(&ring.uv_offset().to_le_bytes());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&uniform, 0, &params);
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
                            resource: uniform.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("converted rgba"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self {
            pipeline,
            binds,
            target_view: target.create_view(&wgpu::TextureViewDescriptor::default()),
            target,
            size,
        }
    }

    /// 出力先を CPU に読み戻す（確かめ用）。RGBA8 の `width * height * 4` バイト。
    pub fn read_back(&self, gpu: &GpuContext) -> Result<Vec<u8>> {
        let (w, h) = (self.size.width, self.size.height);
        let padded = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| MmError::Internal(format!("poll: {e}")))?;
        rx.recv()
            .map_err(|e| MmError::Internal(format!("map: {e}")))?
            .map_err(|e| MmError::Internal(format!("map: {e}")))?;
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| MmError::Internal(format!("map range: {e}")))?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h as usize {
            let start = row * padded as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        Ok(out)
    }

    /// 枠の NV12 を読んで出力先に描き、キューに出す。
    pub fn submit(&self, gpu: &GpuContext, slot: usize) -> wgpu::SubmissionIndex {
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
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
        gpu.queue.submit([enc.finish()])
    }
}
