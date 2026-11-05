//! wgpu の Device と Queue を持つ文脈。headless（面なし）で作り、アダプターを 1 回選んで全実装で共有する。

use std::any::Any;
use std::sync::{Arc, Mutex};

use mm_decode_api::GpuContextRef;
use mm_types::{GpuAdapterId, MmError, Result};

/// 中間のテクスチャの形式。口の約束（`Rgba16Float`、premultiplied、線形）のとおり。
pub const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// 文脈の作り方。`mm-runtime` が設定から 1 回決める。
#[derive(Debug, Clone)]
pub struct GpuRequest {
    /// 使うアダプター。`None` なら高性能のアダプター。
    pub adapter: Option<GpuAdapterId>,
    /// あれば有効にする機能。無いアダプターでは落とさずに外す。
    pub optional_features: wgpu::Features,
}

impl Default for GpuRequest {
    /// 高性能のアダプターで、GPU の時間を測るタイムスタンプがあれば使う。
    fn default() -> Self {
        Self {
            adapter: None,
            optional_features: wgpu::Features::TIMESTAMP_QUERY,
        }
    }
}

/// GPU の文脈。描画器、デコーダー、GpuHelper が共有する。
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// 選んだアダプター。別プロセスのホストに渡す。
    pub adapter: GpuAdapterId,
    /// タイムスタンプのクエリが使えるか。
    pub timestamps: bool,
}

impl GpuContext {
    /// 高性能のアダプターを選んで作る。GPU が無ければ `Err(Unsupported)`。
    pub fn new() -> Result<Arc<Self>> {
        Self::with(&GpuRequest::default())
    }

    /// 指定どおりに作る。
    pub fn with(request: &GpuRequest) -> Result<Arc<Self>> {
        pollster::block_on(Self::new_async(request))
    }

    /// プロセスで 1 つの文脈を共有する。Device の生成と破棄が並行すると Windows の
    /// ドライバが終了時に落ちることがあるため、テストと書き出しはこちらを使う。
    /// 2 回目以降の `request` は無視する（最初に決めたアダプターを使う）。
    pub fn shared(request: &GpuRequest) -> Result<Arc<Self>> {
        static SHARED: Mutex<Option<Arc<GpuContext>>> = Mutex::new(None);
        let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ctx) = slot.as_ref() {
            return Ok(ctx.clone());
        }
        let ctx = Self::with(request)?;
        *slot = Some(ctx.clone());
        Ok(ctx)
    }

    /// 選んだアダプターの名前。Program に出す。
    pub fn adapter_name(&self) -> &str {
        &self.adapter.name
    }

    async fn new_async(request: &GpuRequest) -> Result<Arc<Self>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pick_adapter(&instance, request.adapter.as_ref()).await?;
        let required_features = request.optional_features & adapter.features();
        let timestamps = required_features.contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mm-gpu"),
                required_features,
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| MmError::Unsupported(format!("GPU のデバイスを作れない: {e}")))?;
        Ok(Arc::new(Self {
            device,
            queue,
            adapter: adapter_id(&adapter),
            timestamps,
        }))
    }
}

impl GpuContextRef for GpuContext {
    fn adapter(&self) -> &GpuAdapterId {
        &self.adapter
    }

    fn as_any(&self) -> &dyn Any {
        self
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

/// この PC のアダプターの一覧。設定で選ぶときの候補。
pub fn enumerate_adapters() -> Vec<GpuAdapterId> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY))
        .iter()
        .map(adapter_id)
        .collect()
}

/// 指定のアダプターがあればそれ、無ければ高性能のアダプター。指定が見つからなければ `Unsupported`。
async fn pick_adapter(
    instance: &wgpu::Instance,
    wanted: Option<&GpuAdapterId>,
) -> Result<wgpu::Adapter> {
    if let Some(wanted) = wanted {
        return instance
            .enumerate_adapters(wgpu::Backends::PRIMARY)
            .await
            .into_iter()
            .find(|a| adapter_id(a).matches(wanted))
            .ok_or_else(|| MmError::Unsupported(format!("GPU {} が無い", wanted.name)));
    }
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .map_err(|e| MmError::Unsupported(format!("GPU のアダプターが無い: {e}")))
}

/// アダプターの識別子。D3D12 では LUID も取る。
pub fn adapter_id(adapter: &wgpu::Adapter) -> GpuAdapterId {
    let info = adapter.get_info();
    GpuAdapterId {
        vendor: info.vendor,
        device: info.device,
        name: info.name,
        luid: luid_of(adapter),
    }
}

#[cfg(windows)]
fn luid_of(adapter: &wgpu::Adapter) -> Option<u64> {
    // DXGI の記述を読むだけで、デバイスの状態は変えない。
    let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }?;
    let desc = unsafe { hal.raw_adapter().GetDesc1() }.ok()?;
    let luid = desc.AdapterLuid;
    Some(((luid.HighPart as u32 as u64) << 32) | u64::from(luid.LowPart))
}

#[cfg(not(windows))]
fn luid_of(_adapter: &wgpu::Adapter) -> Option<u64> {
    None
}
