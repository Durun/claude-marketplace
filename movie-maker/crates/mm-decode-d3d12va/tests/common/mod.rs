//! GPU のデコーダーの統合テストが共有する補助（cuvid のテストも `#[path]` で取り込む）。
//!
//! GPU のコマは `ReadbackFactory` で CPU の絵に読み戻して、`mm_testkit` の契約（CPU の絵を前提に
//! 同じコマの一致を比べる）に通す。読み戻しは描画器と同じ手順（`import`、`before_submit`、`after_submit`）。

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, GpuContextRef, MediaInfo};
use mm_gpu::{GpuContext, GpuRequest, WgpuGpu};
use mm_node_api::GpuHelper;
use mm_types::{RationalTime, Result, RgbaImage, TimeRange};

/// 番号の箱の数と、1 つの箱の一辺（画素）。
pub const BIT_COUNT: u32 = 16;
pub const BIT_SIZE: u32 = 16;

fn media_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/media")
}

/// 生成済みの見本の素材のディレクトリ。`need` が無ければ `generate.sh` で作る。
pub fn fixtures(need: &[&str]) -> PathBuf {
    let root = media_root();
    let out = root.join("out");
    let lock = root.join(".generate.lock");
    let deadline = Instant::now() + Duration::from_secs(180);
    let missing = || need.iter().any(|n| !out.join(n).exists());
    while missing() {
        if OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .is_ok()
        {
            let result = run_generate(&root);
            let _ = std::fs::remove_file(&lock);
            result.expect("見本の素材を作れた");
            break;
        } else if Instant::now() > deadline {
            panic!(
                "見本の素材の生成を待ち切れない（{} が残っていないか）",
                lock.display()
            );
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    out
}

fn run_generate(root: &Path) -> std::result::Result<(), String> {
    let script = root.join("generate.sh");
    for sh in [
        "sh",
        "C:/Program Files/Git/bin/sh.exe",
        "C:/Program Files/Git/usr/bin/sh.exe",
    ] {
        match Command::new(sh).arg(&script).status() {
            Ok(s) if s.success() => return Ok(()),
            Ok(s) => return Err(format!("{sh} {}: {s}", script.display())),
            Err(_) => continue,
        }
    }
    Err(format!(
        "sh が見つからない。先に {} を実行する",
        script.display()
    ))
}

/// 描画器と同じ文脈。`request` はデコーダーの `gpu_request`。プロセスで 1 つを共有する。
pub fn gpu(request: GpuRequest) -> Arc<GpuContext> {
    GpuContext::shared(&request).expect("GPU の文脈")
}

/// デコードしたコマから焼いた番号を読む。`scale` は絵を整数倍に拡大した倍率。
pub fn read_frame_number(image: &RgbaImage, scale: u32) -> i64 {
    (0..BIT_COUNT)
        .map(|k| {
            let x = (k * BIT_SIZE + BIT_SIZE / 2) * scale;
            let y = (BIT_SIZE / 2) * scale;
            ((image.pixel(x, y).r > 0.5) as i64) << k
        })
        .sum()
}

/// GPU のデコーダーを、CPU の絵を返すデコーダーとして見せる口。
pub struct ReadbackFactory {
    pub inner: Arc<dyn DecoderFactory>,
    pub ctx: Arc<GpuContext>,
}

impl ReadbackFactory {
    pub fn new(inner: impl DecoderFactory + 'static, ctx: &Arc<GpuContext>) -> Self {
        Self {
            inner: Arc::new(inner),
            ctx: ctx.clone(),
        }
    }
}

impl DecoderFactory for ReadbackFactory {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    /// 渡された `gpu` を無視して、いつも組み立て済みの文脈で開く（契約は `None` で開く）。
    fn open(&self, path: &Path, _gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        let inner = self
            .inner
            .open(path, Some(self.ctx.as_ref() as &dyn GpuContextRef))?;
        Ok(Box::new(ReadbackDecoder {
            inner,
            reader: FrameReader::new(&self.ctx),
        }))
    }
}

/// GPU のコマを、描画器と同じ手順で CPU の絵に読み戻す。
pub struct FrameReader {
    helper: WgpuGpu,
}

impl FrameReader {
    pub fn new(ctx: &Arc<GpuContext>) -> Self {
        Self {
            helper: WgpuGpu::new(ctx.clone()),
        }
    }

    /// GPU のコマを CPU の絵にする。
    pub fn read(&mut self, f: &DecodedFrame) -> RgbaImage {
        let handle = f.gpu.as_ref().expect("GPU のコマ");
        let frame = self.helper.import(handle).expect("import");
        self.helper.before_submit();
        let image = self.helper.download(&frame).expect("読み戻し");
        self.helper.after_submit();
        (*image).clone()
    }
}

/// GPU のコマを CPU の絵にして返すデコーダー。
pub struct ReadbackDecoder {
    inner: Box<dyn Decoder>,
    reader: FrameReader,
}

impl Decoder for ReadbackDecoder {
    fn info(&self) -> &MediaInfo {
        self.inner.info()
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.inner.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let Some(f) = self.inner.next_frame()? else {
            return Ok(None);
        };
        assert!(f.image.is_none() && f.gpu.is_some(), "GPU のコマで返る");
        let image = self.reader.read(&f);
        Ok(Some(DecodedFrame::cpu(f.index, f.pts, image)))
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        self.inner.decode_audio(range)
    }
}

const BUSY_SHADER: &str = r"
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment
fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    var a = p.x * 0.001;
    for (var i = 0u; i < 3000u; i = i + 1u) { a = sin(a) * 1.0001 + 0.37; }
    return vec4<f32>(a, a, a, 1.0);
}
";

/// GPU のキューを `target` ほど塞ぐ。提出するだけで待たない（後続の提出が遅れる状況を作る）。
pub fn busy_gpu(ctx: &GpuContext, target: Duration) {
    let device = &ctx.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("busy"),
        source: wgpu::ShaderSource::Wgsl(BUSY_SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("busy"),
        layout: None,
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
            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let target_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("busy target"),
        size: wgpu::Extent3d {
            width: 4096,
            height: 4096,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let submit = |passes: u32| {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        for _ in 0..passes {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
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
            pass.set_pipeline(&pipeline);
            pass.draw(0..3, 0..1);
        }
        ctx.queue.submit([enc.finish()]);
    };
    let started = Instant::now();
    submit(1);
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let one = started.elapsed().max(Duration::from_millis(1));
    submit((target.as_secs_f64() / one.as_secs_f64()).ceil() as u32);
}

/// 8 bit に落ちていないかの目安として、行の中の画素の値の種類の数。
pub fn distinct_in_row(image: &RgbaImage, y: u32) -> usize {
    let mut values: Vec<u32> = (0..image.width())
        .map(|x| image.pixel(x, y).g.to_bits())
        .collect();
    values.sort_unstable();
    values.dedup();
    values.len()
}
