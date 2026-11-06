//! レンダラーの GPU のコンテキストを D3D12 のアダプターで作る頼み方。
//!
//! `GpuContext` は既定で Vulkan と D3D12 の両方から高性能のアダプターを選ぶため、Windows の NVIDIA では
//! Vulkan が選ばれうる。d3d12va と cuvid は FFmpeg と CUDA に D3D12 のデバイスを渡す（共有資源は
//! 同じ `ID3D12Device` の上に置く）ので、LUID を持つ D3D12 のアダプターを名指しして頼む。

use mm_gpu::{adapter_id, GpuRequest};
use mm_types::{GpuAdapterId, MmError, Result};

/// LUID のアダプターを指す `GpuRequest`。別プロセスで親と同じ GPU を選ぶのに使う。
pub fn dx12_request_for_luid(luid: u64, features: wgpu::Features) -> Result<GpuRequest> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    let instance = wgpu::Instance::new(desc);
    let id = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
        .iter()
        .map(adapter_id)
        .find(|id| id.luid == Some(luid))
        .ok_or_else(|| MmError::Unsupported(format!("LUID {luid} の D3D12 のアダプターが無い")))?;
    Ok(GpuRequest {
        adapter: Some(id),
        optional_features: wgpu::Features::TIMESTAMP_QUERY | features,
        backends: wgpu::Backends::DX12,
    })
}

/// D3D12 のアダプターを指す `GpuRequest`。`wanted` があればその機種、なければ D3D12 での高性能のアダプター。
/// `features` はレンダラーのデバイスに求める機能（`TIMESTAMP_QUERY` は常に求める）。
pub fn dx12_request(wanted: Option<&GpuAdapterId>, features: wgpu::Features) -> Result<GpuRequest> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    let instance = wgpu::Instance::new(desc);
    let adapter = match wanted {
        Some(w) => pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
            .into_iter()
            .filter(|a| adapter_id(a).matches(w))
            .max_by_key(|a| adapter_id(a).is_same_instance(w))
            .ok_or_else(|| MmError::Unsupported(format!("D3D12 のアダプター {} が無い", w.name)))?,
        None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| MmError::Unsupported(format!("D3D12 のアダプターが無い: {e}")))?,
    };
    Ok(GpuRequest {
        adapter: Some(adapter_id(&adapter)),
        optional_features: wgpu::Features::TIMESTAMP_QUERY | features,
        backends: wgpu::Backends::DX12,
    })
}
