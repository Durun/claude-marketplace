//! 書き出しの経路。描いたフレームを GPU で NV12（Rec.709、tv レンジ、Rec.1886 の符号化値）にして
//! リードバックし、エンコーダーの `push_nv12_frames` に押す。リードバックは数フレーム先行させ、
//! GPU の描画と CPU のエンコードを重ねる。エンコーダーが NV12 を受けなければ RGBA の既定の経路に戻る。

use std::collections::VecDeque;
use std::sync::Arc;

use mm_decode_api::{Encoder, Nv12Frame};
use mm_gpu::{GpuContext, Readback, WgpuGpu, FRAME_FORMAT};
use mm_render_api::{export_range_rgba, RangeRequest, RenderRequest};
use mm_types::{MmError, RationalTime, Result, Size};

use crate::renderer::WgpuRenderer;

/// 先行させるリードバックの数。描画からエンコードまでにこの分の遅れが入る。
const READBACK_AHEAD: usize = 3;
/// 変換先のプレーンの組の数。先行するリードバックが読んでいる間に次のフレームを書けるよう 2 組持つ。
const PLANE_SETS: usize = 2;
const Y_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const UV_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;

/// 線形の premultiplied の RGBA を、Rec.1886 の符号化値にしてから Rec.709・tv レンジの Y と CbCr にする。
/// アルファは使わず、黒の上に置いた色として扱う（premultiplied なのでそのままの値）。
const YUV_OUT_WGSL: &str = r"
@group(0) @binding(0) var frame: texture_2d<f32>;

const GAMMA_INV: f32 = 1.0 / 2.4;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn encoded(xy: vec2<u32>) -> vec3<f32> {
    let rgb = clamp(textureLoad(frame, xy, 0).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(rgb, vec3<f32>(GAMMA_INV));
}

fn luma(rgb: vec3<f32>) -> f32 {
    return dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_y(@builtin(position) pos: vec4<f32>) -> @location(0) f32 {
    let y = luma(encoded(vec2<u32>(pos.xy)));
    return (16.0 + 219.0 * y) / 255.0;
}

@fragment
fn fs_uv(@builtin(position) pos: vec4<f32>) -> @location(0) vec2<f32> {
    let base = vec2<u32>(pos.xy) * 2u;
    let c00 = encoded(base);
    let c10 = encoded(base + vec2<u32>(1u, 0u));
    let c01 = encoded(base + vec2<u32>(0u, 1u));
    let c11 = encoded(base + vec2<u32>(1u, 1u));
    let rgb = (c00 + c10 + c01 + c11) * 0.25;
    let y = luma(rgb);
    let cb = (rgb.b - y) / 1.8556;
    let cr = (rgb.r - y) / 1.5748;
    return (vec2<f32>(128.0) + 224.0 * vec2<f32>(cb, cr)) / 255.0;
}
";

/// 1 フレーム分の変換先。
struct PlaneSet {
    y: wgpu::Texture,
    uv: wgpu::Texture,
}

/// 提出済みのリードバック。
struct InFlight {
    index: i64,
    size: Size,
    y: Readback,
    uv: Readback,
}

/// NV12 への変換のパイプラインと、変換先のプレーン。
pub(crate) struct ExportPipe {
    layout: wgpu::BindGroupLayout,
    y_pipeline: wgpu::RenderPipeline,
    uv_pipeline: wgpu::RenderPipeline,
    size: Size,
    planes: Vec<PlaneSet>,
    next_set: usize,
}

impl ExportPipe {
    fn new(ctx: &GpuContext, size: Size) -> Result<Self> {
        if size.width % 2 == 1 || size.height % 2 == 1 {
            return Err(MmError::InvalidArgument(format!(
                "NV12 は偶数の大きさが要る: {}x{}",
                size.width, size.height
            )));
        }
        let device = &ctx.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mm-render yuv out"),
            source: wgpu::ShaderSource::Wgsl(YUV_OUT_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mm-render yuv out"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let planes = (0..PLANE_SETS)
            .map(|_| PlaneSet {
                y: plane_texture(device, size, Y_FORMAT),
                uv: plane_texture(
                    device,
                    Size::new(size.width / 2, size.height / 2),
                    UV_FORMAT,
                ),
            })
            .collect();
        Ok(Self {
            y_pipeline: pipeline(device, &module, &layout, "fs_y", Y_FORMAT),
            uv_pipeline: pipeline(device, &module, &layout, "fs_uv", UV_FORMAT),
            layout,
            size,
            planes,
            next_set: 0,
        })
    }

    /// 中間形式のテクスチャ `frame` を Y と UV に変換し、両方のリードバックを提出する。
    fn encode(&mut self, gpu: &mut WgpuGpu, frame: &wgpu::Texture) -> (Readback, Readback) {
        let ctx = gpu.context().clone();
        let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mm-render yuv out"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &frame.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            }],
        });
        let set = &self.planes[self.next_set];
        self.next_set = (self.next_set + 1) % PLANE_SETS;
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render.yuv_out"),
            });
        draw_plane(&mut encoder, &self.y_pipeline, &bind, &set.y);
        draw_plane(&mut encoder, &self.uv_pipeline, &bind, &set.uv);
        ctx.queue.submit([encoder.finish()]);
        (
            gpu.begin_readback(&set.y, 1),
            gpu.begin_readback(&set.uv, 2),
        )
    }
}

fn plane_texture(device: &wgpu::Device, size: Size, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mm-render yuv plane"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mm-render yuv out"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn draw_plane(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind: &wgpu::BindGroup,
    target: &wgpu::Texture,
) {
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("render.yuv_out"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.draw(0..3, 0..1);
}

impl WgpuRenderer {
    /// 書き出しの区間。GPU で NV12 にして先行してリードバックし、エンコーダーに押す。
    pub(crate) fn export_nv12(
        &mut self,
        req: &RangeRequest<'_>,
        encoder: &mut dyn Encoder,
        sink: &mut dyn FnMut(i64),
    ) -> Result<usize> {
        if let Err(MmError::Unsupported(_)) = encoder.push_nv12_frames(&[]) {
            return export_range_rgba(self, req, encoder, 1, sink);
        }
        let _span = tracing::info_span!("render.export", comp = %req.composition).entered();
        let mut in_flight: VecDeque<InFlight> = VecDeque::with_capacity(READBACK_AHEAD + 1);
        let mut count = 0;
        for index in req.range.frame_indices(req.fps) {
            let frame_req = RenderRequest {
                project: req.project,
                composition: req.composition,
                time: RationalTime::from_frames(index, req.fps)?,
                size: req.size,
                quality: req.quality,
                nodes: req.nodes,
            };
            let id = self.draw_frame(&frame_req)?;
            let (y, uv) = self.encode_nv12(id, req.size)?;
            self.gpu.release_id(id);
            in_flight.push_back(InFlight {
                index,
                size: req.size,
                y,
                uv,
            });
            if in_flight.len() > READBACK_AHEAD {
                self.flush_one(&mut in_flight, encoder, sink)?;
                count += 1;
            }
        }
        while !in_flight.is_empty() {
            self.flush_one(&mut in_flight, encoder, sink)?;
            count += 1;
        }
        Ok(count)
    }

    fn encode_nv12(&mut self, id: u64, size: Size) -> Result<(Readback, Readback)> {
        let ctx: Arc<GpuContext> = self.gpu.context().clone();
        let pipe = match self.export.as_mut() {
            Some(p) if p.size == size => p,
            _ => self.export.insert(ExportPipe::new(&ctx, size)?),
        };
        let frame = self
            .gpu
            .texture(id)
            .filter(|t| t.format() == FRAME_FORMAT)
            .ok_or_else(|| MmError::NotFound(format!("テクスチャ {id}")))?
            .clone();
        Ok(pipe.encode(&mut self.gpu, &frame))
    }

    fn flush_one(
        &mut self,
        in_flight: &mut VecDeque<InFlight>,
        encoder: &mut dyn Encoder,
        sink: &mut dyn FnMut(i64),
    ) -> Result<()> {
        let Some(InFlight { index, size, y, uv }) = in_flight.pop_front() else {
            return Ok(());
        };
        let y = self.gpu.finish_readback(y)?;
        let uv = self.gpu.finish_readback(uv)?;
        encoder.push_nv12_frames(&[Nv12Frame::new(size, y, uv)?])?;
        sink(index);
        Ok(())
    }
}
