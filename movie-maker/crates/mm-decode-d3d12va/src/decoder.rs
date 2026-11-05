//! `mm-decode-api` の `Decoder` の実装（試作）。コマは GPU のテクスチャの参照で返す。
//!
//! 試作の範囲: 固定フレームレートだけを扱い、音は無音を返す。`next_frame` が返す
//! `GpuFrameHandle` の中身は [`D3d12vaDecoder::take_frame`] で引く。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ffmpeg_next::frame;
use mm_audio_api::AudioBlock;
use mm_decode_api::{
    DecodedFrame, Decoder, DecoderFactory, GpuContextRef, GpuFrameHandle, MediaInfo,
};
use mm_types::{ColorSpace, MmError, Rational, RationalTime, Result, Size, TimeRange};

use crate::gpu::GpuContext;
use crate::hwdec::HwDecoder;
use crate::wrap::{GpuNv12, Wrapper};

/// 無音を返すときの形式。
const SILENCE_RATE: u32 = 48_000;
const SILENCE_CHANNELS: u16 = 2;

/// 口の取っ手の中身。試作ではテクスチャの実体は [`D3d12vaDecoder::take_frame`] で番号から引く。
pub struct HeldFrameRef {
    pub id: u64,
}

impl mm_decode_api::GpuFrame for HeldFrameRef {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// 手元に残した、まだ使われていないコマ。FFmpeg のプールに返さないよう復号結果を握る。
pub struct HeldFrame {
    pub nv12: GpuNv12,
    _hw: frame::Video,
}

/// d3d12va のデコーダー。
pub struct D3d12vaDecoder {
    gpu: Arc<GpuContext>,
    hw: HwDecoder,
    wrapper: Wrapper,
    info: MediaInfo,
    fps: Rational,
    start_ticks: i64,
    tick_seconds: (i64, i64),
    next_id: u64,
    held: HashMap<u64, HeldFrame>,
    pending: Option<(i64, frame::Video)>,
}

impl D3d12vaDecoder {
    /// 素材を開く。
    pub fn open(path: &Path, gpu: Arc<GpuContext>) -> Result<Self> {
        let hw = HwDecoder::open(path, &gpu)?;
        let (info, fps, start_ticks, tick_seconds) = {
            let stream = hw.stream();
            let avg = stream.avg_frame_rate();
            let fps = Rational::new(avg.numerator() as u32, avg.denominator() as u32)
                .map_err(|e| MmError::Unsupported(format!("フレームレート: {e}")))?;
            let tb = stream.time_base();
            let start = if stream.start_time() == i64::MIN {
                0
            } else {
                stream.start_time()
            };
            let duration_s = stream.duration().max(0) * i64::from(tb.numerator())
                / i64::from(tb.denominator().max(1));
            let size = Size::new(hw.decoder.width(), hw.decoder.height());
            let info = MediaInfo {
                duration: RationalTime::seconds(duration_s),
                fps: Some(fps),
                size,
                color_space: ColorSpace::bt709(),
                timebase_offset: RationalTime::ZERO,
                audio: None,
                pts_index: Vec::new(),
            };
            (
                info,
                fps,
                start,
                (
                    i64::from(tb.numerator()),
                    i64::from(tb.denominator().max(1)),
                ),
            )
        };
        Ok(Self {
            wrapper: Wrapper::new(gpu.clone()),
            gpu,
            hw,
            info,
            fps,
            start_ticks,
            tick_seconds,
            next_id: 0,
            held: HashMap::new(),
            pending: None,
        })
    }

    /// `next_frame` が返した参照のコマを取り出す。取り出したら呼び出し側が持つ。
    pub fn take_frame(&mut self, handle: &GpuFrameHandle) -> Option<HeldFrame> {
        self.held.remove(&handle.id)
    }

    fn index_of(&self, pts: i64) -> i64 {
        let ticks = pts - self.start_ticks;
        let seconds = ticks as f64 * self.tick_seconds.0 as f64 / self.tick_seconds.1 as f64;
        (seconds * f64::from(self.fps.num()) / f64::from(self.fps.den())).round() as i64
    }

    fn next_decoded(&mut self) -> Result<Option<(i64, frame::Video)>> {
        if let Some(p) = self.pending.take() {
            return Ok(Some(p));
        }
        let Some(hw) = self.hw.next_hw_frame()? else {
            return Ok(None);
        };
        let pts = hw.timestamp().or(hw.pts()).unwrap_or(self.start_ticks);
        Ok(Some((self.index_of(pts), hw)))
    }
}

impl Decoder for D3d12vaDecoder {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        let target = t.frame_index(self.fps);
        self.held.clear();
        self.pending = None;
        let av_time = (t.to_f64() * 1_000_000.0) as i64;
        self.hw
            .input
            .seek(av_time, ..av_time + 1)
            .map_err(|e| MmError::Corrupt(format!("シーク: {e}")))?;
        self.hw.decoder.flush();
        // キーフレームから目的のコマまで復号して捨てる。
        while let Some((index, hw)) = self.next_decoded()? {
            if index >= target {
                self.pending = Some((index, hw));
                break;
            }
        }
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let _span = tracing::info_span!("decode.next_frame", backend = "d3d12va").entered();
        let Some((index, hw)) = self.next_decoded()? else {
            return Ok(None);
        };
        let nv12 = {
            let _wrap = tracing::info_span!("decode.wrap").entered();
            self.wrapper.wrap(&hw)?
        };
        let id = self.next_id;
        self.next_id += 1;
        let handle = GpuFrameHandle {
            id,
            size: Size::new(nv12.width, nv12.height),
            frame: Arc::new(HeldFrameRef { id }),
        };
        self.held.insert(id, HeldFrame { nv12, _hw: hw });
        Ok(Some(DecodedFrame {
            index,
            pts: RationalTime::from_frames(index, self.fps)?,
            image: None,
            gpu: Some(handle),
        }))
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let frames = (range.duration()?.to_f64() * f64::from(SILENCE_RATE)).round() as usize;
        Ok(AudioBlock::silence(SILENCE_RATE, SILENCE_CHANNELS, frames))
    }
}

impl D3d12vaDecoder {
    /// この復号器が使う wgpu の文脈。
    pub fn gpu(&self) -> &Arc<GpuContext> {
        &self.gpu
    }
}

/// `DecoderFactory`。wgpu の文脈を 1 つ持ち、開いた復号器で共有する。
pub struct D3d12vaFactory {
    gpu: Arc<GpuContext>,
}

impl D3d12vaFactory {
    /// 既存の文脈で。
    pub fn new(gpu: Arc<GpuContext>) -> Self {
        Self { gpu }
    }
}

impl DecoderFactory for D3d12vaFactory {
    fn id(&self) -> &'static str {
        "d3d12va"
    }

    /// 試作は自前の D3D12 の文脈を使う。口の `gpu` と同じデバイスにそろえるのは段階 1。
    fn open(&self, path: &Path, _gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        Ok(Box::new(D3d12vaDecoder::open(path, self.gpu.clone())?))
    }
}
