//! FFmpeg の cuvid（NVDEC）で復号する層。出てくるコマは `AV_PIX_FMT_CUDA` で、NV12 が CUDA のメモリにある。

use std::path::Path;

use ff::ffi;
use ff::format::context::Input;
use ff::{frame, Packet};
use ffmpeg_next as ff;
use mm_types::{MmError, Result};

use crate::cuda::{CuContext, CuStream};

/// 選ぶ表面形式は CUDA だけ。cuvid は CPU の NV12 も申し出るが、使うのは GPU の上のコマ。
unsafe extern "C" fn pick_cuda(
    _ctx: *mut ffi::AVCodecContext,
    fmts: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    let mut p = fmts;
    // SAFETY: FFmpeg は AV_PIX_FMT_NONE で終わる配列を渡す。
    unsafe {
        while *p != ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == ffi::AVPixelFormat::AV_PIX_FMT_CUDA {
                return *p;
            }
            p = p.add(1);
        }
    }
    ffi::AVPixelFormat::AV_PIX_FMT_NONE
}

/// cuvid で復号する素材 1 本。
pub struct CuvidDecoder {
    input: Input,
    stream_index: usize,
    pub decoder: ff::decoder::Video,
    draining: bool,
    device_ref: *mut ffi::AVBufferRef,
    /// FFmpeg が使う CUDA の文脈。コピーもこの文脈で行う。
    pub cuda_ctx: CuContext,
    pub cuda_stream: CuStream,
}

// SAFETY: device_ref と文脈は FFmpeg が参照を数えて管理する。デコーダーは 1 スレッドが使う。
unsafe impl Send for CuvidDecoder {}

impl CuvidDecoder {
    /// 素材を開く。H.264 と HEVC だけ。
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(MmError::NotFound(path.display().to_string()));
        }
        ff::init().map_err(|e| MmError::Internal(format!("FFmpeg の初期化: {e}")))?;
        let input = ff::format::input(&path)
            .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| MmError::Unsupported("映像が無い".into()))?;
        let stream_index = stream.index();
        let mut ctx = ff::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| MmError::Unsupported(format!("映像のコーデック: {e}")))?;
        let name = match ctx.id() {
            ff::codec::Id::H264 => "h264_cuvid",
            ff::codec::Id::HEVC => "hevc_cuvid",
            other => {
                return Err(MmError::Unsupported(format!(
                    "cuvid は {other:?} を試さない"
                )))
            }
        };
        let codec = ff::decoder::find_by_name(name)
            .ok_or_else(|| MmError::Unsupported(format!("{name} が無い")))?;

        let mut device_ref: *mut ffi::AVBufferRef = std::ptr::null_mut();
        // SAFETY: av_hwdevice_ctx_create は CUDA の 0 番を開く。成功したら AVCUDADeviceContext が取れる。
        let (cuda_ctx, cuda_stream) = unsafe {
            let ret = ffi::av_hwdevice_ctx_create(
                &mut device_ref,
                ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                c"0".as_ptr(),
                std::ptr::null_mut(),
                0,
            );
            if ret < 0 {
                return Err(MmError::Unsupported(format!(
                    "CUDA のデバイスを開けない: {}",
                    ff::Error::from(ret)
                )));
            }
            let dev = (*device_ref).data as *mut ffi::AVHWDeviceContext;
            let cu = (*dev).hwctx as *mut ffi::AVCUDADeviceContext;
            let p = ctx.as_mut_ptr();
            (*p).hw_device_ctx = ffi::av_buffer_ref(device_ref);
            (*p).get_format = Some(pick_cuda);
            ((*cu).cuda_ctx as CuContext, (*cu).stream as CuStream)
        };
        let decoder = ctx
            .decoder()
            .open_as(codec)
            .and_then(|o| o.video())
            .map_err(|e| MmError::Unsupported(format!("{name}: {e}")))?;
        Ok(Self {
            input,
            stream_index,
            decoder,
            draining: false,
            device_ref,
            cuda_ctx,
            cuda_stream,
        })
    }

    /// 次の復号済みのコマ。終わりなら `None`。
    pub fn next_cuda_frame(&mut self) -> Result<Option<frame::Video>> {
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
}

impl Drop for CuvidDecoder {
    fn drop(&mut self) {
        // SAFETY: open で av_hwdevice_ctx_create した参照。コーデック側の参照は decoder が手放す。
        unsafe { ffi::av_buffer_unref(&mut self.device_ref) };
    }
}
