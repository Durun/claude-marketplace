//! wgpu の Device と Queue を持つ文脈。headless（面なし）で作る。

use std::sync::{Arc, Mutex};

use mm_types::{MmError, Result};

/// 中間のテクスチャの形式。口の約束（`Rgba16Float`、premultiplied、線形）のとおり。
pub const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// GPU の文脈。描画器と `WgpuGpu` が共有する。
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// 選ばれたアダプターの名前。Program に出す。
    pub adapter_name: String,
    /// タイムスタンプのクエリが使えるか。
    pub timestamps: bool,
}

impl GpuContext {
    /// 高性能のアダプターを選んで作る。GPU が無ければ `Err(Unsupported)`。
    pub fn new() -> Result<Arc<Self>> {
        pollster::block_on(Self::new_async())
    }

    /// プロセスで 1 つの文脈を共有する。Device の生成と破棄が並行すると Windows の
    /// ドライバが終了時に落ちることがあるため、テストと書き出しはこちらを使う。
    pub fn shared() -> Result<Arc<Self>> {
        static SHARED: Mutex<Option<Arc<GpuContext>>> = Mutex::new(None);
        let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ctx) = slot.as_ref() {
            return Ok(ctx.clone());
        }
        let ctx = Self::new()?;
        *slot = Some(ctx.clone());
        Ok(ctx)
    }

    async fn new_async() -> Result<Arc<Self>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(|e| MmError::Unsupported(format!("GPU のアダプターが無い: {e}")))?;
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let required_features = if timestamps {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mm-render"),
                required_features,
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| MmError::Unsupported(format!("GPU のデバイスを作れない: {e}")))?;
        Ok(Arc::new(Self {
            device,
            queue,
            adapter_name: adapter.get_info().name,
            timestamps,
        }))
    }
}

impl Drop for GpuContext {
    /// プロセスの終了と GPU の後片付けが重ならないよう、未完の仕事を待ってから Device を壊す。
    fn drop(&mut self) {
        // 待ちに失敗しても片付けは続ける。
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.device.destroy();
    }
}
