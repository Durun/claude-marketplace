//! `Ui` をオフスクリーンのテクスチャに描いて、画素を読み戻す。GPU が無ければ飛ばす（`MM_REQUIRE_GPU` があれば失敗）。

use mm_types::{Rgba, RgbaImage, Size};
use mm_ui_layout::{palette, Item, ItemKind, TimelineModel};
use mm_ui_wgpu::Ui;

const W: u32 = 640;
const H: u32 = 400;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }));
    let made = adapter.map_err(|e| e.to_string()).and_then(|a| {
        pollster::block_on(a.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| e.to_string())
    });
    match made {
        Ok(v) => Some(v),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いので飛ばす: {e}");
            None
        }
    }
}

fn model() -> TimelineModel {
    TimelineModel {
        duration: 10.0,
        fps: 30.0,
        items: vec![Item {
            start: 0.0,
            end: 10.0,
            kind: ItemKind::Footage,
            enabled: true,
        }],
    }
}

/// 描いた結果を RGBA8 で返す（行は詰めてある）。
fn render(device: &wgpu::Device, queue: &wgpu::Queue, ui: &mut Ui) -> Vec<u8> {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    ui.render(device, queue, &texture.create_view(&Default::default()));
    let row = (W * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * H),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let mapped = buffer.slice(..).get_mapped_range().expect("range");
    let mut out = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        let start = (y * row) as usize;
        out.extend_from_slice(&mapped[start..start + (W * 4) as usize]);
    }
    out
}

fn px(img: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2]]
}

fn hex(c: mm_ui_layout::Color) -> [u8; 3] {
    [0, 1, 2].map(|i| (c.0[i] * 255.0).round() as u8)
}

fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    a.iter()
        .zip(&b)
        .all(|(x, y)| (i32::from(*x) - i32::from(*y)).abs() <= tol)
}

#[test]
fn program_は線形の画像を_rec1886_の表示の値で出す() {
    let Some((device, queue)) = gpu() else { return };
    let mut ui = Ui::new(&device, FORMAT);
    ui.set_model(model());
    ui.resize(W, H);
    // 線形 0.5 は Rec.1886 で 0.5^(1/2.4) = 0.749 → 191。
    let img = RgbaImage::filled(Size::new(16, 9), Rgba::new(0.5, 0.5, 0.5, 1.0));
    ui.set_frame(&device, &queue, &img);
    let out = render(&device, &queue, &mut ui);
    let p = ui.layout().program;
    let centre = px(&out, (p.x + p.w / 2.0) as u32, (p.y + p.h / 2.0) as u32);
    assert!(close(centre, [191, 191, 191], 2), "{centre:?}");
}

#[test]
fn 画像の縦横比を保ち_余白は背景のまま() {
    let Some((device, queue)) = gpu() else { return };
    let mut ui = Ui::new(&device, FORMAT);
    ui.set_model(model());
    ui.resize(W, H);
    let img = RgbaImage::filled(Size::new(16, 9), Rgba::new(1.0, 1.0, 1.0, 1.0));
    ui.set_frame(&device, &queue, &img);
    let out = render(&device, &queue, &mut ui);
    let p = ui.layout().program;
    // Program の領域の左端は 16:9 の画像より横長なので余白。
    let edge = px(&out, 2, (p.y + p.h / 2.0) as u32);
    assert!(close(edge, hex(palette::PROGRAM_BACKDROP), 2), "{edge:?}");
}

#[test]
fn 再生位置の線が時刻の位置に立つ() {
    let Some((device, queue)) = gpu() else { return };
    let mut ui = Ui::new(&device, FORMAT);
    ui.set_model(model());
    ui.resize(W, H);
    ui.set_playhead(5.0);
    let out = render(&device, &queue, &mut ui);
    let t = ui.layout().timeline;
    // 全体表示は 10 秒が余白 16 px を除いた幅に収まる。5 秒はその中央。
    let x = (t.x + 16.0 + (t.w - 32.0) / 2.0) as u32;
    let y = (t.y + t.h - 3.0) as u32;
    let found =
        (x.saturating_sub(2)..=x + 2).any(|xx| close(px(&out, xx, y), hex(palette::PLAYHEAD), 3));
    assert!(found, "x={x} の近くに再生位置の線");
}
