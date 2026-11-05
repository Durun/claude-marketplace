//! NV12 の 2 面を RGBA8 に変えるパス。描画器が NV12 を読む所の代わりで、wgpu が面を読めることの確かめに使う。

use ffmpeg_next::frame;
use mm_types::{MmError, Result, Size};

use crate::gpu::GpuContext;

const SHADER: &str = r"
@group(0) @binding(0) var tex_y: texture_2d<f32>;
@group(0) @binding(1) var tex_uv: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<i32>(pos.xy);
    let y = (textureLoad(tex_y, xy, 0).r - 16.0 / 255.0) * (255.0 / 219.0);
    let uv = (textureLoad(tex_uv, xy / 2, 0).rg - 128.0 / 255.0) * (255.0 / 224.0);
    let r = y + 1.5748 * uv.y;
    let g = y - 0.1873 * uv.x - 0.4681 * uv.y;
    let b = y + 1.8556 * uv.x;
    return vec4<f32>(clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
";

/// 出力の形式。
const OUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// NV12 → RGBA8 の描画パス。
pub struct Converter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    size: Size,
}

impl Converter {
    /// `size` の出力先を持つ。
    pub fn new(gpu: &GpuContext, size: Size) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12 to rgba"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
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
            label: Some("nv12 planes"),
            entries: &[tex_entry(0), tex_entry(1)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12 to rgba"),
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
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            pipeline,
            layout,
            target,
            target_view,
            size,
        }
    }

    /// 2 面を読んで出力先に描き、キューに出す。
    pub fn submit(
        &self,
        gpu: &GpuContext,
        y: &wgpu::TextureView,
        uv: &wgpu::TextureView,
    ) -> wgpu::SubmissionIndex {
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(y),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(uv),
                },
            ],
        });
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
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()])
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
}

/// CPU の NV12 を NV12 のテクスチャにアップロードする係（CPU 経由の比較用）。
pub struct CpuUploader {
    texture: wgpu::Texture,
    pub y: wgpu::TextureView,
    pub uv: wgpu::TextureView,
}

impl CpuUploader {
    /// `size` の NV12 を持つ。
    pub fn new(gpu: &GpuContext, size: Size) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cpu nv12"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::NV12,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = |format, aspect| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                aspect,
                ..Default::default()
            })
        };
        let y = view(wgpu::TextureFormat::R8Unorm, wgpu::TextureAspect::Plane0);
        let uv = view(wgpu::TextureFormat::Rg8Unorm, wgpu::TextureAspect::Plane1);
        Self { texture, y, uv }
    }

    /// 降ろした NV12 のコマを 2 面ともアップロードする。
    pub fn upload(&self, gpu: &GpuContext, sw: &frame::Video) {
        let (w, h) = (sw.width(), sw.height());
        for (plane, aspect, pw, ph) in [
            (0, wgpu::TextureAspect::Plane0, w, h),
            (1, wgpu::TextureAspect::Plane1, w / 2, h / 2),
        ] {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect,
                },
                sw.data(plane),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(sw.stride(plane) as u32),
                    rows_per_image: Some(ph),
                },
                wgpu::Extent3d {
                    width: pw,
                    height: ph,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}
