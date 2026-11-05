//! `libavutil/hwcontext_d3d12va.h` の構造体。
//!
//! bindgen は D3D12 の SDK ヘッダーを見つけられない環境があるため、使う欄だけ手で宣言する。
//! 並びと大きさはヘッダーと同じ（x86_64 の Windows で確認）。

use std::ffi::c_void;

/// `D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS`。
/// 復号に使ったキューと wgpu のキューが、バリアなしで同じ資源を読めるようにする。
pub const RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS: u32 = 0x20;

/// `DXGI_FORMAT_NV12`。
pub const DXGI_FORMAT_NV12: u32 = 103;

#[repr(C)]
pub struct AvD3d12vaDeviceContext {
    pub device: *mut c_void,
    pub video_device: *mut c_void,
    pub lock: Option<unsafe extern "C" fn(*mut c_void)>,
    pub unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    pub lock_ctx: *mut c_void,
    pub resource_flags: u32,
    pub heap_flags: u32,
}

#[repr(C)]
pub struct AvD3d12vaSyncContext {
    pub fence: *mut c_void,
    pub event: *mut c_void,
    pub fence_value: u64,
}

#[repr(C)]
pub struct AvD3d12vaFrame {
    pub texture: *mut c_void,
    pub subresource_index: i32,
    pub sync_ctx: AvD3d12vaSyncContext,
    pub flags: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn ヘッダーの大きさと一致する() {
        assert_eq!(size_of::<AvD3d12vaSyncContext>(), 24);
        assert_eq!(size_of::<AvD3d12vaFrame>(), 48);
        assert_eq!(size_of::<AvD3d12vaDeviceContext>(), 48);
    }
}
