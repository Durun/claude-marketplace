//! CEF のコールバック。フレームはここで wgpu のテクスチャにして `Shared` に置く。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cef::osr_texture_import::SharedTextureHandle;
use cef::{
    AcceleratedPaintInfo, Browser, CefString, DisplayHandler, ImplDisplayHandler, ImplLoadHandler,
    LoadHandler, PaintElementType, Rect, RenderHandler, ScreenInfo, WrapDisplayHandler,
    WrapLoadHandler, WrapRenderHandler, *,
};

/// コールバックと本体の間で共有する状態。CEF のコールバックは UI スレッド（本体と同じ）で呼ばれる。
pub struct Shared {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// true なら `on_accelerated_paint`（共有テクスチャ）、false なら `on_paint`（CPU の BGRA）で受ける。
    pub accelerated: bool,
    pub size: Cell<(u32, u32)>,
    /// 最後に受け取ったフレーム。CEF のハンドルとは別の、こちらが持つテクスチャ。
    pub frame: RefCell<Option<wgpu::Texture>>,
    /// フレームを受け取った回数。
    pub painted: Cell<u64>,
    pub loaded: Cell<bool>,
    pub title: RefCell<String>,
    /// 受け取りに失敗した理由。
    pub error: RefCell<Option<String>>,
}

impl Shared {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        accelerated: bool,
        size: (u32, u32),
    ) -> Rc<Self> {
        Rc::new(Self {
            device,
            queue,
            accelerated,
            size: Cell::new(size),
            frame: RefCell::new(None),
            painted: Cell::new(0),
            loaded: Cell::new(false),
            title: RefCell::new(String::new()),
            error: RefCell::new(None),
        })
    }

    fn fail(&self, message: String) {
        *self.error.borrow_mut() = Some(message);
    }

    fn put(&self, texture: wgpu::Texture) {
        *self.frame.borrow_mut() = Some(texture);
        self.painted.set(self.painted.get() + 1);
    }

    /// 共有ハンドルはコールバックの外で使えないので、この中で自前のテクスチャへコピーして待つ。
    fn import_shared(&self, info: &AcceleratedPaintInfo) {
        let _span = tracing::info_span!("host_cef.import").entered();
        let handle = SharedTextureHandle::new(info);
        // SAFETY: on_accelerated_paint の中で呼び、device と queue は CEF と同じ GPU の D3D12。
        match unsafe { handle.import_texture(&self.device, &self.queue) } {
            Ok(texture) => self.put(texture),
            Err(e) => self.fail(format!("共有テクスチャの取り込みに失敗: {e}")),
        }
    }

    /// フォールバック。CPU の BGRA を毎フレーム GPU へ上げる。
    fn upload_cpu(&self, buffer: &[u8], width: u32, height: u32) {
        let _span = tracing::info_span!("host_cef.upload").entered();
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("CEF OnPaint frame"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            buffer,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );
        // 共有テクスチャの経路と同じく、GPU での完了まで含めて測る。
        self.queue.submit(std::iter::empty());
        if let Err(e) = self.device.poll(wgpu::PollType::wait_indefinitely()) {
            self.fail(format!("GPU の完了待ちに失敗: {e}"));
        }
        self.put(texture);
    }
}

wrap_render_handler! {
    pub struct OsrRenderHandler {
        shared: Rc<Shared>,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let (w, h) = self.shared.size.get();
                rect.width = w as _;
                rect.height = h as _;
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, screen_info: Option<&mut ScreenInfo>) -> ::std::os::raw::c_int {
            if let Some(screen_info) = screen_info {
                screen_info.device_scale_factor = 1.0;
                return true as _;
            }
            false as _
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            if let (Some(info), true) = (info, type_ == PaintElementType::default()) {
                self.shared.import_shared(info);
            }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            if buffer.is_null() || width <= 0 || height <= 0 || type_ != PaintElementType::default() {
                return;
            }
            let (w, h) = (width as u32, height as u32);
            // SAFETY: CEF は buffer を width * height * 4 バイトの BGRA として、この呼び出しの間だけ有効にする。
            let pixels = unsafe { std::slice::from_raw_parts(buffer, (w * h * 4) as usize) };
            self.shared.upload_cpu(pixels, w, h);
        }
    }
}

wrap_load_handler! {
    pub struct OsrLoadHandler {
        shared: Rc<Shared>,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            _can_go_back: ::std::os::raw::c_int,
            _can_go_forward: ::std::os::raw::c_int,
        ) {
            self.shared.loaded.set(is_loading == 0);
        }
    }
}

wrap_display_handler! {
    pub struct OsrDisplayHandler {
        shared: Rc<Shared>,
    }

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(title) = title {
                *self.shared.title.borrow_mut() = title.to_string();
            }
        }
    }
}

wrap_client! {
    pub struct OsrClient {
        render: RenderHandler,
        load: LoadHandler,
        display: DisplayHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load.clone())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
    }
}

/// 3 つのハンドラをまとめたクライアント。
pub fn build_client(shared: &Rc<Shared>) -> Client {
    OsrClient::new(
        OsrRenderHandler::new(shared.clone()),
        OsrLoadHandler::new(shared.clone()),
        OsrDisplayHandler::new(shared.clone()),
    )
}

wrap_app! {
    pub struct HostApp {
        high_performance_gpu: bool,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefStringUtf16>,
            command_line: Option<&mut CommandLine>,
        ) {
            // CEF の GPU プロセスは既定で内蔵 GPU を選ぶことがあり、wgpu の別の GPU では共有ハンドルを開けない。
            if let (Some(command_line), true) = (command_line, self.high_performance_gpu) {
                command_line.append_switch(Some(&"force-high-performance-gpu".into()));
            }
        }
    }
}
