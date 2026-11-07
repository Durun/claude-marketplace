//! CEF の画素（BGRA8、premultiplied、sRGB の符号化）を中間の形式（`Rgba16Float`、premultiplied、線形）に直すパス。
//! 半透明の画素は alpha で割ってから線形にし、掛け直す（mm-host-cef の `pixel` と同じ式で、CPU の経路と画素が合う）。

use std::sync::{Arc, Weak};

use mm_gpu::{GpuContext, FRAME_FORMAT};

const SHADER: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(pos.xy), 0);
    if (c.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let straight = min(c.rgb / c.a, vec3<f32>(1.0));
    return vec4<f32>(decode(straight) * c.a, c.a);
}
";

/// 変換のパイプライン。作ったデバイスが作り直されたら作り直す。
pub struct Converter {
    ctx: Weak<GpuContext>,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl Converter {
    /// `ctx` の上に作る。
    pub fn new(ctx: &Arc<GpuContext>) -> Self {
        let device = &ctx.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("web.cef convert"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("web.cef convert"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("web.cef convert"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("web.cef convert"),
            layout: Some(&pipeline_layout),
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
            ctx: Arc::downgrade(ctx),
            pipeline,
            layout,
        }
    }

    /// `ctx` の上で作ったものか。
    pub fn is_for(&self, ctx: &Arc<GpuContext>) -> bool {
        self.ctx.upgrade().is_some_and(|c| Arc::ptr_eq(&c, ctx))
    }

    /// 共有テクスチャ `shared` を、自前の BGRA8 にコピーしてから中間の形式の新しいテクスチャへ変換する。
    /// コピーの後に共有テクスチャの状態を `PRESENT` に戻す（ホストとの約束の COMMON）。提出して返す。
    pub fn convert(&self, ctx: &GpuContext, shared: &wgpu::Texture) -> wgpu::Texture {
        let size = shared.size();
        let staging = texture(
            ctx,
            "web.cef staging",
            size,
            shared.format(),
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let out = texture(
            ctx,
            "web.cef frame",
            size,
            FRAME_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(shared.as_image_copy(), staging.as_image_copy(), size);
        encoder.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture: shared,
                selector: None,
                state: wgpu::TextureUses::PRESENT,
            }),
        );
        self.draw(ctx, &mut encoder, &staging, &out);
        ctx.queue.submit([encoder.finish()]);
        out
    }

    fn draw(
        &self,
        ctx: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Texture,
        dst: &wgpu::Texture,
    ) {
        let src_view = src.create_view(&Default::default());
        let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("web.cef convert"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&src_view),
            }],
        });
        let dst_view = dst.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("web.cef convert"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dst_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn texture(
    ctx: &GpuContext,
    label: &str,
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// キャッシュしたテクスチャを、ヘルパーの持つテクスチャ `dst` へコピーして提出する。
pub fn copy_into(ctx: &GpuContext, src: &wgpu::Texture, dst: &wgpu::Texture) {
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_texture(src.as_image_copy(), dst.as_image_copy(), src.size());
    ctx.queue.submit([encoder.finish()]);
}
