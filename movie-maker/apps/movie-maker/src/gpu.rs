//! サーフェスを 60 fps で描き続ける描画スレッド。UI スレッドの外で回し、WebView の更新と独立に present する。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tracing::Span;
use wgpu::util::DeviceExt;

use crate::shared::Shared;

/// 描く間隔（60 fps）。
const FRAME: Duration = Duration::from_nanos(16_666_667);

const SHADER: &str = r#"
struct U { res: vec2<f32>, t: f32, flash: f32 };
@group(0) @binding(0) var<uniform> u: U;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = pos.xy / u.res;
  var c = mix(vec3<f32>(0.06, 0.08, 0.13), vec3<f32>(0.19, 0.42, 0.77), uv.y * 0.6);
  let grid = step(0.985, fract(uv.x * 16.0)) + step(0.985, fract(uv.y * 9.0));
  c += vec3<f32>(0.06) * grid;
  let x = fract(u.t / 10.0);
  let bar = step(abs(uv.x - x) * u.res.x, 3.0);
  c = mix(c, vec3<f32>(1.0, 0.85, 0.2), bar);
  let ball = vec2<f32>(0.5 + 0.35 * sin(u.t * 2.0), 0.5 + 0.25 * cos(u.t * 3.1));
  let d = length((uv - ball) * vec2<f32>(u.res.x / u.res.y, 1.0));
  c = mix(c, vec3<f32>(0.95, 0.4, 0.3), 1.0 - smoothstep(0.05, 0.06, d));
  return vec4<f32>(mix(c, vec3<f32>(1.0), u.flash * 0.35), 1.0);
}
"#;

/// 描画スレッドのハンドル。
pub struct Renderer {
    quit: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Renderer {
    /// 子 HWND にサーフェスを張って描画スレッドを始める。
    pub fn spawn(shared: Arc<Shared>, hwnd: isize, resize_span: Arc<Mutex<Option<Span>>>) -> Self {
        let quit = Arc::new(AtomicBool::new(false));
        let q = quit.clone();
        let join = std::thread::Builder::new()
            .name("render".into())
            .spawn(move || {
                if let Err(e) = run(&shared, hwnd, &q, &resize_span) {
                    tracing::error!(error = %e, "描画スレッドが終わった");
                }
            })
            .expect("render thread");
        Self {
            quit,
            join: Some(join),
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run(
    shared: &Shared,
    hwnd: isize,
    quit: &AtomicBool,
    resize_span: &Mutex<Option<Span>>,
) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = create_surface(&instance, hwnd)?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| e.to_string())?;
    let info = adapter.get_info();
    *shared.adapter.lock().expect("adapter") = format!("{} ({:?})", info.name, info.backend);
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| e.to_string())?;

    let caps = surface.get_capabilities(&adapter);
    let format = caps.formats[0];
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: 1,
        height: 1,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        color_space: wgpu::SurfaceColorSpace::Auto,
    };

    let ubo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("u"),
        contents: &[0u8; 16],
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spike"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: ubo.as_entire_binding(),
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
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
    });

    let mut configured = (0u32, 0u32);
    let mut deadline = Instant::now();
    let mut last_start: Option<Instant> = None;
    while !quit.load(Ordering::SeqCst) {
        deadline += FRAME;
        // 長く止まった後に取り戻そうと連続で描かないよう、遅れすぎた締め切りは今に合わせる。
        if deadline + FRAME < Instant::now() {
            deadline = Instant::now();
        }
        pace(deadline);
        let (w, h) = shared.surface_size();
        if w == 0 || h == 0 {
            continue;
        }
        if (w, h) != configured {
            config.width = w;
            config.height = h;
            surface.configure(&device, &config);
            configured = (w, h);
        }

        let start = Instant::now();
        if let Some(prev) = last_start {
            shared.record("frame_interval_ms", (start - prev).as_secs_f64() * 1000.0);
        }
        last_start = Some(start);
        let _span = tracing::info_span!("window.frame").entered();

        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                // 大きさの変更の直後に Outdated が返るのは普通なので、数えて張り直す。
                shared.surface_errors.fetch_add(1, Ordering::SeqCst);
                tracing::warn!(state = ?other, "サーフェスを取れなかった");
                surface.configure(&device, &config);
                continue;
            }
        };

        let acquired = Instant::now();
        let t = shared.t0.elapsed().as_secs_f32();
        let flash_age = shared
            .now_us()
            .saturating_sub(shared.flash_us.load(Ordering::SeqCst));
        let flash = if shared.flash_us.load(Ordering::SeqCst) != 0 && flash_age < 150_000 {
            1.0f32
        } else {
            0.0
        };
        let mut bytes = Vec::with_capacity(16);
        for v in [w as f32, h as f32, t, flash] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        queue.write_buffer(&ubo, 0, &bytes);

        let view = frame.texture.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        {
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
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        let submitted = Instant::now();
        queue.present(frame);

        shared.frames.fetch_add(1, Ordering::SeqCst);
        shared.record(
            "frame_acquire_ms",
            (acquired - start).as_secs_f64() * 1000.0,
        );
        shared.record(
            "frame_encode_submit_ms",
            (submitted - acquired).as_secs_f64() * 1000.0,
        );
        shared.record(
            "frame_present_ms",
            submitted.elapsed().as_secs_f64() * 1000.0,
        );
        let stamp = shared.layout_us.swap(0, Ordering::SeqCst);
        if stamp != 0 {
            shared.record(
                "resize_to_present_ms",
                shared.now_us().saturating_sub(stamp) as f64 / 1000.0,
            );
            // 配置を変えたところで開いた span を、新しい大きさの最初の present で閉じる。
            drop(resize_span.lock().expect("resize span").take());
        }
    }
    Ok(())
}

/// 締め切りまで待つ。最後の 1 ms は spin して、sleep の粒度のぶれを避ける。
fn pace(deadline: Instant) {
    let now = Instant::now();
    if deadline > now + Duration::from_millis(2) {
        std::thread::sleep(deadline - now - Duration::from_millis(1));
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

fn create_surface(
    instance: &wgpu::Instance,
    hwnd: isize,
) -> Result<wgpu::Surface<'static>, String> {
    use std::num::NonZeroIsize;
    let handle = wgpu::rwh::Win32WindowHandle::new(NonZeroIsize::new(hwnd).ok_or("hwnd が 0")?);
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(wgpu::rwh::RawDisplayHandle::Windows(
            wgpu::rwh::WindowsDisplayHandle::new(),
        )),
        raw_window_handle: wgpu::rwh::RawWindowHandle::Win32(handle),
    };
    // HWND は Tauri のウィンドウが閉じるまで生きていて、描画スレッドは先に止めるので、'static に延ばしてよい。
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
