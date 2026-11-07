//! `mm-host-cef serve` を子プロセスで起こし、`mm-host-api` の契約と、共有テクスチャで受け取った 1 フレームを
//! 期待画像と比べる。CEF の配布物（`CEF_PATH`）が無い環境ではスキップする（`MM_REQUIRE_CEF` があれば失敗にする）。
#![cfg(all(feature = "cef", windows))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_gpu::{GpuContext, GpuRequest};
use mm_host_api::{HostProcess, HostRequest, HostResponse, HostSpec, SharedFrame};
use mm_host_cef::{pixel, CefLauncher, PipeHost, CEF_PATH_ENV};
use mm_testkit::golden::{backend_name, Golden, GOLDEN_SIZE};
use mm_testkit::host::HostFixture;
use mm_types::{RationalTime, RgbaImage, Size};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

const PAGE: &str = include_str!("pages/golden.html");
const BENCH_PAGE: &str = include_str!("../../assets/bench.html");
pub(crate) const TIMEOUT: Duration = Duration::from_secs(3);
/// 契約の時間切れに使う、レンダラを止める長さ。`TIMEOUT` より長くする。
const BUSY_MS: u64 = 8000;
const REQUIRE_ENV: &str = "MM_REQUIRE_CEF";

/// CEF の配布物が無ければ `None`（スキップする）。
pub(crate) fn host_exe() -> Option<PathBuf> {
    if std::env::var_os(CEF_PATH_ENV).is_none() {
        assert!(
            std::env::var_os(REQUIRE_ENV).is_none(),
            "{REQUIRE_ENV} があるのに {CEF_PATH_ENV} が無い"
        );
        eprintln!("{CEF_PATH_ENV} が無いので CEF のテストをスキップする");
        return None;
    }
    Some(PathBuf::from(env!("CARGO_BIN_EXE_mm-host-cef")))
}

pub(crate) fn gpu() -> Arc<GpuContext> {
    GpuContext::shared(&GpuRequest {
        adapter: None,
        optional_features: wgpu::Features::empty(),
        backends: wgpu::Backends::DX12,
    })
    .expect("D3D12 の GPU")
}

pub(crate) fn spec(exe: &Path, client_pid: Option<u32>) -> HostSpec {
    HostSpec {
        executable: exe.to_path_buf(),
        args: Vec::new(),
        default_timeout: TIMEOUT,
        gpu: Some(gpu().adapter.clone()),
        client_pid,
    }
}

pub(crate) fn prepare(host: &mut PipeHost, page: &str) {
    let req = HostRequest::Prepare {
        params: serde_json::json!({ "html": page }),
    };
    let res = host.request(&req, TIMEOUT).expect("Prepare");
    assert_eq!(res, HostResponse::Prepared);
}

pub(crate) fn render(
    host: &mut PipeHost,
    t: RationalTime,
    size: Size,
    params: serde_json::Value,
) -> SharedFrame {
    let req = HostRequest::RenderFrame {
        time: t,
        size,
        params,
    };
    match host
        .request(&req, TIMEOUT)
        .unwrap_or_else(|e| panic!("RenderFrame {t:?} {size:?}: {e}"))
    {
        HostResponse::Frame(frame) => frame,
        other => panic!("Frame のはずが {other:?}"),
    }
}

/// アプリ側として共有ハンドルを開き、BGRA8 をリードバックする。
pub(crate) fn read_shared(ctx: &GpuContext, handle: u64, size: Size) -> Vec<u8> {
    readback(ctx, &open_shared(ctx, handle, size))
}

/// アプリ側として共有ハンドルを開いて wgpu のテクスチャにし、ハンドルを閉じる。
pub(crate) fn open_shared(ctx: &GpuContext, handle: u64, size: Size) -> wgpu::Texture {
    let handle = HANDLE(handle as usize as *mut _);
    // SAFETY: as_hal の参照はこのブロックの間だけ使う。handle はホストがこのプロセスへ複製したもの。
    let resource = unsafe {
        let hal = ctx.device.as_hal::<wgpu::hal::api::Dx12>().expect("D3D12");
        let mut resource: Option<ID3D12Resource> = None;
        hal.raw_device()
            .OpenSharedHandle(handle, &mut resource)
            .expect("共有ハンドルを開ける");
        CloseHandle(handle).expect("ハンドルを閉じられる");
        resource.expect("資源")
    };
    let extent = wgpu::Extent3d {
        width: size.width,
        height: size.height,
        depth_or_array_layers: 1,
    };
    let desc = wgpu::TextureDescriptor {
        label: Some("host frame"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    // SAFETY: ホストは BGRA8 の 2D テクスチャを COMMON で渡す。
    unsafe {
        let hal = wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            desc.format,
            desc.dimension,
            extent,
            1,
            1,
        );
        ctx.device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
            hal,
            &desc,
            wgpu::TextureUses::PRESENT,
        )
    }
}

fn readback(ctx: &GpuContext, texture: &wgpu::Texture) -> Vec<u8> {
    let size = texture.size();
    let row = size.width * 4;
    let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(padded * size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    encoder.transition_resources(
        std::iter::empty(),
        std::iter::once(wgpu::TextureTransition {
            texture,
            selector: None,
            state: wgpu::TextureUses::PRESENT,
        }),
    );
    ctx.queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("リードバック");
    let mapped = buffer.slice(..).get_mapped_range().expect("map");
    mapped
        .chunks(padded as usize)
        .flat_map(|r| r[..row as usize].to_vec())
        .collect()
}

/// アプリが受け取ったフレームを自分のテクスチャへコピーし、完了を待つ（アプリ側の取り込みの費用）。
fn copy_owned(ctx: &GpuContext, source: &wgpu::Texture) -> wgpu::Texture {
    let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("app frame"),
        size: source.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: source.format(),
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_texture(
        source.as_image_copy(),
        target.as_image_copy(),
        source.size(),
    );
    encoder.transition_resources(
        std::iter::empty(),
        std::iter::once(wgpu::TextureTransition {
            texture: source,
            selector: None,
            state: wgpu::TextureUses::PRESENT,
        }),
    );
    ctx.queue.submit([encoder.finish()]);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("コピーの完了待ち");
    target
}

pub(crate) fn to_image(frame: &SharedFrame, ctx: &GpuContext) -> RgbaImage {
    let data = match frame {
        SharedFrame::Cpu { data, .. } => data.clone(),
        SharedFrame::SharedTexture {
            handle,
            size,
            fence,
        } => {
            assert_eq!(*fence, None, "ホストが完了を待ってから返す");
            pixel::bgra8_to_linear_rgba(&read_shared(ctx, *handle, *size))
        }
    };
    RgbaImage::from_raw(frame.size(), data).expect("大きさが合う")
}

fn golden() -> Option<Golden> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden");
    let actual = std::env::temp_dir().join("mm-host-cef-golden-actual");
    Golden::open(&root, &backend_name("dx12", &gpu().adapter.name), &actual)
}

#[test]
fn ホストの契約を満たす() {
    let Some(exe) = host_exe() else { return };
    let fixture = HostFixture {
        spec: spec(&exe, None),
        prepare_params: serde_json::json!({ "html": PAGE }),
        slow_params: serde_json::json!({ "busy_ms": BUSY_MS }),
    };
    mm_testkit::host::contract(|| Arc::new(CefLauncher), &fixture);
}

#[test]
fn 共有テクスチャと_cpu_の画像が期待画像と一致する() {
    let Some(exe) = host_exe() else { return };
    let ctx = gpu();
    let Some(golden) = golden() else { return };
    let t = RationalTime::seconds(1);
    for client in [Some(std::process::id()), None] {
        let mut host = PipeHost::start(spec(&exe, client)).expect("起動");
        prepare(&mut host, PAGE);
        let frame = render(&mut host, t, GOLDEN_SIZE, serde_json::json!({}));
        assert_eq!(
            matches!(frame, SharedFrame::SharedTexture { .. }),
            client.is_some(),
            "client_pid があれば共有テクスチャで返す"
        );
        golden.check("cef-frame", &to_image(&frame, &ctx)).unwrap();
        assert_eq!(host.open_handles(), 0);
    }
}

/// フレームの seq を書いた画素（左上の 4px）を読む。
pub(crate) fn marker(bgra: &[u8], width: u32) -> u64 {
    let at = ((2 * width + 2) * 4) as usize;
    u64::from(bgra[at + 2]) | u64::from(bgra[at + 1]) << 8
}

#[test]
fn 古い時刻のフレームを返さない() {
    let Some(exe) = host_exe() else { return };
    let ctx = gpu();
    let mut host = PipeHost::start(spec(&exe, Some(std::process::id()))).expect("起動");
    prepare(&mut host, PAGE);
    let size = Size::new(320, 180);
    const FRAMES: u64 = 120;
    for seq in 1..=FRAMES {
        let t = RationalTime::new(i128::from(seq), 30).unwrap();
        let frame = render(&mut host, t, size, serde_json::json!({ "seq": seq }));
        let SharedFrame::SharedTexture { handle, .. } = frame else {
            panic!("共有テクスチャのはず");
        };
        assert_eq!(marker(&read_shared(&ctx, handle, size), size.width), seq);
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// 別プロセス越しの 1 フレームの往復（要求からアプリ側のコピーの完了まで）を測って出す。数字は判定しない。
#[test]
fn 別プロセス越しの往復を測る() {
    let Some(exe) = host_exe() else { return };
    let ctx = gpu();
    const WARMUP: i64 = 30;
    const FRAMES: i64 = 300;
    for size in [Size::new(1920, 1080), Size::new(1280, 720)] {
        let mut host = PipeHost::start(spec(&exe, Some(std::process::id()))).expect("起動");
        prepare(&mut host, BENCH_PAGE);
        let mut ms = Vec::new();
        for n in 0..WARMUP + FRAMES {
            let start = Instant::now();
            let t = RationalTime::new(i128::from(n), 30).unwrap();
            let frame = render(&mut host, t, size, serde_json::json!({}));
            let SharedFrame::SharedTexture { handle, .. } = frame else {
                panic!("共有テクスチャのはず");
            };
            copy_owned(&ctx, &open_shared(&ctx, handle, size));
            if n >= WARMUP {
                ms.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        ms.sort_by(f64::total_cmp);
        eprintln!(
            "往復 {}x{}: p50 {:.2} ms, p95 {:.2} ms（{FRAMES} フレーム、アプリ側の GPU のコピーの完了まで）",
            size.width,
            size.height,
            percentile(&ms, 0.5),
            percentile(&ms, 0.95)
        );
    }
}
