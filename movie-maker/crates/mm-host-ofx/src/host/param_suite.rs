//! OpenFX の Parameter Suite（C から呼ばれる関数）。可変長の関数は `csrc/shim.c` が受け、
//! 成分の型と数（`mm_ofx_param_shape`）を聞いてから固定長の `mm_ofx_param_*` を呼ぶ。

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::sync::Arc;

use mm_host_ofx::desc::ParamKind;

use super::ffi::*;
use super::params::{Param, ParamSet, Raw};
use super::props::{cstring, read_str};

/// 値の成分の型。`csrc/shim.c` の `MM_SHAPE_*` と同じ値。
const SHAPE_NONE: c_int = 0;
const SHAPE_INT: c_int = 1;
const SHAPE_DOUBLE: c_int = 2;
const SHAPE_STRING: c_int = 3;

unsafe fn param<'a>(h: Handle) -> Option<&'a Param> {
    // SAFETY: プラグインはこのホストが渡したパラメータのハンドルを渡す。
    unsafe { (h as *const Param).as_ref() }
}

unsafe fn param_set<'a>(h: Handle) -> Option<&'a ParamSet> {
    // SAFETY: プラグインはこのホストが渡したパラメータの集合のハンドルを渡す。
    unsafe { (h as *const ParamSet).as_ref() }
}

unsafe fn put(out: *mut Handle, value: Handle) {
    if !out.is_null() {
        // SAFETY: NULL でない書き先はプラグインが用意したもの。
        unsafe { out.write(value) };
    }
}

unsafe extern "C" fn define(
    set: Handle,
    ty: *const c_char,
    name: *const c_char,
    props: *mut Handle,
) -> OfxStatus {
    let (Some(set), Some(kind)) = (
        unsafe { param_set(set) },
        ParamKind::from_ofx(&unsafe { read_str(ty) }),
    ) else {
        return STAT_ERR_UNKNOWN;
    };
    match set.define(kind, &unsafe { read_str(name) }) {
        Ok(h) => {
            unsafe { put(props, h) };
            STAT_OK
        }
        Err(st) => st,
    }
}

unsafe extern "C" fn get_handle(
    set: Handle,
    name: *const c_char,
    out: *mut Handle,
    props: *mut Handle,
) -> OfxStatus {
    let Some(set) = (unsafe { param_set(set) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Some(p) = set.find(&unsafe { read_str(name) }) else {
        return STAT_ERR_UNKNOWN;
    };
    unsafe {
        put(out, Arc::as_ptr(&p) as Handle);
        put(props, p.props.handle());
    }
    STAT_OK
}

unsafe extern "C" fn set_get_property_set(set: Handle, out: *mut Handle) -> OfxStatus {
    let Some(set) = (unsafe { param_set(set) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    unsafe { put(out, set.props.handle()) };
    STAT_OK
}

unsafe extern "C" fn get_property_set(h: Handle, out: *mut Handle) -> OfxStatus {
    let Some(p) = (unsafe { param(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    unsafe { put(out, p.props.handle()) };
    STAT_OK
}

/// `csrc/shim.c` から: 値の成分の型と数。
#[no_mangle]
unsafe extern "C" fn mm_ofx_param_shape(h: Handle, count: *mut c_int) -> c_int {
    let Some(p) = (unsafe { param(h) }) else {
        return SHAPE_NONE;
    };
    let dim = p.kind.dimension();
    let (shape, n) = if dim == 0 {
        (SHAPE_NONE, 0)
    } else if p.kind.is_text() {
        (SHAPE_STRING, 1)
    } else if p.kind.is_integer() {
        (SHAPE_INT, dim)
    } else {
        (SHAPE_DOUBLE, dim)
    };
    if !count.is_null() {
        unsafe { count.write(n as c_int) };
    }
    shape
}

/// `csrc/shim.c` から: 値を `out` の各ポインタへ書く。`scale` は微分（0）と積分（区間の長さ）に使う。
#[no_mangle]
unsafe extern "C" fn mm_ofx_param_get(
    h: Handle,
    scale: f64,
    out: *mut *mut c_void,
    count: c_int,
) -> OfxStatus {
    let Some(p) = (unsafe { param(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let value = p.value();
    for i in 0..usize::try_from(count).unwrap_or(0) {
        // SAFETY: out は shim が集めた count 個のポインタで、各々は成分の型の書き先を指す。
        let dst = unsafe { *out.add(i) };
        if dst.is_null() {
            return STAT_ERR_BAD_HANDLE;
        }
        match &*value {
            Raw::Ints(v) => unsafe {
                dst.cast::<c_int>()
                    .write((f64::from(v.get(i).copied().unwrap_or(0)) * scale) as c_int)
            },
            Raw::Doubles(v) => unsafe {
                dst.cast::<f64>()
                    .write(v.get(i).copied().unwrap_or(0.0) * scale)
            },
            Raw::Text(s) => unsafe { dst.cast::<*const c_char>().write(s.as_ptr()) },
            Raw::Nothing => return STAT_ERR_BAD_HANDLE,
        }
    }
    STAT_OK
}

fn store(h: Handle, raw: Raw) -> OfxStatus {
    // SAFETY: shim はプラグインから受けたパラメータのハンドルをそのまま渡す。
    match unsafe { param(h) } {
        Some(p) => {
            *p.value() = raw;
            STAT_OK
        }
        None => STAT_ERR_BAD_HANDLE,
    }
}

#[no_mangle]
unsafe extern "C" fn mm_ofx_param_set_ints(h: Handle, v: *const c_int, count: c_int) -> OfxStatus {
    let n = usize::try_from(count).unwrap_or(0);
    // SAFETY: shim は count 個の整数を並べた配列を渡す。
    store(
        h,
        Raw::Ints(unsafe { std::slice::from_raw_parts(v, n) }.to_vec()),
    )
}

#[no_mangle]
unsafe extern "C" fn mm_ofx_param_set_doubles(h: Handle, v: *const f64, count: c_int) -> OfxStatus {
    let n = usize::try_from(count).unwrap_or(0);
    // SAFETY: shim は count 個の実数を並べた配列を渡す。
    store(
        h,
        Raw::Doubles(unsafe { std::slice::from_raw_parts(v, n) }.to_vec()),
    )
}

#[no_mangle]
unsafe extern "C" fn mm_ofx_param_set_string(h: Handle, v: *const c_char) -> OfxStatus {
    store(h, Raw::Text(cstring(&unsafe { read_str(v) })))
}

unsafe extern "C" fn get_num_keys(_h: Handle, out: *mut c_uint) -> OfxStatus {
    if !out.is_null() {
        unsafe { out.write(0) };
    }
    STAT_OK
}
unsafe extern "C" fn get_key_time(_h: Handle, _nth: c_uint, _out: *mut f64) -> OfxStatus {
    STAT_ERR_BAD_INDEX
}
unsafe extern "C" fn get_key_index(
    _h: Handle,
    _t: f64,
    _dir: c_int,
    _out: *mut c_int,
) -> OfxStatus {
    STAT_FAILED
}
unsafe extern "C" fn delete_key(_h: Handle, _t: f64) -> OfxStatus {
    STAT_ERR_BAD_INDEX
}
unsafe extern "C" fn delete_all_keys(_h: Handle) -> OfxStatus {
    STAT_OK
}
unsafe extern "C" fn copy(
    to: Handle,
    from: Handle,
    _offset: f64,
    _range: *const c_void,
) -> OfxStatus {
    match unsafe { (param(to), param(from)) } {
        (Some(to), Some(from)) if to.kind == from.kind => {
            let v = from.value().clone();
            *to.value() = v;
            STAT_OK
        }
        _ => STAT_ERR_BAD_HANDLE,
    }
}
unsafe extern "C" fn edit_begin(_set: Handle, _name: *const c_char) -> OfxStatus {
    STAT_OK
}
unsafe extern "C" fn edit_end(_set: Handle) -> OfxStatus {
    STAT_OK
}

pub static SUITE: ParameterSuiteV1 = ParameterSuiteV1 {
    define,
    get_handle,
    set_get_property_set,
    get_property_set,
    get_value: mm_ofx_param_get_value,
    get_value_at_time: mm_ofx_param_get_value_at_time,
    get_derivative: mm_ofx_param_get_derivative,
    get_integral: mm_ofx_param_get_integral,
    set_value: mm_ofx_param_set_value,
    set_value_at_time: mm_ofx_param_set_value_at_time,
    get_num_keys,
    get_key_time,
    get_key_index,
    delete_key,
    delete_all_keys,
    copy,
    edit_begin,
    edit_end,
};
