//! wgpu の D3D12 のコンテキストと、受け取ったフレームのリードバック。

use std::path::Path;
use std::sync::Arc;

use mm_gpu::{GpuContext, GpuRequest};
use mm_types::GpuAdapterId;

/// 名前の一部からアダプターを選ぶ。空なら `None`（高性能のアダプター）。
pub fn find_adapter(hint: &str) -> Result<Option<GpuAdapterId>, String> {
    if hint.is_empty() {
        return Ok(None);
    }
    let all = mm_gpu::enumerate_adapters();
    all.iter()
        .find(|a| a.name.contains(hint))
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            let names: Vec<_> = all.iter().map(|a| a.name.as_str()).collect();
            format!("{hint} を含む名前のアダプターが無い: {names:?}")
        })
}

/// CEF の共有ハンドルは D3D12 の `OpenSharedHandle` で開くので、バックエンドは dx12 に固定する。
pub fn open(adapter: Option<GpuAdapterId>) -> Result<Arc<GpuContext>, String> {
    GpuContext::with(&GpuRequest {
        adapter,
        optional_features: wgpu::Features::empty(),
        backends: wgpu::Backends::DX12,
    })
    .map_err(|e| e.to_string())
}

/// テクスチャ（BGRA8）を、行の詰め物を除いた BGRA8 でリードバックする。
pub fn read_bgra(gpu: &GpuContext, texture: &wgpu::Texture) -> Result<Vec<u8>, String> {
    let _span = tracing::info_span!("host_cef.readback").entered();
    let size = texture.size();
    let row_bytes = size.width * 4;
    let padded = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
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
    gpu.queue.submit(std::iter::once(encoder.finish()));
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("リードバックの完了待ちに失敗: {e}"))?;
    let mapped = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| format!("リードバックの対応付けに失敗: {e}"))?;
    let mut out = Vec::with_capacity((row_bytes * size.height) as usize);
    for row in mapped.chunks(padded as usize).take(size.height as usize) {
        out.extend_from_slice(&row[..row_bytes as usize]);
    }
    Ok(out)
}

/// BGRA8 を RGBA8 の並びにする。
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect()
}

/// RGBA8 を PNG に書く。
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}

/// 画像の同一性の確認に使う FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}
