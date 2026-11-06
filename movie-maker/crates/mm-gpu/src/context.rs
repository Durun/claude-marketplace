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
    /// 選べるバックエンド。D3D12 の資源を FFmpeg や CUDA と共有するデコーダーは D3D12 だけにする。
    pub backends: wgpu::Backends,
}

impl Default for GpuRequest {
    /// 高性能のアダプターで、GPU の時間を測るタイムスタンプがあれば使う。
    fn default() -> Self {
        Self {
            adapter: None,
            optional_features: wgpu::Features::TIMESTAMP_QUERY,
            backends: wgpu::Backends::all(),
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
    /// 作るときに求められた任意の機能。後の求めがこの部分集合なら `shared` は同じ文脈を返す。
    requested_features: wgpu::Features,
    /// 選んだアダプターのバックエンド。`shared` が後の求めのバックエンドの範囲と照合する。
    backend: wgpu::Backend,
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
    /// 2 回目以降は、作ってあるものが `request` に合えばそれを返し、合わなければ `Err`。
    /// アダプターは指定があるときだけ照合し、バックエンドは選んだアダプターが求めの範囲にあるか、
    /// 機能は作ったときの希望が求めを含むかで照合する。
    pub fn shared(request: &GpuRequest) -> Result<Arc<Self>> {
        static SHARED: Mutex<Option<Arc<GpuContext>>> = Mutex::new(None);
        let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ctx) = slot.as_ref() {
            ctx.check_compatible(request)?;
            return Ok(ctx.clone());
        }
        let ctx = Self::with(request)?;
        *slot = Some(ctx.clone());
        Ok(ctx)
    }

    fn check_compatible(&self, request: &GpuRequest) -> Result<()> {
        if let Some(wanted) = &request.adapter {
            if !self.adapter.matches(wanted) {
                return Err(MmError::InvalidArgument(format!(
                    "共有の GPU は {} だが {} を求められた",
                    self.adapter.name, wanted.name
                )));
            }
        }
        // 選んだアダプターのバックエンドが求めの範囲にあれば足りる（全バックエンドで作った後の
        // D3D12 だけの求めが、アダプターが D3D12 なら満たされる）。
        if !request
            .backends
            .contains(wgpu::Backends::from(self.backend))
        {
            return Err(MmError::InvalidArgument(format!(
                "共有の GPU はバックエンド {:?} のアダプターで、{:?} を求められた",
                self.backend, request.backends
            )));
        }
        if !self.requested_features.contains(request.optional_features) {
            return Err(MmError::InvalidArgument(format!(
                "共有の GPU は機能 {:?} で作ってあり、{:?} を求められた",
                self.requested_features, request.optional_features
            )));
        }
        Ok(())
    }

    /// 選んだアダプターのバックエンド。
    pub fn backend(&self) -> wgpu::Backend {
        self.backend
    }

    /// 選んだアダプターの名前。Program に出す。
    pub fn adapter_name(&self) -> &str {
        &self.adapter.name
    }

    async fn new_async(request: &GpuRequest) -> Result<Arc<Self>> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = request.backends;
        let instance = wgpu::Instance::new(desc);
        let adapter = pick_adapter(&instance, request.adapter.as_ref()).await?;
        let adapter_info = adapter.get_info();
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
            requested_features: request.optional_features,
            backend: adapter_info.backend,
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
        // 保存した識別子の LUID は古いことがあるので、機種で絞ってから同じ回の LUID を優先する。
        let mut same_model: Vec<_> = instance
            .enumerate_adapters(wgpu::Backends::PRIMARY)
            .await
            .into_iter()
            .filter(|a| adapter_id(a).matches(wanted))
            .collect();
        let exact = same_model
            .iter()
            .position(|a| adapter_id(a).is_same_instance(wanted))
            .unwrap_or(0);
        return if same_model.is_empty() {
            Err(MmError::Unsupported(format!("GPU {} が無い", wanted.name)))
        } else {
            Ok(same_model.swap_remove(exact))
        };
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
