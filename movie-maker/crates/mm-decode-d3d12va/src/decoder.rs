//! `mm-decode-api` の `Decoder` の実装。コマは描画器の wgpu のデバイスの上の `Rgba16Float`
//! （`GpuFrameTexture`）で返す。復号した面は変換の完了までだけ握り、借りが長引いてもプールは尽きない。
//!
//! 音は無音を返す。音は `mm-runtime` が FFmpeg のデコーダーと組み合わせる。

use std::path::Path;

use mm_audio_api::AudioBlock;
use mm_decode_api::{
    DecodedFrame, Decoder, DecoderFactory, GpuContextRef, GpuFrameHandle, MediaInfo,
};
use mm_gpu::{GpuContext, GpuRequest};
use mm_types::{GpuAdapterId, MmError, RationalTime, Result, TimeRange};
use windows::Win32::Graphics::Direct3D12::{ID3D12Fence, D3D12_FENCE_FLAG_NONE};

use crate::convert::YuvConverter;
use crate::hwdec;
use crate::shared::adapter::dx12_request;
use crate::shared::pool::FramePool;
use crate::shared::reader::{IndexedFrame, Sequencer};
use crate::shared::release::ReleaseQueue;
use crate::shared::yuv::{is_full_range, Matrix, YuvParams};
use crate::wrap::Wrapper;

/// 無音を返すときの形式。
const SILENCE_RATE: u32 = 48_000;
const SILENCE_CHANNELS: u16 = 2;

/// 実装の名前。
pub const ID: &str = "d3d12va";

/// この実装が描画器のデバイスに求める機能。`GpuRequest::optional_features` に足す。
pub fn required_features() -> wgpu::Features {
    wgpu::Features::TEXTURE_FORMAT_NV12 | ten_bit_features()
}

/// 10 bit の素材（P010 と、その面を読む 16 bit の形式）に要る機能。
fn ten_bit_features() -> wgpu::Features {
    wgpu::Features::TEXTURE_FORMAT_P010 | wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
}

/// 描画器の GPU の文脈を、d3d12va が使える形（D3D12 のアダプター、必要な機能つき）で頼む。
/// `wanted` が `None` なら D3D12 での高性能のアダプター。
pub fn gpu_request(wanted: Option<&GpuAdapterId>) -> Result<GpuRequest> {
    dx12_request(wanted, required_features())
}

/// d3d12va のデコーダー。
pub struct D3d12vaDecoder {
    seq: Sequencer,
    bits: u32,
    queue: wgpu::Queue,
    wrapper: Wrapper,
    converter: YuvConverter,
    pool: FramePool,
    release: ReleaseQueue,
    /// wgpu のキューが変換を読み終えたことを知らせるフェンス。
    fence: ID3D12Fence,
    fence_value: u64,
    next_id: u64,
}

impl D3d12vaDecoder {
    /// 素材を開く。`gpu` は描画器と同じデバイスで、D3D12 でなければ `Err(Unsupported)`。
    pub fn open(path: &Path, gpu: &GpuContext) -> Result<Self> {
        let features = gpu.device.features();
        if !features.contains(wgpu::Features::TEXTURE_FORMAT_NV12) {
            return Err(MmError::Unsupported(
                "デバイスが NV12 のテクスチャを持たない".into(),
            ));
        }
        let opened = hwdec::open(path, &gpu.device)?;
        if opened.bits == 10 && !features.contains(ten_bit_features()) {
            return Err(MmError::Unsupported(
                "デバイスが P010 と 16 bit のテクスチャを持たない".into(),
            ));
        }
        let fence = create_fence(&gpu.device)?;
        let seq = Sequencer::new(path, opened.reader)?;
        let pool = FramePool::new(gpu.device.clone(), seq.info().size);
        Ok(Self {
            seq,
            bits: opened.bits,
            queue: gpu.queue.clone(),
            wrapper: Wrapper::new(gpu.device.clone()),
            converter: YuvConverter::new(gpu.device.clone(), gpu.queue.clone()),
            pool,
            release: ReleaseQueue::new(),
            fence,
            fence_value: 0,
            next_id: 0,
        })
    }

    /// 復号した面を GPU の読み終わりを待たずに放す（待ちが効いていることを確かめる対照の検査用）。
    pub fn set_gpu_wait(&mut self, on: bool) {
        self.release.set_wait_for_gpu(on);
    }

    /// 握っている面の数。
    pub fn pending_surfaces(&self) -> usize {
        self.release.len()
    }

    /// 包んだ資源の数。
    pub fn resource_count(&self) -> usize {
        self.wrapper.resource_count()
    }

    /// wgpu のキューに、いまの提出までの読み取りが終わったらフェンスを進めさせる。値を返す。
    fn signal_after_submit(&mut self) -> Result<u64> {
        // SAFETY: as_hal の参照はこの呼び出しの間だけ使う。
        let hal = unsafe { self.queue.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        self.fence_value += 1;
        // SAFETY: キューとフェンスは同じデバイスの有効なオブジェクト。
        unsafe { hal.as_raw().Signal(&self.fence, self.fence_value) }
            .map_err(|e| MmError::Internal(format!("フェンスの Signal: {e}")))?;
        Ok(self.fence_value)
    }

    fn present(
        &mut self,
        index: i64,
        pts: RationalTime,
        hw: ffmpeg_next::frame::Video,
    ) -> Result<DecodedFrame> {
        let _wrap = tracing::info_span!("decode.wrap").entered();
        let planes = self.wrapper.wrap(&hw)?;
        let matrix = Matrix::from_frame(hw.color_space(), hw.height());
        let params = YuvParams::new(
            matrix,
            is_full_range(hw.color_range()),
            self.bits,
            YuvParams::texture_code_scale(self.bits),
        );
        planes.wait_decode_on(&self.queue)?;
        let target = self.pool.take();
        self.converter.submit(&planes, params, &target);
        let value = self.signal_after_submit()?;
        self.release.push(self.fence.clone(), value, hw)?;
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

fn create_fence(device: &wgpu::Device) -> Result<ID3D12Fence> {
    // SAFETY: as_hal の参照はこの関数の間だけ使う。
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
        .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
    // SAFETY: 有効なデバイスに、初期値 0 のフェンスを作る。
    unsafe { hal.raw_device().CreateFence(0, D3D12_FENCE_FLAG_NONE) }
        .map_err(|e| MmError::Internal(format!("フェンスの作成: {e}")))
}

impl Drop for D3d12vaDecoder {
    fn drop(&mut self) {
        // GPU が読み終える前に面とデバイスの資源を壊さない。
        self.release.wait_all();
    }
}

impl Decoder for D3d12vaDecoder {
    fn info(&self) -> &MediaInfo {
        self.seq.info()
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.seq.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let _span = tracing::info_span!("decode.next_frame", backend = "d3d12va").entered();
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

/// `DecoderFactory`。デバイスは `open` で渡される描画器のものを使い、自前では作らない。
#[derive(Debug, Clone, Copy, Default)]
pub struct D3d12vaFactory;

impl DecoderFactory for D3d12vaFactory {
    fn id(&self) -> &'static str {
        ID
    }

    /// `gpu` が無い、または `mm-gpu` の文脈でなければ `Err(Unsupported)`（GPU の外には出せない）。
    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        let ctx = gpu
            .and_then(|g| g.as_any().downcast_ref::<GpuContext>())
            .ok_or_else(|| MmError::Unsupported("d3d12va は GPU の文脈が要る".into()))?;
        // 開くだけなら復号しないので安全（壊れた素材はここで Corrupt になる）。落ちる素材は
        // ドライバーが D3D12 のデバイスを全部消すので、最初の復号の前に別プロセスで試す。
        let decoder = D3d12vaDecoder::open(path, ctx)?;
        let luid = ctx
            .adapter
            .luid
            .ok_or_else(|| MmError::Unsupported("D3D12 のアダプターでない".into()))?;
        crate::probe::probe(path, luid)?;
        Ok(Box::new(decoder))
    }
}
