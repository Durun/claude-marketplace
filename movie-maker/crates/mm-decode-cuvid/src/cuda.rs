//! CUDA ドライバー API の必要な関数だけ。`nvcuda.dll` を実行時に読むので、CUDA Toolkit が無くてもビルドできる。

use std::ffi::c_void;

use mm_types::{MmError, Result};
use windows::core::s;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

pub type CuResult = i32;
pub type CuContext = *mut c_void;
pub type CuStream = *mut c_void;
pub type CuDevicePtr = u64;
pub type CuExternalMemory = *mut c_void;
pub type CuExternalSemaphore = *mut c_void;

/// `CU_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE`。
pub const EXTMEM_D3D12_RESOURCE: u32 = 5;
/// `CUDA_EXTERNAL_MEMORY_DEDICATED`。コミット済みの D3D12 資源に必須。
pub const EXTMEM_DEDICATED: u32 = 1;
/// `CU_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE`。
pub const EXTSEM_D3D12_FENCE: u32 = 4;
/// `CU_MEMORYTYPE_DEVICE`。
pub const MEMORYTYPE_DEVICE: u32 = 2;

/// `CUDA_EXTERNAL_MEMORY_HANDLE_DESC`。共用体は win32 の 2 ポインタ分で表す。
#[repr(C)]
pub struct ExternalMemoryHandleDesc {
    pub kind: u32,
    pub handle: *mut c_void,
    pub name: *const c_void,
    pub size: u64,
    pub flags: u32,
    pub reserved: [u32; 16],
}

#[repr(C)]
pub struct ExternalMemoryBufferDesc {
    pub offset: u64,
    pub size: u64,
    pub flags: u32,
    pub reserved: [u32; 16],
}

#[repr(C)]
pub struct ExternalSemaphoreHandleDesc {
    pub kind: u32,
    pub handle: *mut c_void,
    pub name: *const c_void,
    pub flags: u32,
    pub reserved: [u32; 16],
}

/// `CUDA_EXTERNAL_SEMAPHORE_SIGNAL_PARAMS`。フェンスの値だけを使う。
#[repr(C)]
pub struct ExternalSemaphoreSignalParams {
    pub fence_value: u64,
    pub nvsci_sync: u64,
    pub keyed_mutex_key: u64,
    pub params_reserved: [u32; 12],
    pub flags: u32,
    pub reserved: [u32; 16],
}

/// `CUDA_EXTERNAL_SEMAPHORE_WAIT_PARAMS`。フェンスの値だけを使う。
#[repr(C)]
pub struct ExternalSemaphoreWaitParams {
    pub fence_value: u64,
    pub nvsci_sync: u64,
    pub keyed_mutex_key: u64,
    pub keyed_mutex_timeout_ms: u32,
    pub params_reserved: [u32; 11],
    pub flags: u32,
    pub reserved: [u32; 16],
}

/// `CUDA_MEMCPY2D`。
#[repr(C)]
pub struct Memcpy2D {
    pub src_x_in_bytes: usize,
    pub src_y: usize,
    pub src_memory_type: u32,
    pub src_host: *const c_void,
    pub src_device: CuDevicePtr,
    pub src_array: *mut c_void,
    pub src_pitch: usize,
    pub dst_x_in_bytes: usize,
    pub dst_y: usize,
    pub dst_memory_type: u32,
    pub dst_host: *mut c_void,
    pub dst_device: CuDevicePtr,
    pub dst_array: *mut c_void,
    pub dst_pitch: usize,
    pub width_in_bytes: usize,
    pub height: usize,
}

type FnPush = unsafe extern "system" fn(CuContext) -> CuResult;
type FnPop = unsafe extern "system" fn(*mut CuContext) -> CuResult;
type FnImportMem =
    unsafe extern "system" fn(*mut CuExternalMemory, *const ExternalMemoryHandleDesc) -> CuResult;
type FnMappedBuffer = unsafe extern "system" fn(
    *mut CuDevicePtr,
    CuExternalMemory,
    *const ExternalMemoryBufferDesc,
) -> CuResult;
type FnMemcpy2D = unsafe extern "system" fn(*const Memcpy2D, CuStream) -> CuResult;
type FnImportSem = unsafe extern "system" fn(
    *mut CuExternalSemaphore,
    *const ExternalSemaphoreHandleDesc,
) -> CuResult;
type FnSignal = unsafe extern "system" fn(
    *const CuExternalSemaphore,
    *const ExternalSemaphoreSignalParams,
    u32,
    CuStream,
) -> CuResult;
type FnWait = unsafe extern "system" fn(
    *const CuExternalSemaphore,
    *const ExternalSemaphoreWaitParams,
    u32,
    CuStream,
) -> CuResult;
type FnFree = unsafe extern "system" fn(CuDevicePtr) -> CuResult;
type FnDestroyMem = unsafe extern "system" fn(CuExternalMemory) -> CuResult;
type FnDestroySem = unsafe extern "system" fn(CuExternalSemaphore) -> CuResult;
type FnInit = unsafe extern "system" fn(u32) -> CuResult;
type FnDeviceCount = unsafe extern "system" fn(*mut i32) -> CuResult;
type FnDeviceLuid = unsafe extern "system" fn(*mut u8, *mut u32, i32) -> CuResult;
type FnSync = unsafe extern "system" fn(CuStream) -> CuResult;

/// 読み込んだ関数の表。
pub struct CudaApi {
    push: FnPush,
    pop: FnPop,
    import_mem: FnImportMem,
    mapped_buffer: FnMappedBuffer,
    memcpy2d: FnMemcpy2D,
    import_sem: FnImportSem,
    signal: FnSignal,
    wait: FnWait,
    free: FnFree,
    destroy_mem: FnDestroyMem,
    destroy_sem: FnDestroySem,
    init: FnInit,
    device_count: FnDeviceCount,
    device_luid: FnDeviceLuid,
    sync: FnSync,
}

macro_rules! load {
    ($lib:expr, $name:literal, $ty:ty) => {{
        // SAFETY: 名前は CUDA ドライバー API の公開関数で、型は cuda.h の宣言どおり。
        let p = unsafe { GetProcAddress($lib, windows::core::PCSTR(concat!($name, "\0").as_ptr())) }
            .ok_or_else(|| MmError::Unsupported(concat!("nvcuda に ", $name, " が無い").into()))?;
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(p) }
    }};
}

fn check(what: &str, r: CuResult) -> Result<()> {
    if r == 0 {
        Ok(())
    } else {
        Err(MmError::Internal(format!("{what}: CUresult {r}")))
    }
}

impl CudaApi {
    /// `nvcuda.dll` から読む。
    pub fn load() -> Result<Self> {
        // SAFETY: NVIDIA のドライバーが入れる DLL を名前で読む。
        let lib = unsafe { LoadLibraryA(s!("nvcuda.dll")) }
            .map_err(|e| MmError::Unsupported(format!("nvcuda.dll を読めない: {e}")))?;
        Ok(Self {
            push: load!(lib, "cuCtxPushCurrent_v2", FnPush),
            pop: load!(lib, "cuCtxPopCurrent_v2", FnPop),
            import_mem: load!(lib, "cuImportExternalMemory", FnImportMem),
            mapped_buffer: load!(lib, "cuExternalMemoryGetMappedBuffer", FnMappedBuffer),
            memcpy2d: load!(lib, "cuMemcpy2DAsync_v2", FnMemcpy2D),
            import_sem: load!(lib, "cuImportExternalSemaphore", FnImportSem),
            signal: load!(lib, "cuSignalExternalSemaphoresAsync", FnSignal),
            wait: load!(lib, "cuWaitExternalSemaphoresAsync", FnWait),
            free: load!(lib, "cuMemFree_v2", FnFree),
            destroy_mem: load!(lib, "cuDestroyExternalMemory", FnDestroyMem),
            destroy_sem: load!(lib, "cuDestroyExternalSemaphore", FnDestroySem),
            init: load!(lib, "cuInit", FnInit),
            device_count: load!(lib, "cuDeviceGetCount", FnDeviceCount),
            device_luid: load!(lib, "cuDeviceGetLuid", FnDeviceLuid),
            sync: load!(lib, "cuStreamSynchronize", FnSync),
        })
    }

    /// 文脈をこのスレッドの現在にする。
    ///
    /// # Safety
    /// `ctx` は有効な CUDA の文脈。
    pub unsafe fn push(&self, ctx: CuContext) -> Result<()> {
        check("cuCtxPushCurrent", unsafe { (self.push)(ctx) })
    }

    /// 現在の文脈を外す。
    ///
    /// # Safety
    /// 対応する `push` の後に呼ぶ。
    pub unsafe fn pop(&self) -> Result<()> {
        let mut out = std::ptr::null_mut();
        check("cuCtxPopCurrent", unsafe { (self.pop)(&mut out) })
    }

    /// D3D12 の共有ハンドルの資源を CUDA のメモリとして取り込み、全体を 1 本の線形バッファに写す。
    ///
    /// # Safety
    /// 文脈が現在で、`handle` は `size` バイトの共有バッファの NT ハンドル。
    pub unsafe fn import_buffer(
        &self,
        handle: *mut c_void,
        size: u64,
    ) -> Result<(CuExternalMemory, CuDevicePtr)> {
        let desc = ExternalMemoryHandleDesc {
            kind: EXTMEM_D3D12_RESOURCE,
            handle,
            name: std::ptr::null(),
            size,
            flags: EXTMEM_DEDICATED,
            reserved: [0; 16],
        };
        let mut mem = std::ptr::null_mut();
        check("cuImportExternalMemory", unsafe {
            (self.import_mem)(&mut mem, &desc)
        })?;
        let buf = ExternalMemoryBufferDesc {
            offset: 0,
            size,
            flags: 0,
            reserved: [0; 16],
        };
        let mut ptr = 0;
        check("cuExternalMemoryGetMappedBuffer", unsafe {
            (self.mapped_buffer)(&mut ptr, mem, &buf)
        })?;
        Ok((mem, ptr))
    }

    /// D3D12 のフェンスを CUDA の外部セマフォとして取り込む。
    ///
    /// # Safety
    /// 文脈が現在で、`handle` は共有フェンスの NT ハンドル。
    pub unsafe fn import_fence(&self, handle: *mut c_void) -> Result<CuExternalSemaphore> {
        let desc = ExternalSemaphoreHandleDesc {
            kind: EXTSEM_D3D12_FENCE,
            handle,
            name: std::ptr::null(),
            flags: 0,
            reserved: [0; 16],
        };
        let mut sem = std::ptr::null_mut();
        check("cuImportExternalSemaphore", unsafe {
            (self.import_sem)(&mut sem, &desc)
        })?;
        Ok(sem)
    }

    /// 装置内の 2D コピーをストリームに積む。
    ///
    /// # Safety
    /// 文脈が現在で、`copy` の両端が有効なデバイスメモリ。
    pub unsafe fn memcpy2d_async(&self, copy: &Memcpy2D, stream: CuStream) -> Result<()> {
        check("cuMemcpy2DAsync", unsafe { (self.memcpy2d)(copy, stream) })
    }

    /// ストリームのここまでが終わったら、フェンスを `value` にする。
    ///
    /// # Safety
    /// 文脈が現在で、`sem` は取り込み済み。
    pub unsafe fn signal(
        &self,
        sem: CuExternalSemaphore,
        value: u64,
        stream: CuStream,
    ) -> Result<()> {
        let params = ExternalSemaphoreSignalParams {
            fence_value: value,
            nvsci_sync: 0,
            keyed_mutex_key: 0,
            params_reserved: [0; 12],
            flags: 0,
            reserved: [0; 16],
        };
        check("cuSignalExternalSemaphoresAsync", unsafe {
            (self.signal)(&sem, &params, 1, stream)
        })
    }

    /// ストリームのここから先を、フェンスが `value` に届くまで止める（GPU の中で待ち、CPU は待たない）。
    ///
    /// # Safety
    /// 文脈が現在で、`sem` は取り込み済み。
    pub unsafe fn wait(
        &self,
        sem: CuExternalSemaphore,
        value: u64,
        stream: CuStream,
    ) -> Result<()> {
        let params = ExternalSemaphoreWaitParams {
            fence_value: value,
            nvsci_sync: 0,
            keyed_mutex_key: 0,
            keyed_mutex_timeout_ms: 0,
            params_reserved: [0; 11],
            flags: 0,
            reserved: [0; 16],
        };
        check("cuWaitExternalSemaphoresAsync", unsafe {
            (self.wait)(&sem, &params, 1, stream)
        })
    }

    /// 取り込んだ外部メモリと、そこに写したポインタ、取り込んだセマフォを手放す。失敗は無視する（後片付け）。
    ///
    /// # Safety
    /// 文脈が現在で、引数はその文脈で取り込んだもの。GPU の仕事が終わっている。
    pub unsafe fn release_import(
        &self,
        mem: CuExternalMemory,
        ptr: CuDevicePtr,
        sem: CuExternalSemaphore,
    ) {
        unsafe {
            (self.destroy_sem)(sem);
            (self.free)(ptr);
            (self.destroy_mem)(mem);
        }
    }

    /// LUID のアダプターに当たる CUDA のデバイスの番号。
    pub fn device_for_luid(&self, luid: u64) -> Result<i32> {
        // SAFETY: 引数はどれも有効な出力先。
        unsafe {
            check("cuInit", (self.init)(0))?;
            let mut count = 0;
            check("cuDeviceGetCount", (self.device_count)(&mut count))?;
            for dev in 0..count {
                let (mut bytes, mut mask) = ([0u8; 8], 0u32);
                check(
                    "cuDeviceGetLuid",
                    (self.device_luid)(bytes.as_mut_ptr(), &mut mask, dev),
                )?;
                if u64::from_le_bytes(bytes) == luid {
                    return Ok(dev);
                }
            }
        }
        Err(MmError::Unsupported(
            "この GPU に当たる CUDA のデバイスが無い".into(),
        ))
    }

    /// ストリームの完了を待つ（比較用）。
    ///
    /// # Safety
    /// 文脈が現在。
    pub unsafe fn sync(&self, stream: CuStream) -> Result<()> {
        check("cuStreamSynchronize", unsafe { (self.sync)(stream) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn cuda_h_の大きさと一致する() {
        assert_eq!(size_of::<ExternalMemoryHandleDesc>(), 104);
        assert_eq!(size_of::<ExternalMemoryBufferDesc>(), 88);
        assert_eq!(size_of::<ExternalSemaphoreHandleDesc>(), 96);
        assert_eq!(size_of::<ExternalSemaphoreSignalParams>(), 144);
        assert_eq!(size_of::<ExternalSemaphoreWaitParams>(), 144);
        assert_eq!(size_of::<Memcpy2D>(), 128);
    }
}
