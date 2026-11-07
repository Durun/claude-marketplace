//! OpenFX 1.5 の C API のうち、このホストが使う部分。構造体の並びは `vendor/openfx/include` の公式のヘッダーに合わせ、
//! 大きさは C の側（`csrc/shim.c` の `mm_ofx_layout`）と試験で突き合わせる。
//! ハンドルは全部 `*mut c_void` で受け、中身はホストの Rust の構造体（`props::PropSet` など）を指す。

use std::ffi::{c_char, c_int, c_uint, c_void};

pub type OfxStatus = c_int;
pub type Handle = *mut c_void;

pub const STAT_OK: OfxStatus = 0;
pub const STAT_FAILED: OfxStatus = 1;
pub const STAT_ERR_UNKNOWN: OfxStatus = 3;
pub const STAT_ERR_UNSUPPORTED: OfxStatus = 5;
pub const STAT_ERR_EXISTS: OfxStatus = 6;
pub const STAT_ERR_MEMORY: OfxStatus = 8;
pub const STAT_ERR_BAD_HANDLE: OfxStatus = 9;
pub const STAT_ERR_BAD_INDEX: OfxStatus = 10;
pub const STAT_ERR_VALUE: OfxStatus = 11;
pub const STAT_REPLY_DEFAULT: OfxStatus = 14;

#[repr(C)]
pub struct OfxHost {
    pub host: Handle,
    pub fetch_suite: unsafe extern "C" fn(Handle, *const c_char, c_int) -> *const c_void,
}

pub type MainEntry =
    unsafe extern "C" fn(*const c_char, *const c_void, Handle, Handle) -> OfxStatus;

#[repr(C)]
pub struct OfxPlugin {
    pub plugin_api: *const c_char,
    pub api_version: c_int,
    pub plugin_identifier: *const c_char,
    pub version_major: c_uint,
    pub version_minor: c_uint,
    pub set_host: Option<unsafe extern "C" fn(*mut OfxHost)>,
    pub main_entry: Option<MainEntry>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OfxRectD {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// 大きさの突き合わせにだけ使う（このホストは整数の矩形を受け渡さない）。
#[cfg(test)]
#[repr(C)]
pub struct OfxRectI {
    pub x1: c_int,
    pub y1: c_int,
    pub x2: c_int,
    pub y2: c_int,
}

type S = *const c_char;

#[repr(C)]
pub struct PropertySuiteV1 {
    pub set_pointer: unsafe extern "C" fn(Handle, S, c_int, *mut c_void) -> OfxStatus,
    pub set_string: unsafe extern "C" fn(Handle, S, c_int, S) -> OfxStatus,
    pub set_double: unsafe extern "C" fn(Handle, S, c_int, f64) -> OfxStatus,
    pub set_int: unsafe extern "C" fn(Handle, S, c_int, c_int) -> OfxStatus,
    pub set_pointer_n: unsafe extern "C" fn(Handle, S, c_int, *const *mut c_void) -> OfxStatus,
    pub set_string_n: unsafe extern "C" fn(Handle, S, c_int, *const S) -> OfxStatus,
    pub set_double_n: unsafe extern "C" fn(Handle, S, c_int, *const f64) -> OfxStatus,
    pub set_int_n: unsafe extern "C" fn(Handle, S, c_int, *const c_int) -> OfxStatus,
    pub get_pointer: unsafe extern "C" fn(Handle, S, c_int, *mut *mut c_void) -> OfxStatus,
    pub get_string: unsafe extern "C" fn(Handle, S, c_int, *mut S) -> OfxStatus,
    pub get_double: unsafe extern "C" fn(Handle, S, c_int, *mut f64) -> OfxStatus,
    pub get_int: unsafe extern "C" fn(Handle, S, c_int, *mut c_int) -> OfxStatus,
    pub get_pointer_n: unsafe extern "C" fn(Handle, S, c_int, *mut *mut c_void) -> OfxStatus,
    pub get_string_n: unsafe extern "C" fn(Handle, S, c_int, *mut S) -> OfxStatus,
    pub get_double_n: unsafe extern "C" fn(Handle, S, c_int, *mut f64) -> OfxStatus,
    pub get_int_n: unsafe extern "C" fn(Handle, S, c_int, *mut c_int) -> OfxStatus,
    pub reset: unsafe extern "C" fn(Handle, S) -> OfxStatus,
    pub get_dimension: unsafe extern "C" fn(Handle, S, *mut c_int) -> OfxStatus,
}

#[repr(C)]
pub struct ParameterSuiteV1 {
    pub define: unsafe extern "C" fn(Handle, S, S, *mut Handle) -> OfxStatus,
    pub get_handle: unsafe extern "C" fn(Handle, S, *mut Handle, *mut Handle) -> OfxStatus,
    pub set_get_property_set: unsafe extern "C" fn(Handle, *mut Handle) -> OfxStatus,
    pub get_property_set: unsafe extern "C" fn(Handle, *mut Handle) -> OfxStatus,
    pub get_value: unsafe extern "C" fn(Handle, ...) -> OfxStatus,
    pub get_value_at_time: unsafe extern "C" fn(Handle, f64, ...) -> OfxStatus,
    pub get_derivative: unsafe extern "C" fn(Handle, f64, ...) -> OfxStatus,
    pub get_integral: unsafe extern "C" fn(Handle, f64, f64, ...) -> OfxStatus,
    pub set_value: unsafe extern "C" fn(Handle, ...) -> OfxStatus,
    pub set_value_at_time: unsafe extern "C" fn(Handle, f64, ...) -> OfxStatus,
    pub get_num_keys: unsafe extern "C" fn(Handle, *mut c_uint) -> OfxStatus,
    pub get_key_time: unsafe extern "C" fn(Handle, c_uint, *mut f64) -> OfxStatus,
    pub get_key_index: unsafe extern "C" fn(Handle, f64, c_int, *mut c_int) -> OfxStatus,
    pub delete_key: unsafe extern "C" fn(Handle, f64) -> OfxStatus,
    pub delete_all_keys: unsafe extern "C" fn(Handle) -> OfxStatus,
    pub copy: unsafe extern "C" fn(Handle, Handle, f64, *const c_void) -> OfxStatus,
    pub edit_begin: unsafe extern "C" fn(Handle, S) -> OfxStatus,
    pub edit_end: unsafe extern "C" fn(Handle) -> OfxStatus,
}

#[repr(C)]
pub struct ImageEffectSuiteV1 {
    pub get_property_set: unsafe extern "C" fn(Handle, *mut Handle) -> OfxStatus,
    pub get_param_set: unsafe extern "C" fn(Handle, *mut Handle) -> OfxStatus,
    pub clip_define: unsafe extern "C" fn(Handle, S, *mut Handle) -> OfxStatus,
    pub clip_get_handle: unsafe extern "C" fn(Handle, S, *mut Handle, *mut Handle) -> OfxStatus,
    pub clip_get_property_set: unsafe extern "C" fn(Handle, *mut Handle) -> OfxStatus,
    pub clip_get_image:
        unsafe extern "C" fn(Handle, f64, *const OfxRectD, *mut Handle) -> OfxStatus,
    pub clip_release_image: unsafe extern "C" fn(Handle) -> OfxStatus,
    pub clip_get_region_of_definition:
        unsafe extern "C" fn(Handle, f64, *mut OfxRectD) -> OfxStatus,
    pub abort: unsafe extern "C" fn(Handle) -> c_int,
    pub image_memory_alloc: unsafe extern "C" fn(Handle, usize, *mut Handle) -> OfxStatus,
    pub image_memory_free: unsafe extern "C" fn(Handle) -> OfxStatus,
    pub image_memory_lock: unsafe extern "C" fn(Handle, *mut *mut c_void) -> OfxStatus,
    pub image_memory_unlock: unsafe extern "C" fn(Handle) -> OfxStatus,
}

#[repr(C)]
pub struct MemorySuiteV1 {
    pub alloc: unsafe extern "C" fn(*mut c_void, usize, *mut *mut c_void) -> OfxStatus,
    pub free: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

pub type ThreadFunction = unsafe extern "C" fn(c_uint, c_uint, *mut c_void);

#[repr(C)]
pub struct MultiThreadSuiteV1 {
    pub multi_thread:
        unsafe extern "C" fn(Option<ThreadFunction>, c_uint, *mut c_void) -> OfxStatus,
    pub num_cpus: unsafe extern "C" fn(*mut c_uint) -> OfxStatus,
    pub index: unsafe extern "C" fn(*mut c_uint) -> OfxStatus,
    pub is_spawned_thread: unsafe extern "C" fn() -> c_int,
    pub mutex_create: unsafe extern "C" fn(*mut Handle, c_int) -> OfxStatus,
    pub mutex_destroy: unsafe extern "C" fn(Handle) -> OfxStatus,
    pub mutex_lock: unsafe extern "C" fn(Handle) -> OfxStatus,
    pub mutex_unlock: unsafe extern "C" fn(Handle) -> OfxStatus,
    pub mutex_try_lock: unsafe extern "C" fn(Handle) -> OfxStatus,
}

/// V1 は V2 の先頭の 1 つと同じ並びなので、V2 の表を V1 としても渡す。
#[repr(C)]
pub struct MessageSuiteV2 {
    pub message: unsafe extern "C" fn(*mut c_void, S, S, S, ...) -> OfxStatus,
    pub set_persistent_message: unsafe extern "C" fn(*mut c_void, S, S, S, ...) -> OfxStatus,
    pub clear_persistent_message: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

// `csrc/shim.c` の可変長の関数と、プロセスの準備。
extern "C" {
    pub fn mm_ofx_param_get_value(h: Handle, ...) -> OfxStatus;
    pub fn mm_ofx_param_get_value_at_time(h: Handle, t: f64, ...) -> OfxStatus;
    pub fn mm_ofx_param_get_derivative(h: Handle, t: f64, ...) -> OfxStatus;
    pub fn mm_ofx_param_get_integral(h: Handle, t1: f64, t2: f64, ...) -> OfxStatus;
    pub fn mm_ofx_param_set_value(h: Handle, ...) -> OfxStatus;
    pub fn mm_ofx_param_set_value_at_time(h: Handle, t: f64, ...) -> OfxStatus;
    pub fn mm_ofx_message_v(handle: *mut c_void, ty: S, id: S, format: S, ...) -> OfxStatus;
    pub fn mm_ofx_stdout_to_stderr() -> c_int;
    pub fn mm_ofx_quiet_crash_dialogs();
    #[cfg(test)]
    fn mm_ofx_layout(which: c_int) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mm_ofx_layout` の番号と、Rust 側の大きさ。
    fn rust_sizes() -> [(c_int, usize, &'static str); 10] {
        use std::mem::size_of;
        [
            (0, size_of::<OfxHost>(), "OfxHost"),
            (1, size_of::<OfxPlugin>(), "OfxPlugin"),
            (2, size_of::<PropertySuiteV1>(), "OfxPropertySuiteV1"),
            (3, size_of::<ParameterSuiteV1>(), "OfxParameterSuiteV1"),
            (4, size_of::<ImageEffectSuiteV1>(), "OfxImageEffectSuiteV1"),
            (5, size_of::<MemorySuiteV1>(), "OfxMemorySuiteV1"),
            (6, size_of::<MultiThreadSuiteV1>(), "OfxMultiThreadSuiteV1"),
            (7, size_of::<MessageSuiteV2>(), "OfxMessageSuiteV2"),
            (8, size_of::<OfxRectI>(), "OfxRectI"),
            (9, size_of::<OfxRectD>(), "OfxRectD"),
        ]
    }

    #[test]
    fn 構造体の大きさは公式のヘッダーと同じ() {
        for (which, size, name) in rust_sizes() {
            // SAFETY: 引数の番号を見て sizeof を返すだけの C の関数。
            let c = unsafe { mm_ofx_layout(which) };
            assert_eq!(size, c, "{name}");
        }
    }
}
