//! 復号結果（FFmpeg のプールの面）を、GPU が読み終えるまで握る。
//!
//! 面を放すとプールに戻り、次の復号が上書きする。FFmpeg は wgpu のキューの読み取りを知らないので、
//! wgpu のキューで進めたフェンスが値に届くまで放さない。

use std::collections::VecDeque;

use ffmpeg_next::frame;
use mm_types::{MmError, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::ID3D12Fence;

/// 握ったままにする面の上限。超えたら一番古いものの完了を CPU で待つ。
/// FFmpeg のプールの予備（`extra_hw_frames`）より小さくして、復号が面を取れなくなる前に止める。
pub const MAX_PENDING: usize = 8;

/// 読み終えるのを待っている面。
struct Pending {
    fence: ID3D12Fence,
    value: u64,
    _frame: frame::Video,
}

/// 放す順番待ちの面。
pub struct ReleaseQueue {
    pending: VecDeque<Pending>,
    /// テストで待ちを外した対照を作るための印。本番では常に `true`。
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

    /// GPU の完了を待たずに放す（待ちが効いていることを確かめる対照のため）。
    pub fn set_wait_for_gpu(&mut self, on: bool) {
        self.wait_for_gpu = on;
    }

    /// 握っている面の数。
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// 握っている面が無いか。
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// `fence` が `value` に届いたら放す面として握る。
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

    /// 完了したものを放す（CPU は待たない）。
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

    /// 全部の完了を待って放す。復号器を壊す前に呼ぶ。
    pub fn wait_all(&mut self) {
        while !self.pending.is_empty() {
            if self.wait_oldest().is_err() {
                // 待てないなら、握ったまま壊すより放して進む。
                self.pending.clear();
            }
        }
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
