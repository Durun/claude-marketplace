//! FFmpeg の d3d12va で復号する層。wgpu の `ID3D12Device` を FFmpeg に渡し、同じデバイスの上に復号結果を置く。

use std::path::Path;
use std::sync::Mutex;

use ff::ffi;
use ffmpeg_next as ff;
use mm_types::{MmError, Result};
use windows::core::Interface;

use crate::ffi::{AvD3d12vaDeviceContext, RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS};
use crate::shared::reader::HwReader;

/// 復号器が DPB とは別に持つ予備のフレーム数。`ReleaseQueue` が握る面の数より大きくする。
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
fn create_device_ref(device: &wgpu::Device) -> Result<*mut ffi::AVBufferRef> {
    // SAFETY: as_hal で得た参照は、このブロックの間だけ使う。raw のデバイスは AddRef してから FFmpeg に渡す。
    unsafe {
        let hal = device
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

/// 素材のビット深度。d3d12va の面（NV12 と P010）に載る 4:2:0 の 8 bit と 10 bit だけ。
fn bit_depth(stream: &ff::format::stream::Stream<'_>) -> Result<u32> {
    // SAFETY: パラメーターは有効なストリームのもの。format は AVPixelFormat の整数値。
    let raw = unsafe { (*stream.parameters().as_ptr()).format };
    let is = |f: ffi::AVPixelFormat| raw == f as i32;
    if is(ffi::AVPixelFormat::AV_PIX_FMT_YUV420P) || is(ffi::AVPixelFormat::AV_PIX_FMT_YUVJ420P) {
        Ok(8)
    } else if is(ffi::AVPixelFormat::AV_PIX_FMT_YUV420P10LE) {
        Ok(10)
    } else {
        Err(MmError::Unsupported(format!(
            "d3d12va は 4:2:0 の 8 bit と 10 bit だけ（画素形式 {raw}）"
        )))
    }
}

/// 開いた素材。
pub struct Opened {
    pub reader: HwReader,
    /// 1 成分のビット深度（8 か 10）。
    pub bits: u32,
}

/// 素材を開く。H.264 と HEVC の 4:2:0 でなければ `Err(Unsupported)`。
pub fn open(path: &Path, device: &wgpu::Device) -> Result<Opened> {
    // 同じ ID3D12Device に FFmpeg のデバイスを同時に作ると初期化が EINVAL で落ちるので、開く間は直列にする。
    static OPEN_LOCK: Mutex<()> = Mutex::new(());
    let _guard = OPEN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let bits = bit_depth(&stream)?;
    let mut ctx = ff::codec::Context::from_parameters(stream.parameters())
        .map_err(|e| MmError::Unsupported(format!("映像のコーデック: {e}")))?;
    if !matches!(ctx.id(), ff::codec::Id::H264 | ff::codec::Id::HEVC) {
        return Err(MmError::Unsupported(format!(
            "d3d12va は {:?} を試さない",
            ctx.id()
        )));
    }
    let mut device_ref = create_device_ref(device)?;
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
    Ok(Opened {
        reader: HwReader::new(input, stream_index, decoder),
        bits,
    })
}
