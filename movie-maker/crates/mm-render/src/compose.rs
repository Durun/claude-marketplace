//! レイヤーの合成。下から順に、変形した四角形を premultiplied の source-over で重ねる。

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use mm_gpu::{GpuContext, FRAME_FORMAT};

use crate::clip::{Clip, MaskUniform, MAX_MASKS};

/// 重ねる 1 枚。
pub struct DrawItem<'a> {
    pub texture: &'a wgpu::Texture,
    /// 画像の UV から出力の NDC への列優先の行列。
    pub matrix: [f32; 16],
    pub opacity: f32,
    /// 出力の 1 画素あたりの元の画素の数（軸ごと）。1 を超えると面積平均で縮小する。
    pub taps: [f32; 2],
    /// 切り抜きとマスク。
    pub clip: Clip,
}

/// シェーダーの `Item` と同じフィールド順（240 バイト）。
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ItemUniform {
    matrix: [f32; 16],
    opacity: f32,
    mask_count: u32,
    taps: [f32; 2],
    place: [f32; 4],
    crop: [f32; 4],
    masks: [MaskUniform; MAX_MASKS],
}

/// 合成のパイプライン。
pub struct Compositor {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

const OVER: wgpu::BlendComponent = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::One,
    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
    operation: wgpu::BlendOperation::Add,
};

impl Compositor {
    /// パイプラインを作る。
    pub fn new(ctx: &GpuContext) -> Self {
        let device = &ctx.device;
        let layout = bind_group_layout(device);
        let pipeline = create_pipeline(device, &layout);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mm-render layer sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    /// `target` を透明に塗ってから `items` を順に重ねるパスを `encoder` に積む。
    pub fn draw(
        &self,
        ctx: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        items: &[DrawItem<'_>],
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let bind_groups: Vec<wgpu::BindGroup> = items
            .iter()
            .map(|item| self.bind_group(ctx, item))
            .collect();
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render.composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        for group in &bind_groups {
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    fn bind_group(&self, ctx: &GpuContext, item: &DrawItem<'_>) -> wgpu::BindGroup {
        let uniform = ItemUniform {
            matrix: item.matrix,
            opacity: item.opacity,
            mask_count: item.clip.mask_count,
            taps: item.taps,
            place: item.clip.place,
            crop: item.clip.crop,
            masks: item.clip.masks,
        };
        let buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mm-render layer uniform"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let view = item
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mm-render layer"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// 0 が 1 枚分の uniform、1 が画像、2 が標本化。
fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mm-render compose layout"),
        entries: &[
            entry(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            entry(
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(
                2,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
        ],
    })
}

fn create_pipeline(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mm-render compose"),
        source: wgpu::ShaderSource::Wgsl(include_str!("compose.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mm-render compose pipeline layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mm-render compose"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FRAME_FORMAT,
                blend: Some(wgpu::BlendState {
                    color: OVER,
                    alpha: OVER,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
