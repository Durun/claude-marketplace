//! FFmpeg の d3d12va で復号する層。wgpu の `ID3D12Device` を FFmpeg に渡し、同じデバイスの上に復号結果を置く。

use std::path::Path;

use ff::ffi;
use ff::format::context::Input;
use ff::{frame, Packet};
use ffmpeg_next as ff;
use mm_types::{MmError, Result};
use windows::core::Interface;

use crate::ffi::{AvD3d12vaDeviceContext, RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS};
use crate::gpu::GpuContext;

/// 復号器が DPB とは別に持つ予備のフレーム数。呼び出し側が手元に残せる枚数の上限になる。
const EXTRA_HW_FRAMES: i32 = 16;

/// FFmpeg を 1 回だけ初期化する。
pub fn init() -> Result<()> {
    ff::init().map_err(|e| MmError::Internal(format!("FFmpeg の初期化: {e}")))
}

/// d3d12va の表面形式を選ぶ。素材が対応しないコーデックなら `AV_PIX_FMT_NONE` を返して失敗させる。
unsafe extern "C" fn pick_d3d12(
    _ctx: *mut ffi::AVCodecContext,
    fmts: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    let mut p = fmts;
    // SAFETY: FFmpeg は AV_PIX_FMT_NONE で終わる配列を渡す。
    unsafe {
        while *p != ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == ffi::AVPixelFormat::AV_PIX_FMT_D3D12 {
                return *p;
            }
            p = p.add(1);
        }
    }
    ffi::AVPixelFormat::AV_PIX_FMT_NONE
}

/// wgpu の D3D12 デバイスを包む `AVHWDeviceContext`。
fn create_device_ref(gpu: &GpuContext) -> Result<*mut ffi::AVBufferRef> {
    // SAFETY: as_hal で得た参照は、このブロックの間だけ使う。raw のデバイスは AddRef してから FFmpeg に渡す。
    unsafe {
        let hal = gpu
            .device
            .as_hal::<wgpu::hal::api::Dx12>()
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        let raw = hal.raw_device().clone();
        let mut device_ref =
            ffi::av_hwdevice_ctx_alloc(ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D12VA);
        if device_ref.is_null() {
            return Err(MmError::Unsupported(
                "d3d12va のデバイスを確保できない".into(),
            ));
        }
        let device_ctx = (*device_ref).data as *mut ffi::AVHWDeviceContext;
        let hw = (*device_ctx).hwctx as *mut AvD3d12vaDeviceContext;
        (*hw).device = raw.into_raw();
        (*hw).resource_flags = RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS;
        let ret = ffi::av_hwdevice_ctx_init(device_ref);
        if ret < 0 {
            ffi::av_buffer_unref(&mut device_ref);
            return Err(MmError::Unsupported(format!(
                "d3d12va のデバイスを初期化できない: {}",
                ff::Error::from(ret)
            )));
        }
        Ok(device_ref)
    }
}

/// d3d12va で復号する素材 1 本。出てくるコマは `AV_PIX_FMT_D3D12` で、`data[0]` が `AVD3D12VAFrame`。
pub struct HwDecoder {
    pub input: Input,
    stream_index: usize,
    pub decoder: ff::decoder::Video,
    draining: bool,
}

impl HwDecoder {
    /// 素材を開く。コーデックが d3d12va に対応しなければ `Err(Unsupported)`。
    pub fn open(path: &Path, gpu: &GpuContext) -> Result<Self> {
        if !path.exists() {
            return Err(MmError::NotFound(path.display().to_string()));
        }
        init()?;
        let input = ff::format::input(&path)
            .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| MmError::Unsupported("映像が無い".into()))?;
        let stream_index = stream.index();
        let mut ctx = ff::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| MmError::Unsupported(format!("映像のコーデック: {e}")))?;
        let mut device_ref = create_device_ref(gpu)?;
        // SAFETY: open の前に、所有する AVCodecContext の欄を埋める。device_ref は av_buffer_ref で別に参照を取る。
        unsafe {
            let p = ctx.as_mut_ptr();
            (*p).hw_device_ctx = ffi::av_buffer_ref(device_ref);
            (*p).get_format = Some(pick_d3d12);
            (*p).extra_hw_frames = EXTRA_HW_FRAMES;
            ffi::av_buffer_unref(&mut device_ref);
        }
        let decoder = ctx
            .decoder()
            .video()
            .map_err(|e| MmError::Unsupported(format!("映像のデコーダー: {e}")))?;
        Ok(Self {
            input,
            stream_index,
            decoder,
            draining: false,
        })
    }

    /// 映像ストリーム。
    pub fn stream(&self) -> ff::format::stream::Stream<'_> {
        self.input
            .stream(self.stream_index)
            .expect("開いた映像ストリーム")
    }

    /// 次の復号済みのコマ。終わりなら `None`。
    pub fn next_hw_frame(&mut self) -> Result<Option<frame::Video>> {
        let mut packet = Packet::empty();
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => return Ok(Some(decoded)),
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => {}
                Err(e) => return Err(MmError::Corrupt(format!("映像の復号: {e}"))),
            }
            if self.draining {
                return Ok(None);
            }
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.stream_index => {
                    match self.decoder.send_packet(&packet) {
                        Ok(()) | Err(ff::Error::InvalidData) => {}
                        Err(e) => return Err(MmError::Corrupt(format!("映像のパケット: {e}"))),
                    }
                }
                Ok(()) | Err(ff::Error::InvalidData) => {}
                Err(ff::Error::Eof) => {
                    let _ = self.decoder.send_eof();
                    self.draining = true;
                }
                Err(e) => return Err(MmError::Corrupt(format!("映像の読み込み: {e}"))),
            }
        }
    }

    /// 先頭から読み直す。
    pub fn rewind(&mut self) -> Result<()> {
        self.input
            .seek(0, ..1)
            .map_err(|e| MmError::Corrupt(format!("シーク: {e}")))?;
        self.decoder.flush();
        self.draining = false;
        Ok(())
    }

    /// GPU の上のコマを CPU の NV12 にコピーして降ろす（比較用の経路）。
    pub fn transfer_to_cpu(hw: &frame::Video) -> Result<frame::Video> {
        let mut sw = frame::Video::empty();
        // SAFETY: どちらも有効な AVFrame。sw は空で、FFmpeg が形式と領域を決める。
        let ret = unsafe { ffi::av_hwframe_transfer_data(sw.as_mut_ptr(), hw.as_ptr(), 0) };
        if ret < 0 {
            return Err(MmError::Internal(format!(
                "av_hwframe_transfer_data: {}",
                ff::Error::from(ret)
            )));
        }
        Ok(sw)
    }
}
