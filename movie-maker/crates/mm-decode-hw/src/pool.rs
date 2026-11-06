//! 変換先の `Rgba16Float` のテクスチャプール。借用側が `GpuFrameTexture` をドロップすると自動で戻る。
//!
//! 借用はレンダラーの `submit` が終わるまで（RFC 0001）。戻ったテクスチャへの次の書き込みは
//! 同じキューの後ろに積まれるので、読み取りの後に順序づく。

use std::sync::{Arc, Mutex};

use mm_gpu::{GpuFrameTexture, FRAME_FORMAT};
use mm_types::Size;

/// プールに残す空きの上限。借用が増えても戻りすぎた分は破棄する。
const MAX_FREE: usize = 6;

type Free = Arc<Mutex<Vec<wgpu::Texture>>>;

/// 返却ガード。`GpuFrameTexture::wait` の中に持たせ、ドロップされたときにテクスチャをプールへ戻す。
struct Return {
    free: Free,
    texture: wgpu::Texture,
}

impl Drop for Return {
    fn drop(&mut self) {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        if free.len() < MAX_FREE {
            free.push(self.texture.clone());
        }
    }
}

/// 同じ大きさのテクスチャのプール。
pub struct FramePool {
    device: wgpu::Device,
    size: Size,
    free: Free,
}

impl FramePool {
    /// `size` のテクスチャを作るプール。
    pub fn new(device: wgpu::Device, size: Size) -> Self {
        Self {
            device,
            size,
            free: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 空きがあれば使い回し、無ければ作る。
    pub fn take(&self) -> wgpu::Texture {
        let reused = self.free.lock().unwrap_or_else(|e| e.into_inner()).pop();
        reused.unwrap_or_else(|| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("decoded frame"),
                size: wgpu::Extent3d {
                    width: self.size.width,
                    height: self.size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FRAME_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        })
    }

    /// 借用に出す。ドロップされたら `texture` がプールへ戻る。同じキューで書いた画像なので待ちは付けない。
    pub fn lend(&self, texture: wgpu::Texture) -> GpuFrameTexture {
        let guard = Return {
            free: self.free.clone(),
            texture: texture.clone(),
        };
        GpuFrameTexture {
            texture,
            wait: Some(Box::new(move |_queue| {
                let _keep = &guard;
            })),
        }
    }

    /// プールにある空きの数。
    pub fn free_count(&self) -> usize {
        self.free.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}
