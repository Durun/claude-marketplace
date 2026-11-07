//! プロパティの集合（`OfxPropertySetHandle` の中身）と、OpenFX の Property Suite。
//! 型の違う読み書きは `kOfxStatErrValue`、範囲外の番号は `kOfxStatErrBadIndex`、知らない名前の読みは
//! `kOfxStatErrUnknown` を返す。書き込みは知らない名前でも足す（プラグイン独自のプロパティを置けるように）。

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::{Mutex, MutexGuard};

use super::ffi::*;

/// 1 つのプロパティの値。ポインタは `Send` にするため整数で持つ。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Ptr(Vec<usize>),
    Str(Vec<CString>),
    Double(Vec<f64>),
    Int(Vec<c_int>),
}

impl Value {
    fn len(&self) -> usize {
        match self {
            Self::Ptr(v) => v.len(),
            Self::Str(v) => v.len(),
            Self::Double(v) => v.len(),
            Self::Int(v) => v.len(),
        }
    }
}

/// プロパティの集合。プラグインは描画の中から複数のスレッドで読むので、`Mutex` で守る。
#[derive(Debug, Default)]
pub struct PropSet {
    map: Mutex<HashMap<String, Value>>,
}

impl PropSet {
    pub fn lock(&self) -> MutexGuard<'_, HashMap<String, Value>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 同じ中身の別の集合（記述子からインスタンスを作るとき）。
    pub fn duplicate(&self) -> Self {
        Self {
            map: Mutex::new(self.lock().clone()),
        }
    }

    pub fn handle(&self) -> Handle {
        self as *const Self as Handle
    }

    pub fn set(&self, name: &str, value: Value) {
        self.lock().insert(name.to_string(), value);
    }

    pub fn set_str(&self, name: &str, values: &[&str]) {
        let values = values.iter().map(|s| cstring(s)).collect();
        self.set(name, Value::Str(values));
    }

    pub fn set_int(&self, name: &str, values: &[c_int]) {
        self.set(name, Value::Int(values.to_vec()));
    }

    pub fn set_double(&self, name: &str, values: &[f64]) {
        self.set(name, Value::Double(values.to_vec()));
    }

    pub fn set_ptr(&self, name: &str, value: *mut c_void) {
        self.set(name, Value::Ptr(vec![value as usize]));
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.lock().get(name).cloned()
    }

    /// 文字のプロパティの全部。無い名前と型違いは空。
    pub fn strings(&self, name: &str) -> Vec<String> {
        match self.get(name) {
            Some(Value::Str(v)) => v.iter().map(|s| s.to_string_lossy().into_owned()).collect(),
            _ => Vec::new(),
        }
    }

    /// 文字のプロパティの最初の値。
    pub fn string(&self, name: &str) -> String {
        self.strings(name).into_iter().next().unwrap_or_default()
    }

    /// 数のプロパティの全部（整数も実数にする）。
    pub fn numbers(&self, name: &str) -> Vec<f64> {
        match self.get(name) {
            Some(Value::Double(v)) => v,
            Some(Value::Int(v)) => v.into_iter().map(f64::from).collect(),
            _ => Vec::new(),
        }
    }

    /// 整数のプロパティの最初の値。
    pub fn int(&self, name: &str) -> Option<c_int> {
        match self.get(name) {
            Some(Value::Int(v)) => v.first().copied(),
            _ => None,
        }
    }
}

/// NUL を含む文字は NUL の前までにする（OpenFX の文字は C の文字列）。
pub fn cstring(s: &str) -> CString {
    let end = s.find('\0').unwrap_or(s.len());
    CString::new(&s[..end]).unwrap_or_default()
}

/// C の文字列を読む。NULL は空。
///
/// # Safety
/// `p` は NULL か、NUL で終わる文字列を指す。
pub unsafe fn read_str(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: 呼ぶ側の約束どおり、NUL で終わる文字列。
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// ハンドルから集合へ。NULL は `None`。
///
/// # Safety
/// `h` は NULL か、このホストが渡した `PropSet` のハンドル。
pub unsafe fn from_handle<'a>(h: Handle) -> Option<&'a PropSet> {
    // SAFETY: 呼ぶ側の約束どおり、生きている PropSet を指す。
    unsafe { (h as *const PropSet).as_ref() }
}

/// 型ごとの値の出し入れ。
trait Kind: Sized + Clone + Default {
    fn slot(v: &mut Value) -> Option<&mut Vec<Self>>;
    fn wrap(v: Vec<Self>) -> Value;
}

macro_rules! kind {
    ($t:ty, $variant:ident) => {
        impl Kind for $t {
            fn slot(v: &mut Value) -> Option<&mut Vec<Self>> {
                match v {
                    Value::$variant(items) => Some(items),
                    _ => None,
                }
            }
            fn wrap(v: Vec<Self>) -> Value {
                Value::$variant(v)
            }
        }
    };
}
kind!(usize, Ptr);
kind!(CString, Str);
kind!(f64, Double);
kind!(c_int, Int);

unsafe fn write<T: Kind>(h: Handle, name: *const c_char, index: c_int, value: T) -> OfxStatus {
    // SAFETY: プラグインはこのホストが渡したハンドルと C の文字列を渡す。
    let (Some(set), name) = (unsafe { from_handle(h) }, unsafe { read_str(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Ok(index) = usize::try_from(index) else {
        return STAT_ERR_BAD_INDEX;
    };
    let mut map = set.lock();
    let entry = map.entry(name).or_insert_with(|| T::wrap(Vec::new()));
    let Some(items) = T::slot(entry) else {
        return STAT_ERR_VALUE;
    };
    if items.len() <= index {
        items.resize(index + 1, T::default());
    }
    items[index] = value;
    STAT_OK
}

unsafe fn read<T: Kind>(h: Handle, name: *const c_char, index: c_int, out: *mut T) -> OfxStatus {
    // SAFETY: プラグインはこのホストが渡したハンドルと C の文字列を渡す。
    let (Some(set), name) = (unsafe { from_handle(h) }, unsafe { read_str(name) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let mut map = set.lock();
    let Some(entry) = map.get_mut(&name) else {
        return STAT_ERR_UNKNOWN;
    };
    let Some(items) = T::slot(entry) else {
        return STAT_ERR_VALUE;
    };
    let item = usize::try_from(index).ok().and_then(|i| items.get(i));
    match (item, out.is_null()) {
        (Some(v), false) => {
            // SAFETY: out は NULL でなく、プラグインの用意した書き先。
            unsafe { out.write(v.clone()) };
            STAT_OK
        }
        (None, _) => STAT_ERR_BAD_INDEX,
        (_, true) => STAT_ERR_BAD_HANDLE,
    }
}

unsafe extern "C" fn set_pointer(
    h: Handle,
    n: *const c_char,
    i: c_int,
    v: *mut c_void,
) -> OfxStatus {
    unsafe { write(h, n, i, v as usize) }
}
unsafe extern "C" fn set_string(
    h: Handle,
    n: *const c_char,
    i: c_int,
    v: *const c_char,
) -> OfxStatus {
    unsafe { write(h, n, i, cstring(&read_str(v))) }
}
unsafe extern "C" fn set_double(h: Handle, n: *const c_char, i: c_int, v: f64) -> OfxStatus {
    unsafe { write(h, n, i, v) }
}
unsafe extern "C" fn set_int(h: Handle, n: *const c_char, i: c_int, v: c_int) -> OfxStatus {
    unsafe { write(h, n, i, v) }
}

/// `count` 個を順に書く。`get` は `index` 番目の値を作る。
unsafe fn write_n<T: Kind>(
    h: Handle,
    n: *const c_char,
    count: c_int,
    get: impl Fn(usize) -> T,
) -> OfxStatus {
    for i in 0..count.max(0) {
        let st = unsafe { write(h, n, i, get(i as usize)) };
        if st != STAT_OK {
            return st;
        }
    }
    STAT_OK
}
unsafe extern "C" fn set_pointer_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    v: *const *mut c_void,
) -> OfxStatus {
    unsafe { write_n(h, n, c, |i| *v.add(i) as usize) }
}
unsafe extern "C" fn set_string_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    v: *const *const c_char,
) -> OfxStatus {
    unsafe { write_n(h, n, c, |i| cstring(&read_str(*v.add(i)))) }
}
unsafe extern "C" fn set_double_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    v: *const f64,
) -> OfxStatus {
    unsafe { write_n(h, n, c, |i| *v.add(i)) }
}
unsafe extern "C" fn set_int_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    v: *const c_int,
) -> OfxStatus {
    unsafe { write_n(h, n, c, |i| *v.add(i)) }
}

unsafe extern "C" fn get_pointer(
    h: Handle,
    n: *const c_char,
    i: c_int,
    out: *mut *mut c_void,
) -> OfxStatus {
    unsafe { read(h, n, i, out.cast::<usize>()) }
}
unsafe extern "C" fn get_string(
    h: Handle,
    n: *const c_char,
    i: c_int,
    out: *mut *const c_char,
) -> OfxStatus {
    // 返すポインタは集合の中の文字を指し、次にそのプロパティを書くまで生きる。
    let Some(set) = (unsafe { from_handle(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let name = unsafe { read_str(n) };
    let map = set.lock();
    let item = match map.get(&name) {
        None => return STAT_ERR_UNKNOWN,
        Some(Value::Str(v)) => usize::try_from(i).ok().and_then(|i| v.get(i)),
        Some(_) => return STAT_ERR_VALUE,
    };
    match item {
        Some(s) if !out.is_null() => {
            unsafe { out.write(s.as_ptr()) };
            STAT_OK
        }
        Some(_) => STAT_ERR_BAD_HANDLE,
        None => STAT_ERR_BAD_INDEX,
    }
}
unsafe extern "C" fn get_double(h: Handle, n: *const c_char, i: c_int, out: *mut f64) -> OfxStatus {
    unsafe { read(h, n, i, out) }
}
unsafe extern "C" fn get_int(h: Handle, n: *const c_char, i: c_int, out: *mut c_int) -> OfxStatus {
    unsafe { read(h, n, i, out) }
}

/// `count` 個を順に読む。
unsafe fn read_n(count: c_int, mut one: impl FnMut(c_int) -> OfxStatus) -> OfxStatus {
    for i in 0..count.max(0) {
        let st = one(i);
        if st != STAT_OK {
            return st;
        }
    }
    STAT_OK
}
unsafe extern "C" fn get_pointer_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    out: *mut *mut c_void,
) -> OfxStatus {
    unsafe { read_n(c, |i| get_pointer(h, n, i, out.add(i as usize))) }
}
unsafe extern "C" fn get_string_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    out: *mut *const c_char,
) -> OfxStatus {
    unsafe { read_n(c, |i| get_string(h, n, i, out.add(i as usize))) }
}
unsafe extern "C" fn get_double_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    out: *mut f64,
) -> OfxStatus {
    unsafe { read_n(c, |i| get_double(h, n, i, out.add(i as usize))) }
}
unsafe extern "C" fn get_int_n(
    h: Handle,
    n: *const c_char,
    c: c_int,
    out: *mut c_int,
) -> OfxStatus {
    unsafe { read_n(c, |i| get_int(h, n, i, out.add(i as usize))) }
}

/// 既定値を覚えていないので、何もせずに成功を返す。
unsafe extern "C" fn reset(h: Handle, _n: *const c_char) -> OfxStatus {
    if h.is_null() {
        STAT_ERR_BAD_HANDLE
    } else {
        STAT_OK
    }
}

unsafe extern "C" fn get_dimension(h: Handle, n: *const c_char, out: *mut c_int) -> OfxStatus {
    let Some(set) = (unsafe { from_handle(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let name = unsafe { read_str(n) };
    match (set.lock().get(&name), out.is_null()) {
        (Some(v), false) => {
            unsafe { out.write(c_int::try_from(v.len()).unwrap_or(c_int::MAX)) };
            STAT_OK
        }
        (None, _) => STAT_ERR_UNKNOWN,
        (_, true) => STAT_ERR_BAD_HANDLE,
    }
}

pub static SUITE: PropertySuiteV1 = PropertySuiteV1 {
    set_pointer,
    set_string,
    set_double,
    set_int,
    set_pointer_n,
    set_string_n,
    set_double_n,
    set_int_n,
    get_pointer,
    get_string,
    get_double,
    get_int,
    get_pointer_n,
    get_string_n,
    get_double_n,
    get_int_n,
    reset,
    get_dimension,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 書いた値を読める_型違いと範囲外は誤り() {
        let set = PropSet::default();
        let h = set.handle();
        let name = c"OfxPropLabel";
        unsafe {
            assert_eq!(
                (SUITE.set_string)(h, name.as_ptr(), 1, c"b".as_ptr()),
                STAT_OK
            );
            let mut out: *const c_char = std::ptr::null();
            assert_eq!((SUITE.get_string)(h, name.as_ptr(), 1, &mut out), STAT_OK);
            assert_eq!(read_str(out), "b");
            let mut dim = 0;
            assert_eq!((SUITE.get_dimension)(h, name.as_ptr(), &mut dim), STAT_OK);
            assert_eq!(dim, 2, "番号 1 に書くと 0 も埋まる");
            let mut x = 0.0;
            assert_eq!(
                (SUITE.get_double)(h, name.as_ptr(), 0, &mut x),
                STAT_ERR_VALUE
            );
            assert_eq!(
                (SUITE.get_string)(h, name.as_ptr(), 5, &mut out),
                STAT_ERR_BAD_INDEX
            );
            assert_eq!(
                (SUITE.get_double)(h, c"Nope".as_ptr(), 0, &mut x),
                STAT_ERR_UNKNOWN
            );
        }
    }

    #[test]
    fn 複数の値をまとめて書いて読める() {
        let set = PropSet::default();
        let h = set.handle();
        let name = c"OfxImagePropBounds";
        let v = [1, 2, 3, 4];
        let mut out = [0; 4];
        unsafe {
            assert_eq!((SUITE.set_int_n)(h, name.as_ptr(), 4, v.as_ptr()), STAT_OK);
            assert_eq!(
                (SUITE.get_int_n)(h, name.as_ptr(), 4, out.as_mut_ptr()),
                STAT_OK
            );
        }
        assert_eq!(out, v);
    }
}
