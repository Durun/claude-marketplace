//! デコーダーの GPU のフレームの中身。口の `GpuFrame` の wgpu での形。

use std::any::Any;
use std::sync::Arc;

use mm_decode_api::{GpuFrame, GpuFrameHandle};
use mm_types::Size;

/// デコードの完了を待たせる手続き。`Queue::submit` の前に呼ぶ。
pub type WaitFn = dyn Fn(&wgpu::Queue) + Send + Sync;

/// wgpu のテクスチャに包んだフレーム。`FRAME_FORMAT`（`Rgba16Float`）か、レンダラーが読める形式で渡す。
pub struct GpuFrameTexture {
    pub texture: wgpu::Texture,
    /// デコードの完了を待たせる。`None` ならデコーダーが同じキューで書いており、順序が守られる。
    pub wait: Option<Box<WaitFn>>,
}

impl GpuFrameTexture {
    /// 口の取っ手にする。
    pub fn into_handle(self, id: u64) -> GpuFrameHandle {
        let size = Size::new(self.texture.width(), self.texture.height());
        GpuFrameHandle {
            id,
            size,
            frame: Arc::new(self),
        }
    }

    /// 口の取っ手から戻す。別の実装の中身なら `None`。
    pub fn of(handle: &GpuFrameHandle) -> Option<&Self> {
        Self::of_frame(handle.frame.as_ref())
    }

    /// 型を消した中身から戻す。
    pub fn of_frame(frame: &dyn GpuFrame) -> Option<&Self> {
        frame.as_any().downcast_ref::<Self>()
    }
}

impl GpuFrame for GpuFrameTexture {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
