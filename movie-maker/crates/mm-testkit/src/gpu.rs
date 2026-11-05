//! CPU で動く `GpuHelper`。テクスチャは番号で引く絵として持つ。

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use mm_node_api::{FrameRef, GpuHelper, GpuTexture};
use mm_types::{MmError, Result, RgbaImage, Size};

/// 見本の GPU。`upload` した絵を番号で覚え、`download` で返す。
#[derive(Debug, Default)]
pub struct CpuGpu {
    next_id: u64,
    textures: HashMap<u64, Arc<RgbaImage>>,
}

impl CpuGpu {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 今持っているテクスチャの数。漏れの検査に使う。
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    fn insert(&mut self, image: Arc<RgbaImage>) -> FrameRef {
        let id = self.next_id;
        self.next_id += 1;
        let size = image.size();
        self.textures.insert(id, image);
        FrameRef::Gpu(GpuTexture { id, size })
    }
}

impl GpuHelper for CpuGpu {
    fn upload(&mut self, image: &RgbaImage) -> Result<FrameRef> {
        Ok(self.insert(Arc::new(image.clone())))
    }

    fn download(&mut self, frame: &FrameRef) -> Result<Arc<RgbaImage>> {
        match frame {
            FrameRef::Transparent { size } => Ok(Arc::new(RgbaImage::transparent(*size))),
            FrameRef::Cpu(img) => Ok(img.clone()),
            FrameRef::Gpu(tex) => self
                .textures
                .get(&tex.id)
                .cloned()
                .ok_or_else(|| MmError::NotFound(format!("テクスチャ {}", tex.id))),
        }
    }

    fn allocate(&mut self, size: Size) -> Result<FrameRef> {
        Ok(self.insert(Arc::new(RgbaImage::transparent(size))))
    }

    fn release(&mut self, frame: &FrameRef) {
        if let FrameRef::Gpu(tex) = frame {
            self.textures.remove(&tex.id);
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
