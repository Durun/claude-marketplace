//! デコード結果（FFmpeg のプールのサーフェス）を、GPU が読み終えるまで保持する。
//!
//! サーフェスを解放するとプールに戻り、次のデコードが上書きする。FFmpeg は wgpu のキューの読み取りを知らないので、
//! wgpu のキューで進めたフェンスが値に届くまで解放しない。

use std::collections::VecDeque;

use ffmpeg_next::frame;
use mm_types::{MmError, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::ID3D12Fence;

/// 保持したままにするサーフェスの上限。超えたら一番古いものの完了を CPU で待つ。
/// FFmpeg のプールの予備（`extra_hw_frames`）より小さくして、デコードがサーフェスを取れなくなる前に止める
/// （d3d12va は `EXTRA_HW_FRAMES > MAX_PENDING` をコンパイル時に確かめる）。
pub const MAX_PENDING: usize = 8;

/// 読み終えるのを待っているサーフェス。
struct Pending {
    fence: ID3D12Fence,
    value: u64,
    _frame: frame::Video,
}

/// 解放の順番待ちのサーフェス。
pub struct ReleaseQueue {
    pending: VecDeque<Pending>,
    /// テストで待ちを外した対照を作るためのフラグ。本番では常に `true`。
    wait_for_gpu: bool,
}

impl Default for ReleaseQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl ReleaseQueue {
    /// 空の待ち行列。
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            wait_for_gpu: true,
        }
    }

    /// GPU の完了を待たずに解放する（待ちが効いていることを確かめる対照のため）。
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn set_wait_for_gpu(&mut self, on: bool) {
        self.wait_for_gpu = on;
    }

    /// 保持しているサーフェスの数。
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// 保持しているサーフェスが無いか。
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// `fence` が `value` に届いたら解放するサーフェスとして保持する。
    pub fn push(&mut self, fence: ID3D12Fence, value: u64, frame: frame::Video) -> Result<()> {
        if !self.wait_for_gpu {
            return Ok(());
        }
        self.pending.push_back(Pending {
            fence,
            value,
            _frame: frame,
        });
        self.reap();
        while self.pending.len() > MAX_PENDING {
            self.wait_oldest()?;
        }
        Ok(())
    }

    /// 完了したものを解放する（CPU は待たない）。
    pub fn reap(&mut self) {
        while let Some(front) = self.pending.front() {
            // SAFETY: フェンスは有効な COM オブジェクト。
            if unsafe { front.fence.GetCompletedValue() } >= front.value {
                self.pending.pop_front();
            } else {
                break;
            }
        }
    }

    fn wait_oldest(&mut self) -> Result<()> {
        if let Some(front) = self.pending.pop_front() {
            wait_fence(&front.fence, front.value)?;
        }
        Ok(())
    }

    /// 全部の完了を待って解放する。デコーダーを壊す前に呼ぶ。待てないなら、保持したまま壊すより解放して進む。
    pub fn wait_all(&mut self) {
        let _ = self.try_wait_all();
    }

    /// 全部の完了を待って解放する。待てなかったサーフェスがあれば、残りも解放した上で最初のエラーを返す。
    pub fn try_wait_all(&mut self) -> Result<()> {
        let mut first_error = None;
        while !self.pending.is_empty() {
            if let Err(e) = self.wait_oldest() {
                first_error.get_or_insert(e);
                self.pending.clear();
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// フェンスが `value` に届くまで CPU で待つ。
pub fn wait_fence(fence: &ID3D12Fence, value: u64) -> Result<()> {
    // SAFETY: イベントのハンドルを渡さない（NULL）と、完了までこの呼び出しが返らない。
    unsafe {
        if fence.GetCompletedValue() >= value {
            return Ok(());
        }
        fence
            .SetEventOnCompletion(value, HANDLE::default())
            .map_err(|e| MmError::Internal(format!("フェンスの待ち: {e}")))
    }
}
