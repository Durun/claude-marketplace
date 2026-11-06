//! サーフェス 1 枚分の UI。上に Program、下に Timeline を描き、Timeline のポインター操作を受ける。

use mm_types::RgbaImage;
use mm_ui_layout::palette::{self, Palette};
use mm_ui_layout::{
    fit_rect, split_surface, timeline_quads, Action, PaintState, PointerEvent, Quad, Rect,
    SurfaceLayout, TimeView, TimelineController, TimelineModel, VIEW_MARGIN,
};

use crate::program::ProgramViewer;
use crate::quads::QuadRenderer;

/// サーフェスの UI。描画の資源と、Timeline の表示・操作の状態を持つ。
pub struct Ui {
    quads: QuadRenderer,
    program: ProgramViewer,
    model: TimelineModel,
    controller: TimelineController,
    layout: SurfaceLayout,
    size: (f32, f32),
    playhead: f64,
    fitted: bool,
    palette: Palette,
}

impl Ui {
    /// `format` はサーフェスのフォーマット。
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            quads: QuadRenderer::new(device, format),
            program: ProgramViewer::new(device, format),
            model: TimelineModel::default(),
            controller: TimelineController::new(TimeView {
                origin: 0.0,
                px_per_sec: 100.0,
            }),
            layout: SurfaceLayout {
                program: Rect::default(),
                timeline: Rect::default(),
            },
            size: (0.0, 0.0),
            playhead: 0.0,
            fitted: false,
            palette: Palette::DEFAULT,
        }
    }

    /// Timeline の色（`ui/theme` の theme.json から作ったもの）。
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// サーフェスの大きさ（物理ピクセル）。割り付けを計算し直す。
    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = (width as f32, height as f32);
        self.layout = split_surface(self.size.0, self.size.1, self.model.content_height());
        if !self.fitted {
            self.fit();
        }
    }

    /// Timeline に出すもの。最初に渡されたときに全体表示にする。
    pub fn set_model(&mut self, model: TimelineModel) {
        self.model = model;
        self.fitted = false;
        self.resize(self.size.0 as u32, self.size.1 as u32);
    }

    /// 全体が収まる表示にする。
    pub fn fit(&mut self) {
        if self.layout.timeline.w > 0.0 && self.model.duration > 0.0 {
            self.controller.view =
                TimeView::fit(self.model.duration, self.layout.timeline.w, VIEW_MARGIN);
            self.fitted = true;
        }
    }

    /// 再生位置（秒）。
    pub fn set_playhead(&mut self, secs: f64) {
        self.playhead = secs;
    }

    /// Program に出すフレーム。
    pub fn set_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &RgbaImage) {
        self.program.set_image(device, queue, image);
    }

    /// Program に出すフレームを、同じデバイスのテクスチャのまま渡す。
    pub fn set_frame_texture(&mut self, device: &wgpu::Device, texture: &wgpu::Texture) {
        self.program.set_texture(device, texture);
    }

    /// 現在の割り付け。
    pub fn layout(&self) -> SurfaceLayout {
        self.layout
    }

    /// サーフェスの座標のポインターの入力を受ける。Timeline の外から始まったドラッグは捨てる。
    pub fn pointer(&mut self, ev: PointerEvent) -> Vec<Action> {
        let t = self.layout.timeline;
        let local = match ev {
            PointerEvent::Down { x, y } if !t.contains(x, y) => return Vec::new(),
            PointerEvent::Wheel { x, y, .. } if !t.contains(x, y) => return Vec::new(),
            PointerEvent::Down { x, y } => PointerEvent::Down {
                x: x - t.x,
                y: y - t.y,
            },
            PointerEvent::Move { x, y } => PointerEvent::Move {
                x: x - t.x,
                y: y - t.y,
            },
            PointerEvent::Wheel { x, y, dy, ctrl } => PointerEvent::Wheel {
                x: x - t.x,
                y: y - t.y,
                dy,
                ctrl,
            },
            other => other,
        };
        self.controller.handle(local, &self.model, self.playhead)
    }

    /// 1 フレーム描く。`target` はサーフェスのテクスチャのビュー。
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
    ) {
        let (w, h) = self.size;
        let count = self
            .quads
            .prepare(device, queue, &self.timeline_quads(), (w, h));
        if let Some((iw, ih)) = self.program.image_size() {
            let rect = fit_rect(iw as f32, ih as f32, self.layout.program);
            self.program.prepare(queue, rect, (w, h));
        }
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = begin_pass(&mut enc, target);
            self.program.draw(&mut pass);
            self.quads.draw(&mut pass, count);
        }
        queue.submit([enc.finish()]);
    }

    /// Timeline の矩形を、サーフェスの座標に置いた並びにする。
    fn timeline_quads(&self) -> Vec<Quad> {
        let t = self.layout.timeline;
        let mut quads = timeline_quads(
            &self.model,
            &self.controller.view,
            (t.w, t.h),
            &PaintState {
                palette: self.palette,
                ..self.controller.paint_state(self.playhead)
            },
        );
        for q in &mut quads {
            q.rect = q.rect.translated(t.x, t.y);
        }
        quads
    }
}

/// サーフェス全体を Program の背景色で塗るパスを始める。
fn begin_pass<'a>(
    enc: &'a mut wgpu::CommandEncoder,
    target: &'a wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    let c = palette::PROGRAM_BACKDROP.0;
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ui"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(c[0]),
                    g: f64::from(c[1]),
                    b: f64::from(c[2]),
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
