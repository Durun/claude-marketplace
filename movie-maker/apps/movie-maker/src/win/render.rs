//! サーフェスに描き続ける描画スレッド。UI スレッドの外で回し、WebView の更新と独立に present する。
//!
//! ペースは Fifo の垂直同期に任せる（自前の sleep や spin は持たない）。再生位置は時計を直接読み、
//! フレームは再生スレッドが置いた最新の 1 枚を取る。
//! サーフェスは再生スレッドのレンダラーと同じ `GpuContext` のデバイスで作り、描いたテクスチャをリードバックせずに描く。
//! レンダラーがデバイスを作り直したら、新しいデバイスでサーフェスと UI を作り直す。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod sync;

use mm_runtime::GpuContext;
use mm_ui_layout::TimelineModel;
use mm_ui_wgpu::Ui;

use crate::win::playback::Command;
use crate::win::state::{FrameImage, PreviewFrame, Shared};
use crate::win::surface;
use crate::win::theme;
use sync::{handle_input, DocSync, SourceSync};

/// サーフェスを取れなかったときの待ち。再試行で CPU を回さないための間隔。
const RETRY_WAIT: Duration = Duration::from_millis(8);
/// 描画スレッドの本体が失敗や panic で終わってから作り直すまでの待ち。
const RESTART_WAIT: Duration = Duration::from_millis(500);
/// サーフェスを取れない周が続いたら、デバイスが消えたと見て再生スレッドにフレームを描き直させる周の数（約 2 秒）。
const SURFACE_LOST_AFTER: u32 = 250;

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
                supervise(&q, RESTART_WAIT, || run(&shared, hwnd, &q, &model));
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

/// 描画スレッドの本体を、失敗や panic で終わっても `quit` が立つまで作り直して回す。
/// 一度の失敗でプレビューが黙って固まったままにならないようにする。
fn supervise(quit: &AtomicBool, wait: Duration, mut body: impl FnMut() -> Result<(), String>) {
    while !quit.load(Ordering::SeqCst) {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut body));
        match outcome {
            Ok(Ok(())) => return,
            Ok(Err(e)) => tracing::error!(error = %e, "描画が失敗したので作り直す"),
            Err(_) => tracing::error!("描画スレッドが panic したので作り直す"),
        }
        std::thread::sleep(wait);
    }
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// レンダラーと共有したコンテキスト。`None` は自前のデバイス（wgpu でないレンダラー）。
    context: Option<Arc<GpuContext>>,
}

impl Gpu {
    /// `frame` のテクスチャをこのデバイスで描けるか。CPU の画像はいつでも描ける。
    fn can_draw(&self, frame: &PreviewFrame) -> bool {
        match (&frame.image, &self.context) {
            (FrameImage::Cpu(_), _) => true,
            (FrameImage::Gpu(f), Some(ctx)) => Arc::ptr_eq(&f.context, ctx),
            (FrameImage::Gpu(_), None) => false,
        }
    }
}

/// レンダラーのコンテキストがあればそのデバイスで、無ければ自前のデバイスでサーフェスを作る。
/// レンダラーのアダプターがサーフェスに出せなければ、自前のデバイスに戻して再生スレッドに CPU の画像を頼む。
fn init_gpu(
    shared: &Shared,
    hwnd: isize,
    mut context: Option<Arc<GpuContext>>,
) -> Result<Gpu, String> {
    let shared_device = match &context {
        Some(ctx) => {
            let surface = create_surface(ctx.instance(), hwnd)?;
            let adapter = ctx.wgpu_adapter().clone();
            adapter
                .is_surface_supported(&surface)
                .then(|| (surface, adapter, ctx.device.clone(), ctx.queue.clone()))
        }
        None => None,
    };
    let (surface, adapter, device, queue) = match shared_device {
        Some(parts) => parts,
        None => {
            if context.take().is_some() {
                tracing::warn!("レンダラーの GPU がサーフェスに出せないので、CPU の画像で受ける");
                shared.cpu_frames.store(true, Ordering::SeqCst);
            }
            own_device(hwnd)?
        }
    };
    let info = adapter.get_info();
    *shared.adapter.lock().expect("adapter") = format!("{} ({:?})", info.name, info.backend);
    let config = surface_config(&surface, &adapter)?;
    Ok(Gpu {
        surface,
        device,
        queue,
        config,
        context,
    })
}

type OwnDevice = (
    wgpu::Surface<'static>,
    wgpu::Adapter,
    wgpu::Device,
    wgpu::Queue,
);

fn own_device(hwnd: isize) -> Result<OwnDevice, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = create_surface(&instance, hwnd)?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| e.to_string())?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| e.to_string())?;
    Ok((surface, adapter, device, queue))
}

fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
) -> Result<wgpu::SurfaceConfiguration, String> {
    let caps = surface.get_capabilities(adapter);
    let (Some(&first), Some(&alpha_mode)) = (caps.formats.first(), caps.alpha_modes.first()) else {
        return Err("アダプターがこのサーフェスの形式を返さない".into());
    };
    // UI の色は表示の値で指定するので、sRGB の変換が掛からないフォーマットを選ぶ。
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(first);
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: 1,
        height: 1,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode,
        view_formats: vec![],
        color_space: wgpu::SurfaceColorSpace::Auto,
    })
}

/// サーフェスと、その上の UI。デバイスを持ち替えるときは丸ごと作り直す。
struct View {
    gpu: Gpu,
    ui: Ui,
    configured: (u32, u32),
}

impl View {
    fn new(
        shared: &Shared,
        hwnd: isize,
        context: Option<Arc<GpuContext>>,
        model: &TimelineModel,
    ) -> Result<Self, String> {
        let gpu = init_gpu(shared, hwnd, context)?;
        let mut ui = Ui::new(&gpu.device, gpu.config.format);
        ui.set_model(model.clone());
        ui.set_palette(theme::surface_palette());
        Ok(Self {
            gpu,
            ui,
            configured: (0, 0),
        })
    }

    fn configure(&mut self, (w, h): (u32, u32)) {
        if (w, h) != self.configured {
            self.gpu.config.width = w;
            self.gpu.config.height = h;
            self.gpu
                .surface
                .configure(&self.gpu.device, &self.gpu.config);
            self.ui.resize(w, h);
            self.configured = (w, h);
        }
    }
}

fn run(
    shared: &Arc<Shared>,
    hwnd: isize,
    quit: &AtomicBool,
    model: &TimelineModel,
) -> Result<(), String> {
    let start = shared.gpu.lock().expect("gpu").clone();
    let mut view = View::new(shared, hwnd, start, model)?;
    let mut seen = 0u64;
    let mut doc = DocSync::default();
    let mut source = SourceSync::default();
    let mut failed_frames = 0u32;
    // HWND が破棄された後に present し続けないよう、毎周で生死を確かめる。
    while !quit.load(Ordering::SeqCst) && surface::is_alive(hwnd) {
        let size = shared.surface_size();
        if size.0 == 0 || size.1 == 0 {
            std::thread::sleep(RETRY_WAIT);
            continue;
        }
        if let Some(context) = moved_context(shared, &view.gpu, seen) {
            tracing::warn!("レンダラーの GPU が変わったので、サーフェスを作り直す");
            view = View::new(shared, hwnd, Some(context), model)?;
            doc = DocSync::default();
            source = SourceSync::default();
        }
        view.configure(size);
        doc.refresh(shared, &mut view.ui);
        source.refresh(shared, &mut view);
        handle_input(shared, &mut view.ui, &doc);
        if frame(shared, &view.gpu, &mut view.ui, &mut seen) {
            failed_frames = 0;
        } else {
            failed_frames += 1;
            nudge_if_lost(shared, &mut failed_frames);
        }
    }
    Ok(())
}

/// サーフェスを取れない周が長く続いたとき、止めている間なら再生スレッドに今のフレームを描き直させる。
/// デバイスが消えていれば、再生スレッドが作り直した新しいデバイスのフレームが届いて、サーフェスも作り直せる。
fn nudge_if_lost(shared: &Shared, failed_frames: &mut u32) {
    if *failed_frames < SURFACE_LOST_AFTER {
        return;
    }
    *failed_frames = 0;
    if !shared.playing.load(Ordering::SeqCst) {
        shared.send(Command::Seek(shared.playhead()));
    }
}

/// 次のフレームが今のデバイスで描けない（レンダラーがデバイスを作り直した）なら、その新しいコンテキスト。
fn moved_context(shared: &Shared, gpu: &Gpu, seen: u64) -> Option<Arc<GpuContext>> {
    let frame = shared.frames.newer_than(seen)?;
    // 自前のデバイスに戻したとき（`cpu_frames`）は、再生スレッドが CPU の画像に切り替えるまで待つ。
    if gpu.can_draw(&frame) || gpu.context.is_none() {
        return None;
    }
    match frame.image {
        FrameImage::Gpu(f) => Some(f.context),
        FrameImage::Cpu(_) => None,
    }
}

/// 1 フレーム描いて present する。サーフェスを取れなければ偽。
fn frame(shared: &Shared, gpu: &Gpu, ui: &mut Ui, seen: &mut u64) -> bool {
    let _span = tracing::info_span!("window.frame").entered();
    let texture = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
        other => {
            // 大きさの変更の直後の Outdated や、隠れている間の Occluded は普通なので、数えて張り直す。
            shared.surface_errors.fetch_add(1, Ordering::SeqCst);
            tracing::debug!(state = ?other, "サーフェスを取れなかった");
            gpu.surface.configure(&gpu.device, &gpu.config);
            std::thread::sleep(RETRY_WAIT);
            return false;
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
    true
}

/// 新しいフレームがあれば Program に渡して、そのフレームの時刻を返す。GPU のテクスチャはリードバックせずにそのまま渡す。
fn upload_frame(shared: &Shared, gpu: &Gpu, ui: &mut Ui, seen: &mut u64) -> Option<f64> {
    let frame = shared.frames.newer_than(*seen)?;
    if !gpu.can_draw(&frame) {
        return None;
    }
    *seen = frame.serial;
    let started = Instant::now();
    match &frame.image {
        FrameImage::Cpu(image) => ui.set_frame(&gpu.device, &gpu.queue, image),
        FrameImage::Gpu(f) => ui.set_frame_texture(&gpu.device, &f.texture),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn 描画の本体が失敗や_panic_で終わっても作り直して回し続ける() {
        let quit = AtomicBool::new(false);
        let mut calls = 0;
        supervise(&quit, Duration::from_millis(1), || {
            calls += 1;
            match calls {
                1 => Err("View::new の失敗".into()),
                2 => panic!("描画スレッドの panic"),
                _ => Ok(()),
            }
        });
        assert_eq!(calls, 3, "3 回目に正常に終わるまで作り直す");
    }

    #[test]
    fn 止める合図が立てば作り直さない() {
        let quit = AtomicBool::new(true);
        let mut calls = 0;
        supervise(&quit, Duration::from_millis(1), || {
            calls += 1;
            Err("失敗".into())
        });
        assert_eq!(calls, 0);
    }

    #[test]
    fn サーフェスを取れない周が続いたら止めている間だけフレームを描き直させる() {
        let shared = Shared::new();
        let (tx, rx) = channel();
        *shared.commands.lock().unwrap() = Some(tx);
        let mut failed = SURFACE_LOST_AFTER - 1;
        nudge_if_lost(&shared, &mut failed);
        assert!(rx.try_recv().is_err(), "しきい値の手前では送らない");
        failed += 1;
        nudge_if_lost(&shared, &mut failed);
        assert_eq!(rx.try_recv(), Ok(Command::Seek(0.0)));
        assert_eq!(failed, 0, "送ったら数え直す");
        shared.playing.store(true, Ordering::SeqCst);
        failed = SURFACE_LOST_AFTER;
        nudge_if_lost(&shared, &mut failed);
        assert!(rx.try_recv().is_err(), "再生中は音を乱さない");
    }
}
