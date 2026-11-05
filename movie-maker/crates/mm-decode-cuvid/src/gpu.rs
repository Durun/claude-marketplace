//! D3D12 バックエンドの wgpu の Device と Queue。

use std::sync::Arc;

use mm_types::{MmError, Result};

/// wgpu の文脈。
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
}

impl GpuContext {
    /// D3D12 のアダプターで作る。無ければ `Err(Unsupported)`。
    pub fn new() -> Result<Arc<Self>> {
        pollster::block_on(Self::new_async())
    }

    async fn new_async() -> Result<Arc<Self>> {
        // 共有バッファと共有フェンスは D3D12 の資源なので、バックエンドを D3D12 に固定する。
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(|e| MmError::Unsupported(format!("D3D12 のアダプターが無い: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mm-decode-cuvid"),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| MmError::Unsupported(format!("GPU のデバイスを作れない: {e}")))?;
        Ok(Arc::new(Self {
            device,
            queue,
            adapter_name: adapter.get_info().name,
        }))
    }
}
