//! wgpu の D3D12 デバイスと、受け取った絵を PNG にするリードバック。

use std::path::Path;

/// CEF の共有ハンドルは D3D12 の `OpenSharedHandle` で開くので、バックエンドは dx12 に固定する。
pub fn create_device(adapter_hint: &str) -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    pollster::block_on(async {
        // CEF の GPU プロセスと同じアダプタでないと共有ハンドルは開けないので、名前で選べるようにする。
        let adapters = instance.enumerate_adapters(wgpu::Backends::DX12).await;
        let names: Vec<String> = adapters.iter().map(|a| a.get_info().name).collect();
        println!("adapters: {names:?}");
        let adapter = if adapter_hint.is_empty() {
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("アダプタが無い: {e}"))?
        } else {
            adapters
                .into_iter()
                .find(|a| a.get_info().name.contains(adapter_hint))
                .ok_or_else(|| format!("{adapter_hint} を含む名前のアダプタが無い: {names:?}"))?
        };
        let name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| format!("デバイスを作れない: {e}"))?;
        Ok((device, queue, name))
    })
}

/// テクスチャ（BGRA8）を RGBA8 の並びで読み戻す。
pub fn read_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Vec<u8>, String> {
    let _span = tracing::info_span!("host_cef.readback").entered();
    let size = texture.size();
    let row_bytes = size.width * 4;
    let padded = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback"),
    });
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
    queue.submit(std::iter::once(encoder.finish()));
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("リードバックの完了待ちに失敗: {e}"))?;
    let mapped = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| format!("リードバックの対応付けに失敗: {e}"))?;
    let mut out = Vec::with_capacity((row_bytes * size.height) as usize);
    for row in mapped.chunks(padded as usize).take(size.height as usize) {
        for px in row[..row_bytes as usize].chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    Ok(out)
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

/// 絵の同一性の確認に使う FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}
