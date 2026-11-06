//! FFmpeg の cuvid（NVDEC）でデコードする層。出てくるフレームは `AV_PIX_FMT_CUDA` で、NV12 か P010 が CUDA のメモリにある。

use std::ffi::CString;
use std::path::Path;
use std::sync::Mutex;

use ff::ffi;
use ffmpeg_next as ff;
use mm_types::{MmError, Result};

use crate::cuda::{CuContext, CuStream};
use mm_decode_ffmpeg_core::reader::StreamReader;
use mm_decode_hw::yuv::stream_bit_depth;

/// 選ぶ表面形式は CUDA だけ。cuvid は CPU の NV12 も申し出るが、使うのは GPU の上のフレーム。
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

/// `av_hwdevice_ctx_create` で作った CUDA のデバイスへの参照。捨てるときに手放す。
pub struct DeviceRef(*mut ffi::AVBufferRef);

// SAFETY: 参照は FFmpeg が数えて管理する。持つのは 1 つのデコーダーだけ。
unsafe impl Send for DeviceRef {}

impl Drop for DeviceRef {
    fn drop(&mut self) {
        // SAFETY: open で作った参照。コーデック側の参照はデコーダーが手放す。
        unsafe { ffi::av_buffer_unref(&mut self.0) };
    }
}

/// 開いた素材。
pub struct Opened {
    pub reader: StreamReader,
    /// 1 成分のビット深度（8 か 10）。
    pub bits: u32,
    /// FFmpeg が使う CUDA の文脈。コピーもこの文脈で行う。
    pub cuda_ctx: CuContext,
    pub cuda_stream: CuStream,
    pub device: DeviceRef,
}

/// 素材を開く。H.264 と HEVC の 4:2:0 でなければ `Err(Unsupported)`。`cuda_device` は CUDA のデバイスの番号。
pub fn open(path: &Path, cuda_device: i32) -> Result<Opened> {
    // 同時に開くと FFmpeg のデバイスの初期化が競合しうるので、開く間は直列にする。
    static OPEN_LOCK: Mutex<()> = Mutex::new(());
    let _guard = OPEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let bits = stream_bit_depth(&stream)?;
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
    let time_base = stream.time_base();
    let index = CString::new(cuda_device.to_string()).expect("数字に NUL は無い");
    let mut device_ref: *mut ffi::AVBufferRef = std::ptr::null_mut();
    // SAFETY: av_hwdevice_ctx_create は指定の番号の CUDA を開く。成功したら AVCUDADeviceContext が取れる。
    let (cuda_ctx, cuda_stream) = unsafe {
        let ret = ffi::av_hwdevice_ctx_create(
            &mut device_ref,
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
            index.as_ptr(),
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
        (*p).pkt_timebase = time_base.into();
        ((*cu).cuda_ctx as CuContext, (*cu).stream as CuStream)
    };
    let device = DeviceRef(device_ref);
    let decoder = ctx
        .decoder()
        .open_as(codec)
        .and_then(|o| o.video())
        .map_err(|e| MmError::Unsupported(format!("{name}: {e}")))?;
    Ok(Opened {
        reader: StreamReader::new(input, stream_index, decoder),
        bits,
        cuda_ctx,
        cuda_stream,
        device,
    })
}
