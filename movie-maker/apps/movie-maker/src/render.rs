//! サーフェスに描き続ける描画スレッド。UI スレッドの外で回し、WebView の更新と独立に present する。
//!
//! ペースは Fifo の垂直同期に任せる（自前の sleep や spin は持たない）。再生位置は時計を直接読み、
//! フレームは再生スレッドが置いた最新の 1 枚を取る。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mm_ui_layout::{Action, TimelineModel};
use mm_ui_wgpu::Ui;

use crate::playback::Command;
use crate::state::Shared;
use crate::surface;

/// サーフェスを取れなかったときの待ち。再試行で CPU を回さないための間隔。
const RETRY_WAIT: Duration = Duration::from_millis(8);

/// 描画スレッドのハンドル。落とすと止めて待つ。
pub struct Renderer {
    quit: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Renderer {
    /// 子 HWND にサーフェスを張って描画スレッドを始める。
    pub fn spawn(shared: Arc<Shared>, hwnd: isize, model: TimelineModel) -> Self {
        let quit = Arc::new(AtomicBool::new(false));
        let q = quit.clone();
        let join = std::thread::Builder::new()
            .name("render".into())
            .spawn(move || {
                if let Err(e) = run(&shared, hwnd, &q, model) {
                    tracing::error!(error = %e, "描画スレッドが終わった");
                }
            })
            .expect("render thread");
        Self {
            quit,
            join: Some(join),
        }
    }

    /// 止めて待つ。HWND が破棄される前に呼ぶ。2 回目以降は何もしない。
    pub fn stop(&mut self) {
        self.quit.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

fn init_gpu(shared: &Shared, hwnd: isize) -> Result<Gpu, String> {
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
    // UI の色は表示の値で指定するので、sRGB の変換が掛からないフォーマットを選ぶ。
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
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
    Ok(Gpu {
        surface,
        device,
        queue,
        config,
    })
}

fn run(
    shared: &Shared,
    hwnd: isize,
    quit: &AtomicBool,
    model: TimelineModel,
) -> Result<(), String> {
    let mut gpu = init_gpu(shared, hwnd)?;
    let mut ui = Ui::new(&gpu.device, gpu.config.format);
    ui.set_model(model);
    let mut configured = (0u32, 0u32);
    let mut seen = 0u64;
    // HWND が破棄された後に present し続けないよう、毎周で生死を確かめる。
    while !quit.load(Ordering::SeqCst) && surface::is_alive(hwnd) {
        let (w, h) = shared.surface_size();
        if w == 0 || h == 0 {
            std::thread::sleep(RETRY_WAIT);
            continue;
        }
        if (w, h) != configured {
            gpu.config.width = w;
            gpu.config.height = h;
            gpu.surface.configure(&gpu.device, &gpu.config);
            ui.resize(w, h);
            configured = (w, h);
        }
        handle_input(shared, &mut ui);
        frame(shared, &gpu, &mut ui, &mut seen);
    }
    Ok(())
}

/// ポインターの入力を UI に渡し、起きた操作を再生スレッドへ送る。
fn handle_input(shared: &Shared, ui: &mut Ui) {
    let events = std::mem::take(&mut *shared.input.lock().expect("input"));
    for ev in events {
        for action in ui.pointer(ev) {
            match action {
                Action::Seek(t) => {
                    // スクラブは再生を止めて位置を追う。
                    if shared.playing.load(Ordering::SeqCst) {
                        shared.send(Command::Pause);
                    }
                    shared.send(Command::Seek(t));
                }
                Action::Select(_) | Action::Trim { .. } => {}
            }
        }
    }
}

fn frame(shared: &Shared, gpu: &Gpu, ui: &mut Ui, seen: &mut u64) {
    let _span = tracing::info_span!("window.frame").entered();
    let texture = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
        other => {
            // 大きさの変更の直後の Outdated や、隠れている間の Occluded は普通なので、数えて張り直す。
            shared.surface_errors.fetch_add(1, Ordering::SeqCst);
            tracing::debug!(state = ?other, "サーフェスを取れなかった");
            gpu.surface.configure(&gpu.device, &gpu.config);
            std::thread::sleep(RETRY_WAIT);
            return;
        }
    };
    let uploaded = upload_frame(shared, gpu, ui, seen);
    ui.set_playhead(shared.playhead());
    let view = texture.texture.create_view(&Default::default());
    ui.render(&gpu.device, &gpu.queue, &view);
    {
        let _present = tracing::info_span!("preview.present").entered();
        gpu.queue.present(texture);
    }
    shared.frames_presented.fetch_add(1, Ordering::SeqCst);
    if let Some(frame_time) = uploaded {
        record_skew(shared, frame_time);
    }
}

/// 新しいフレームがあれば上げて、そのフレームの時刻を返す。
fn upload_frame(shared: &Shared, gpu: &Gpu, ui: &mut Ui, seen: &mut u64) -> Option<f64> {
    let frame = shared.frames.newer_than(*seen)?;
    *seen = frame.serial;
    let started = Instant::now();
    ui.set_frame(&gpu.device, &gpu.queue, &frame.image);
    shared.record("upload_ms", started.elapsed().as_secs_f64() * 1000.0);
    Some(frame.time)
}

/// 映像と音のずれ（ミリ秒）。正は映像が音の時計より遅れている。再生中に出したフレームだけ測る。
fn record_skew(shared: &Shared, frame_time: f64) {
    if !shared.playing.load(Ordering::SeqCst) {
        return;
    }
    let skew_ms = (shared.playhead() - frame_time) * 1000.0;
    shared.record("av_skew_ms", skew_ms);
    mm_trace::gauge!("preview.av_skew_ms", skew_ms);
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
    // HWND はウィンドウが閉じる前に描画スレッドを止めて待つので、描画スレッドの間は生きている。
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
