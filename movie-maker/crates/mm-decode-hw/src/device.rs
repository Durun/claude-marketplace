//! wgpu の D3D12 のキューとデバイスへの、フェンスの待ちと信号、デバイスの消失の確認。
//!
//! 待ちは `ID3D12CommandQueue::Wait` を直接積む。wgpu の `add_wait_fence` は「キューの次の submit」に付くため、
//! 別のスレッドが先に submit すると待ちがそちらに付いてしまう。キューへ直接積んだ待ちは、積んだ時点より後に
//! キューへ入る全部の実行に効くので、どのスレッドの submit が挟まっても自分の変換より前に効く
//! （余計な待ちが他の submit に付くだけで、正しさは変わらない）。
//! 信号は submit の後に直接積む。間に別の submit が挟まっても、信号が遅れるだけで早まらない。

use mm_types::{MmError, Result};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandQueue, ID3D12Fence, D3D12_FENCE_FLAG_NONE,
};

const NOT_D3D12: &str = "wgpu が D3D12 ではない";

/// wgpu のキューの D3D12 のコマンドキューで `f` を実行する。
fn with_raw_queue<T>(
    queue: &wgpu::Queue,
    f: impl FnOnce(&ID3D12CommandQueue) -> windows::core::Result<T>,
    what: &str,
) -> Result<T> {
    // SAFETY: as_hal の参照はこの呼び出しの間だけ使う。
    let hal = unsafe { queue.as_hal::<wgpu::hal::api::Dx12>() }
        .ok_or_else(|| MmError::Unsupported(NOT_D3D12.into()))?;
    f(hal.as_raw()).map_err(|e| MmError::Internal(format!("{what}: {e}")))
}

/// キューの以後の実行を、`fence` が `value` に届くまで GPU の側で止める（CPU は待たない）。
pub fn queue_wait(queue: &wgpu::Queue, fence: &ID3D12Fence, value: u64) -> Result<()> {
    // SAFETY: キューとフェンスは同じデバイスの有効なオブジェクト。
    with_raw_queue(queue, |q| unsafe { q.Wait(fence, value) }, "キューの Wait")
}

/// キューのここまでの実行が終わったら、`fence` を `value` にする。
pub fn queue_signal(queue: &wgpu::Queue, fence: &ID3D12Fence, value: u64) -> Result<()> {
    // SAFETY: キューとフェンスは同じデバイスの有効なオブジェクト。
    with_raw_queue(
        queue,
        |q| unsafe { q.Signal(fence, value) },
        "キューの Signal",
    )
}

/// 初期値 0 のフェンスを作る。`flags` が共有ならほかの API と共有できる。
pub fn create_fence(
    device: &wgpu::Device,
    flags: windows::Win32::Graphics::Direct3D12::D3D12_FENCE_FLAGS,
) -> Result<ID3D12Fence> {
    // SAFETY: as_hal の参照はこの関数の間だけ使う。
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
        .ok_or_else(|| MmError::Unsupported(NOT_D3D12.into()))?;
    // SAFETY: 有効なデバイスに、初期値 0 のフェンスを作る。
    unsafe { hal.raw_device().CreateFence(0, flags) }
        .map_err(|e| MmError::Internal(format!("フェンスの作成: {e}")))
}

/// 共有でない、初期値 0 のフェンスを作る。
pub fn create_local_fence(device: &wgpu::Device) -> Result<ID3D12Fence> {
    create_fence(device, D3D12_FENCE_FLAG_NONE)
}

/// デバイスが消えていないか。NVIDIA のドライバーは復号の失敗をエラーで返さずデバイスの消失で表すことがある。
pub fn check_device(device: &wgpu::Device) -> Result<()> {
    // SAFETY: as_hal の参照はこの関数の間だけ使う。
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
        .ok_or_else(|| MmError::Unsupported(NOT_D3D12.into()))?;
    // SAFETY: 引数なしの問い合わせ。
    unsafe { hal.raw_device().GetDeviceRemovedReason() }
        .map_err(|e| MmError::Internal(format!("D3D12 のデバイスが消えた: {e}")))
}
