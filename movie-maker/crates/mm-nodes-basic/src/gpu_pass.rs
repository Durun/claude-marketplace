//! 全画面の三角形で 1 枚の画像を描くパス。色の種類と `shader.wgsl` が共有する。
//!
//! 種類は `FrameCx::gpu` の `as_any` で `WgpuGpu` に降り、自前のコマンドを作ってすぐ提出する。
//! 入力のテクスチャは提出の時点で中身が揃っている必要があるので、レンダラーは入力を渡す前に自分の分を提出する。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use mm_gpu::{GpuContext, WgpuGpu, FRAME_FORMAT};
use mm_node_api::{FrameRef, GpuHelper};
use mm_types::{MmError, Result, Size};
use wgpu::util::DeviceExt;

/// 頂点シェーダー。画面を覆う 1 枚の三角形で、`uv` は左上が (0, 0)。
const VERTEX_WGSL: &str = r"
struct MmVsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn mm_vs(@builtin(vertex_index) i: u32) -> MmVsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: MmVsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}
";

/// キャッシュするパイプラインの数の上限。`shader.wgsl` が毎フレーム違うコードを渡しても増え続けない。
const MAX_PIPELINES: usize = 32;

/// 1 パスの入力。
pub(crate) struct Pass<'a> {
    /// `struct U`（binding 0 の uniform の型）と `mm_fs`（フラグメントシェーダー）を含む WGSL。
    pub body: &'a str,
    /// uniform の中身。大きさは 16 の倍数。
    pub uniform: &'a [u8],
    /// 入力の 2D テクスチャの番号。`mm_t0`、`mm_t1` … にバインドする。
    pub inputs: &'a [u64],
    /// 3D LUT。あれば `mm_lut` にバインドする。
    pub lut: Option<&'a wgpu::Texture>,
    /// 出力の大きさ。
    pub size: Size,
}

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

struct Entries {
    /// 作った時のコンテキスト。作り直されたら全部捨てる。
    ctx: Weak<GpuContext>,
    map: HashMap<String, std::result::Result<Arc<Pipeline>, String>>,
}

/// パイプラインのキャッシュ。WGSL の全文で引き、コンパイルの失敗も覚える（毎フレーム作り直さない）。
#[derive(Default)]
pub(crate) struct PassCache {
    entries: Mutex<Option<Entries>>,
}

impl std::fmt::Debug for PassCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PassCache")
    }
}

/// WGSL のバインディングの宣言。0 が uniform、1.. が入力、その次がサンプラー、最後が LUT。
fn bindings_wgsl(inputs: usize, lut: bool) -> String {
    let mut s = String::from("@group(0) @binding(0) var<uniform> u: U;\n");
    for i in 0..inputs {
        s += &format!(
            "@group(0) @binding({}) var mm_t{i}: texture_2d<f32>;\n",
            i + 1
        );
    }
    s += &format!("@group(0) @binding({}) var mm_samp: sampler;\n", inputs + 1);
    if lut {
        s += &format!(
            "@group(0) @binding({}) var mm_lut: texture_3d<f32>;\n",
            inputs + 2
        );
    }
    s
}

fn layout_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    }
}

fn texture_entry(
    binding: u32,
    dimension: wgpu::TextureViewDimension,
    filterable: bool,
) -> wgpu::BindGroupLayoutEntry {
    layout_entry(
        binding,
        wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: dimension,
            multisampled: false,
        },
    )
}

fn layout_entries(inputs: usize, lut: bool) -> Vec<wgpu::BindGroupLayoutEntry> {
    let mut entries = vec![layout_entry(
        0,
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    )];
    for i in 0..inputs as u32 {
        entries.push(texture_entry(i + 1, wgpu::TextureViewDimension::D2, true));
    }
    let next = inputs as u32 + 1;
    entries.push(layout_entry(
        next,
        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
    ));
    if lut {
        entries.push(texture_entry(
            next + 1,
            wgpu::TextureViewDimension::D3,
            false,
        ));
    }
    entries
}

/// WGSL を組み、検証つきでパイプラインを作る。失敗は検証のメッセージ。
fn build(
    ctx: &GpuContext,
    source: &str,
    inputs: usize,
    lut: bool,
) -> std::result::Result<Pipeline, String> {
    let device = &ctx.device;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mm-nodes-basic pass"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mm-nodes-basic pass layout"),
        entries: &layout_entries(inputs, lut),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mm-nodes-basic pass pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mm-nodes-basic pass"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("mm_vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("mm_fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FRAME_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(error.to_string());
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("mm-nodes-basic pass sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    Ok(Pipeline {
        pipeline,
        layout,
        sampler,
    })
}

impl PassCache {
    fn pipeline(&self, ctx: &Arc<GpuContext>, pass: &Pass<'_>) -> Result<Arc<Pipeline>> {
        let source = format!(
            "{}\n{VERTEX_WGSL}\n{}",
            bindings_wgsl(pass.inputs.len(), pass.lut.is_some()),
            pass.body
        );
        let mut guard = self
            .entries
            .lock()
            .map_err(|_| MmError::Internal("パイプラインのキャッシュが壊れた".into()))?;
        let current = Arc::downgrade(ctx);
        let entries = guard.get_or_insert_with(|| Entries {
            ctx: current.clone(),
            map: HashMap::new(),
        });
        if !Weak::ptr_eq(&entries.ctx, &current) || entries.map.len() >= MAX_PIPELINES {
            entries.ctx = current;
            entries.map.clear();
        }
        let key = format!("{}:{}\n{source}", pass.inputs.len(), pass.lut.is_some());
        let built = entries.map.entry(key).or_insert_with(|| {
            build(ctx, &source, pass.inputs.len(), pass.lut.is_some()).map(Arc::new)
        });
        built.clone().map_err(|message| {
            MmError::InvalidArgument(format!("WGSL のコンパイルに失敗: {message}"))
        })
    }

    /// パスを描いて提出し、出力のテクスチャの番号を返す。コンパイルの失敗は `Err(InvalidArgument)`。
    pub(crate) fn run(&self, gpu: &mut WgpuGpu, pass: &Pass<'_>) -> Result<u64> {
        let ctx = gpu.context().clone();
        let pipeline = self.pipeline(&ctx, pass)?;
        let out = gpu.create(pass.size)?;
        let result = encode(&ctx, gpu, &pipeline, pass, out);
        if result.is_err() {
            gpu.release_id(out);
        }
        result.map(|()| out)
    }
}

fn encode(
    ctx: &GpuContext,
    gpu: &WgpuGpu,
    pipeline: &Pipeline,
    pass: &Pass<'_>,
    out: u64,
) -> Result<()> {
    let device = &ctx.device;
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mm-nodes-basic pass uniform"),
        contents: pass.uniform,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let views = pass
        .inputs
        .iter()
        .map(|id| {
            gpu.texture(*id)
                .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
                .ok_or_else(|| MmError::NotFound(format!("入力のテクスチャ {id}")))
        })
        .collect::<Result<Vec<_>>>()?;
    let lut_view = pass
        .lut
        .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: uniform.as_entire_binding(),
    }];
    for (i, view) in views.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: i as u32 + 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    let sampler_binding = views.len() as u32 + 1;
    entries.push(wgpu::BindGroupEntry {
        binding: sampler_binding,
        resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
    });
    if let Some(view) = &lut_view {
        entries.push(wgpu::BindGroupEntry {
            binding: sampler_binding + 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mm-nodes-basic pass"),
        layout: &pipeline.layout,
        entries: &entries,
    });
    let target = gpu
        .texture(out)
        .ok_or_else(|| MmError::Internal("作ったばかりの出力が無い".into()))?
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("mm-nodes-basic pass"),
    });
    {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mm-nodes-basic pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            timestamp_writes: gpu.timer().pass_writes(),
            ..Default::default()
        });
        rp.set_pipeline(&pipeline.pipeline);
        rp.set_bind_group(0, &group, &[]);
        rp.draw(0..3, 0..1);
    }
    // 借用しているデコーダーのフレームが入力なら、その完了を待たせてから提出する。
    gpu.before_submit();
    ctx.queue.submit([encoder.finish()]);
    Ok(())
}

/// パスの入力にするテクスチャ。`made` が真なら、この呼び出しで作ったので使い終えたら解放する。
pub(crate) struct InputTexture {
    pub id: u64,
    pub size: Size,
    pub made: bool,
}

/// フレームをパスの入力のテクスチャにする。CPU の画像は上げ、透明は透明のテクスチャを確保する。
pub(crate) fn input_texture(gpu: &mut WgpuGpu, frame: FrameRef) -> Result<InputTexture> {
    let (frame, made) = match frame {
        FrameRef::Transparent { size } => (GpuHelper::allocate(gpu, size)?, true),
        FrameRef::Cpu(image) => (GpuHelper::upload(gpu, &image)?, true),
        gpu_frame @ FrameRef::Gpu(_) => (gpu_frame, false),
    };
    match frame {
        FrameRef::Gpu(tex) => Ok(InputTexture {
            id: tex.id,
            size: tex.size,
            made,
        }),
        _ => Err(MmError::Internal("GPU のテクスチャにならなかった".into())),
    }
}
