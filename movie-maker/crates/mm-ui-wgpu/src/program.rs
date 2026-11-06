//! Program のビューアー。エンジンが返した線形の画像（`RgbaImage`）を Rgba16Float で持ち、
//! 書き出しと同じ Rec.1886（べき 2.4）の表示の式で画面に出す。

use half::f16;
use mm_types::RgbaImage;
use mm_ui_layout::Rect;

const SHADER: &str = r#"
struct U { rect: vec4<f32>, res: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> Out {
  var corners = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
  let c = corners[vi];
  let p = u.rect.xy + c * u.rect.zw;
  var o: Out;
  o.pos = vec4<f32>(p.x / u.res.x * 2.0 - 1.0, 1.0 - p.y / u.res.y * 2.0, 0.0, 1.0);
  o.uv = c;
  return o;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
  let c = textureSample(tex, samp, in.uv);
  // 線形の光を Rec.1886 の表示の値にする。premultiplied のまま黒の上に置く。
  let rgb = pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4));
  return vec4<f32>(rgb, 1.0);
}
"#;

/// テクスチャの 1 画素のバイト数（Rgba16Float）。
const BYTES_PER_PIXEL: u32 = 8;

struct Gpu {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    size: (u32, u32),
    /// ビューアーが作ったテクスチャか。偽なら借用したもので、画像を書き込まない。
    owned: bool,
}

/// Program のビューア。
pub struct ProgramViewer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    gpu: Option<Gpu>,
}

impl ProgramViewer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bgl = bind_group_layout(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui program uniform"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline: pipeline(device, format, &bgl),
            bgl,
            sampler,
            uniform,
            gpu: None,
        }
    }

    /// 画像があるか（最初のフレームが来るまでは無い）。
    pub fn has_image(&self) -> bool {
        self.gpu.is_some()
    }

    /// 画像の大きさ。
    pub fn image_size(&self) -> Option<(u32, u32)> {
        self.gpu.as_ref().map(|g| g.size)
    }

    /// 新しいフレームを上げる。大きさが変わったらテクスチャを作り直す。
    pub fn set_image(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &RgbaImage) {
        let size = (image.width(), image.height());
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        if self.gpu.as_ref().filter(|g| g.owned).map(|g| g.size) != Some(size) {
            self.gpu = Some(self.create(device, size));
        }
        let Some(gpu) = &self.gpu else { return };
        let texels: Vec<u16> = image
            .data()
            .iter()
            .map(|v| f16::from_f32(*v).to_bits())
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * BYTES_PER_PIXEL),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// 同じデバイスで描いたテクスチャ（Rgba16Float、線形の premultiplied）をそのまま出す。CPU を通さない。
    /// テクスチャは次に `set_texture` か `set_image` を呼ぶまで持つ。
    pub fn set_texture(&mut self, device: &wgpu::Device, texture: &wgpu::Texture) {
        let size = (texture.width(), texture.height());
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        let bind = self.bind(device, texture);
        self.gpu = Some(Gpu {
            texture: texture.clone(),
            bind,
            size,
            owned: false,
        });
    }

    fn bind(&self, device: &wgpu::Device, texture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui program"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
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

    fn create(&self, device: &wgpu::Device, size: (u32, u32)) -> Gpu {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui program frame"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bind = self.bind(device, &texture);
        Gpu {
            texture,
            bind,
            size,
            owned: true,
        }
    }

    /// 描く位置を転送する。パスの外（`submit` の前）で呼ぶ。
    pub fn prepare(&self, queue: &wgpu::Queue, rect: Rect, viewport: (f32, f32)) {
        let u = [
            rect.x, rect.y, rect.w, rect.h, viewport.0, viewport.1, 0.0, 0.0,
        ];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&u));
    }

    /// `prepare` で渡した矩形に描く。画像が無ければ何もしない。
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(gpu) = &self.gpu else { return };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &gpu.bind, &[]);
        pass.draw(0..6, 0..1);
    }
}

fn uniform_ty() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    }
}

fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ui program"),
        entries: &[
            entry(0, wgpu::ShaderStages::VERTEX, uniform_ty()),
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

fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bgl: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ui program"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ui program"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ui program"),
        layout: Some(&layout),
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
            targets: &[Some(format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
