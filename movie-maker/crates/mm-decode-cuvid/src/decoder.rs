//! `mm-decode-api` の `Decoder` の実装。cuvid（NVDEC）の CUDA のメモリを、レンダラーの D3D12 のデバイスの
//! 共有バッファへ 1 回コピーし、線形の `Rgba16Float`（`GpuFrameTexture`）にして返す。
//!
//! CUDA のコピーは枠ごとの共有フェンスで、wgpu が前に読み終えるまで待つ。FFmpeg の面は、コピーの完了までだけ握る。
//! 音は無音を返す。音は `mm-runtime` が FFmpeg のデコーダーと組み合わせる。

use std::path::Path;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{
    DecodedFrame, Decoder, DecoderFactory, GpuContextRef, GpuFrameHandle, MediaInfo,
};
use mm_gpu::{GpuContext, GpuRequest};
use mm_types::{GpuAdapterId, MmError, RationalTime, Result, TimeRange};

use crate::convert::BufferConverter;
use crate::cuda::CudaApi;
use crate::dec::{self, DeviceRef};
use crate::share::{BufferLayout, SharedRing};
use mm_decode_ffmpeg_core::matrix::{is_full_range, Matrix};
use mm_decode_ffmpeg_core::reader::{IndexedFrame, Sequencer};
use mm_decode_hw::adapter::dx12_request;
use mm_decode_hw::pool::FramePool;
use mm_decode_hw::release::ReleaseQueue;
use mm_decode_hw::yuv::YuvParams;

/// 無音を返すときの形式。
const SILENCE_RATE: u32 = 48_000;
const SILENCE_CHANNELS: u16 = 2;
/// 共有バッファの輪の枠の数。
const RING_SLOTS: usize = 4;

/// レンダラーの GPU の文脈を、cuvid が使える形（D3D12 のアダプター）で頼む。
/// `wanted` が `None` なら D3D12 での高性能のアダプター。
pub fn gpu_request(wanted: Option<&GpuAdapterId>) -> Result<GpuRequest> {
    dx12_request(wanted, wgpu::Features::empty())
}

/// cuvid のデコーダー。
pub struct CuvidDecoder {
    seq: Sequencer,
    bits: u32,
    ring: SharedRing,
    converter: BufferConverter,
    pool: FramePool,
    release: ReleaseQueue,
    next_id: u64,
    // デコーダーより後に手放す。
    _device: DeviceRef,
}

// SAFETY: CUDA の文脈とストリームは FFmpeg が持つ生のハンドルで、このデコーダーを使う 1 つのスレッドだけが触る。
unsafe impl Send for CuvidDecoder {}

impl CuvidDecoder {
    /// 素材を開く。`gpu` はレンダラーと同じデバイスで、D3D12 でなければ `Err(Unsupported)`。
    pub fn open(path: &Path, gpu: &GpuContext) -> Result<Self> {
        let luid = gpu
            .adapter
            .luid
            .ok_or_else(|| MmError::Unsupported("D3D12 のアダプターでない".into()))?;
        let cuda = Arc::new(CudaApi::load()?);
        let cuda_device = cuda.device_for_luid(luid)?;
        let opened = dec::open(path, cuda_device)?;
        let seq = Sequencer::new(path, opened.reader)?;
        let size = seq.info().size;
        let bytes_per_sample = if opened.bits == 8 { 1 } else { 2 };
        // SAFETY: 文脈とストリームは、いま開いたデコーダーの CUDA のもの。
        let ring = unsafe {
            SharedRing::new(
                (gpu.device.clone(), gpu.queue.clone()),
                cuda,
                (opened.cuda_ctx, opened.cuda_stream),
                BufferLayout::new(size.width, size.height, bytes_per_sample),
                RING_SLOTS,
            )?
        };
        Ok(Self {
            converter: BufferConverter::new(gpu.queue.clone(), &ring, opened.bits),
            pool: FramePool::new(gpu.device.clone(), size),
            seq,
            bits: opened.bits,
            ring,
            release: ReleaseQueue::new(),
            next_id: 0,
            _device: opened.device,
        })
    }

    /// デコードした面を GPU の読み終わりを待たずに放す（待ちが効いていることを確かめる対照の検査用）。
    #[cfg(feature = "test-hooks")]
    pub fn set_gpu_wait(&mut self, on: bool) {
        self.release.set_wait_for_gpu(on);
        self.ring.set_wait_for_reader(on);
    }

    /// 握っている面の数。
    pub fn pending_surfaces(&self) -> usize {
        self.release.len()
    }

    fn present(
        &mut self,
        index: i64,
        pts: RationalTime,
        cuda_frame: ffmpeg_next::frame::Video,
    ) -> Result<DecodedFrame> {
        let slot = {
            let _copy = tracing::info_span!("decode.copy").entered();
            self.ring.copy_frame(&cuda_frame)?
        };
        let params = YuvParams::new(
            Matrix::from_frame(cuda_frame.color_space(), cuda_frame.height()),
            is_full_range(cuda_frame.color_range()),
            self.bits,
            1.0,
            (cuda_frame.width(), cuda_frame.height()),
        );
        // コピーを積んだ以上、読み終えの値を必ず進める。待つ側（次のコピー）が止まらないように。
        let wait = self.ring.wait_copy_on(slot);
        let target = self.pool.take();
        self.converter.submit(slot, params, &target);
        let signal = self.ring.signal_read_done(slot);
        wait?;
        signal?;
        let (fence, copy_done) = {
            let s = self.ring.slot(slot);
            (s.fence().clone(), s.copy_done_value())
        };
        self.release.push(fence, copy_done, cuda_frame)?;
        let id = self.next_id;
        self.next_id += 1;
        let handle: GpuFrameHandle = self.pool.lend(target).into_handle(id);
        Ok(DecodedFrame {
            index,
            pts,
            image: None,
            gpu: Some(handle),
        })
    }
}

impl Drop for CuvidDecoder {
    fn drop(&mut self) {
        // CUDA のコピーが終わる前に面と共有資源を壊さない。
        self.release.wait_all();
    }
}

impl Decoder for CuvidDecoder {
    fn info(&self) -> &MediaInfo {
        self.seq.info()
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.seq.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let _span = tracing::info_span!("decode.next_frame", backend = "cuvid").entered();
        self.release.reap();
        let Some(IndexedFrame { index, pts, frame }) = self.seq.next_frame()? else {
            return Ok(None);
        };
        self.present(index, pts, frame).map(Some)
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let frames = (range.duration()?.to_f64() * f64::from(SILENCE_RATE)).round() as usize;
        Ok(AudioBlock::silence(SILENCE_RATE, SILENCE_CHANNELS, frames))
    }
}

/// `DecoderFactory`。デバイスは `open` で渡されるレンダラーのものを使い、自前では作らない。
#[derive(Debug, Clone, Copy, Default)]
pub struct CuvidFactory;

impl DecoderFactory for CuvidFactory {
    fn id(&self) -> &'static str {
        crate::ID
    }

    /// `gpu` が無い、または `mm-gpu` の文脈でなければ `Err(Unsupported)`。
    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        let ctx = gpu
            .and_then(|g| g.as_any().downcast_ref::<GpuContext>())
            .ok_or_else(|| MmError::Unsupported("cuvid は GPU の文脈が要る".into()))?;
        Ok(Box::new(CuvidDecoder::open(path, ctx)?))
    }
}
