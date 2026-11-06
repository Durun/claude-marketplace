//! YUV から `Rgba16Float` への変換の描画パス。全画面の三角形 1 枚を、呼び出し側のフラグメントで塗る。
//! d3d12va（2 プレーンのテクスチャ）と cuvid（共有バッファ）は、読む資源の束縛とフラグメントだけが違う。

use mm_gpu::FRAME_FORMAT;

use crate::yuv::{YuvParams, PARAMS_BYTES, YUV_WGSL};

/// フラグメントシェーダーからの束縛のフィールド。
pub fn fragment_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    }
}

/// 一様バッファの束縛の型。
pub fn uniform_binding() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

/// 読み取り専用のストレージバッファの束縛の型。
pub fn storage_binding() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

/// 補間しない 2D のテクスチャの束縛の型。
pub fn texture_binding() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: false },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

/// 変換の描画パイプラインと、その束縛の並び。
pub struct YuvPass {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
}

impl YuvPass {
    /// `fragment` は共通部（`YUV_WGSL`）の後ろに足すフラグメント。エントリーポイントは `fs`。
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        fragment: &str,
        entries: &[wgpu::BindGroupLayoutEntry],
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(format!("{YUV_WGSL}{fragment}").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries,
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
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
        Self { pipeline, layout }
    }

    /// `bind` を読んで `target` を塗り、キューに出す。
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind: &wgpu::BindGroup,
        target: &wgpu::Texture,
    ) -> wgpu::SubmissionIndex {
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
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
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()])
    }
}

/// 変換のパラメーターの一様バッファ。値が変わったときだけ書き込む。
pub struct ParamsBuffer {
    buffer: wgpu::Buffer,
    last: Option<YuvParams>,
}

impl ParamsBuffer {
    /// 空のバッファを作る。
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yuv params"),
                size: PARAMS_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            last: None,
        }
    }

    /// 束縛に渡すバッファ。
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// `params` が前回と違えば書き込む。
    pub fn update(&mut self, queue: &wgpu::Queue, params: YuvParams) {
        if self.last != Some(params) {
            queue.write_buffer(&self.buffer, 0, &params.to_bytes());
            self.last = Some(params);
        }
    }
}
