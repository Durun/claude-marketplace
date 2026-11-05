//! WGSL の `mm_color_transform` を wgpu の compute で実行する試験の道具。
//! GPU の値を CPU の値と突き合わせるために使う。アダプターが無い環境では [`GpuEval::new`] が None を返す。

use wgpu::util::DeviceExt;

/// 入力の画素に `mm_color_transform` を当てる compute の枠。bind group 0 は枠が使う。
const HARNESS: &str = r"
@group(0) @binding(0) var<storage, read> mm_in: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> mm_out: array<vec4<f32>>;

@compute @workgroup_size(64)
fn mm_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&mm_in) { return; }
    let c = mm_in[id.x];
    mm_out[id.x] = vec4<f32>(mm_color_transform(c.xyz), c.w);
}
";

/// GPU の実行環境。
pub struct GpuEval {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// アダプターの名前（報告用）。
    pub adapter_name: String,
}

impl GpuEval {
    /// アダプターを探す。無ければ None。
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some(Self {
            device,
            queue,
            adapter_name: adapter.get_info().name,
        })
    }

    /// `wgsl`（`mm_color_transform` を含む）を `inputs` に当てる。
    /// `lut` は `LUT3D_APPLY_WGSL` を使うときの `(size, size^3*3 の f32)`。
    pub fn run(
        &self,
        wgsl: &str,
        inputs: &[[f32; 3]],
        lut: Option<(u32, &[f32])>,
    ) -> Result<Vec<[f32; 3]>, String> {
        let padded: Vec<u8> = inputs
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 1.0])
            .flat_map(f32::to_le_bytes)
            .collect();
        let size = padded.len() as u64;
        let device = &self.device;

        let source = format!("{HARNESS}\n{wgsl}");
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mm-color"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("mm-color"),
            layout: None,
            module: &module,
            entry_point: Some("mm_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(format!("{err}"));
        }

        let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &padded,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let lut_view = lut.map(|(n, data)| self.lut_texture(n, data));
        let group1 = lut_view.as_ref().map(|view| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(1),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                }],
            })
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group0, &[]);
            if let Some(g) = &group1 {
                pass.set_bind_group(1, g, &[]);
            }
            let groups = u32::try_from(inputs.len().div_ceil(64)).map_err(|e| e.to_string())?;
            pass.dispatch_workgroups(groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bytes = slice.get_mapped_range().map_err(|e| e.to_string())?;
        let floats: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(floats.chunks_exact(4).map(|c| [c[0], c[1], c[2]]).collect())
    }

    fn lut_texture(&self, n: u32, data: &[f32]) -> wgpu::TextureView {
        let rgba: Vec<u8> = data
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 1.0])
            .flat_map(f32::to_le_bytes)
            .collect();
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("mm-lut"),
                size: wgpu::Extent3d {
                    width: n,
                    height: n,
                    depth_or_array_layers: n,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &rgba,
        );
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }
}
