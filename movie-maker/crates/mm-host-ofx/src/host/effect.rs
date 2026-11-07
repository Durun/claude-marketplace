//! 画像の効果（`OfxImageEffectHandle`）、クリップ、画像と、OpenFX の Image Effect Suite。
//! 画像はホストが描く前に用意した画素の列（[`Pixels`]）を指すだけで、`clipGetImage` ごとにコピーしない。

use std::ffi::{c_char, c_int, c_void};
use std::sync::{Arc, Mutex, MutexGuard};

use super::ffi::*;
use super::names::*;
use super::params::ParamSet;
use super::pixels::Pixels;
use super::props::{read_str, PropSet};

/// 1 つのクリップ（入力の `Source`、出力の `Output`）。
#[derive(Debug, Default)]
pub struct Clip {
    pub props: PropSet,
    pixels: Mutex<Option<Arc<Pixels>>>,
}

impl Clip {
    pub fn handle(&self) -> Handle {
        self as *const Self as Handle
    }

    fn slot(&self) -> MutexGuard<'_, Option<Arc<Pixels>>> {
        self.pixels.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 描く前に画素を置く。
    pub fn attach(&self, pixels: Arc<Pixels>) {
        *self.slot() = Some(pixels);
    }

    /// 置いた画素。
    pub fn pixels(&self) -> Option<Arc<Pixels>> {
        self.slot().clone()
    }

    fn descriptor(name: &str) -> Self {
        let props = PropSet::default();
        props.set_str(PROP_TYPE, &[TYPE_CLIP]);
        for key in [PROP_NAME, PROP_LABEL, PROP_SHORT_LABEL, PROP_LONG_LABEL] {
            props.set_str(key, &[name]);
        }
        props.set(
            FX_SUPPORTED_COMPONENTS,
            super::props::Value::Str(Vec::new()),
        );
        props.set_int(FX_TEMPORAL_CLIP_ACCESS, &[0]);
        props.set_int(CLIP_OPTIONAL, &[0]);
        props.set_int(CLIP_IS_MASK, &[0]);
        props.set_str(CLIP_FIELD_EXTRACTION, &[FIELD_DOUBLED]);
        props.set_int(FX_SUPPORTS_TILES, &[1]);
        Self {
            props,
            pixels: Mutex::new(None),
        }
    }
}

/// `clipGetImage` が返す画像。`props` を先頭に置き、ハンドルを `PropSet` としても読めるようにする。
#[repr(C)]
struct ImageRef {
    props: PropSet,
    _pixels: Arc<Pixels>,
}

/// 効果の記述子かインスタンス。プラグインに渡したハンドルが動かないよう、各クリップを `Arc` で持つ。
#[derive(Debug, Default)]
pub struct Effect {
    pub props: PropSet,
    pub params: ParamSet,
    clips: Mutex<Vec<Arc<Clip>>>,
}

impl Effect {
    pub fn handle(&self) -> Handle {
        self as *const Self as Handle
    }

    fn clips(&self) -> MutexGuard<'_, Vec<Arc<Clip>>> {
        self.clips.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 名前でクリップを引く。
    pub fn clip(&self, name: &str) -> Option<Arc<Clip>> {
        self.clips()
            .iter()
            .find(|c| c.props.string(PROP_NAME) == name)
            .cloned()
    }

    fn define_clip(&self, name: &str) -> Handle {
        if let Some(c) = self.clip(name) {
            return c.props.handle();
        }
        let clip = Arc::new(Clip::descriptor(name));
        let props = clip.props.handle();
        self.clips().push(clip);
        props
    }

    /// 文脈ごとの記述子（describe の記述子のプロパティを写し、クリップとパラメータは空）。
    pub fn for_context(&self) -> Self {
        Self {
            props: self.props.duplicate(),
            ..Self::default()
        }
    }

    /// 文脈ごとの記述子から、インスタンスを作る。
    pub fn instantiate(&self) -> Self {
        let clips = self
            .clips()
            .iter()
            .map(|c| {
                Arc::new(Clip {
                    props: c.props.duplicate(),
                    pixels: Mutex::new(None),
                })
            })
            .collect();
        let instance = Self {
            props: self.props.duplicate(),
            params: self.params.instantiate(),
            clips: Mutex::new(clips),
        };
        instance.props.set_str(PROP_TYPE, &[TYPE_EFFECT_INSTANCE]);
        instance
    }

    /// 定義されたクリップの名前。
    pub fn clip_names(&self) -> Vec<String> {
        self.clips()
            .iter()
            .map(|c| c.props.string(PROP_NAME))
            .collect()
    }
}

fn image_props(clip: &Clip, px: &Pixels) -> PropSet {
    let p = PropSet::default();
    p.set_str(PROP_TYPE, &[TYPE_IMAGE]);
    p.set_str(FX_PIXEL_DEPTH, &[px.depth.ofx_name()]);
    p.set_str(FX_COMPONENTS, &[COMPONENT_RGBA]);
    p.set_str(FX_PREMULTIPLICATION, &[PREMULTIPLIED]);
    p.set_double(FX_RENDER_SCALE, &[1.0, 1.0]);
    p.set_double(IMAGE_PIXEL_ASPECT, &[1.0]);
    p.set_ptr(IMAGE_DATA, px.data_ptr());
    let rect = [0, 0, px.width_i32(), px.height_i32()];
    p.set_int(IMAGE_BOUNDS, &rect);
    p.set_int(IMAGE_ROD, &rect);
    p.set_int(IMAGE_ROW_BYTES, &[px.row_bytes_i32()]);
    p.set_str(IMAGE_FIELD, &[FIELD_NONE]);
    let id = format!("{}-{:p}", clip.props.string(PROP_NAME), px.data_ptr());
    p.set_str(IMAGE_UNIQUE_ID, &[&id]);
    p
}

unsafe fn effect<'a>(h: Handle) -> Option<&'a Effect> {
    // SAFETY: プラグインはこのホストが渡した効果のハンドルを渡す。
    unsafe { (h as *const Effect).as_ref() }
}

unsafe fn clip<'a>(h: Handle) -> Option<&'a Clip> {
    // SAFETY: プラグインはこのホストが渡したクリップのハンドルを渡す。
    unsafe { (h as *const Clip).as_ref() }
}

unsafe fn put(out: *mut Handle, value: Handle) {
    if !out.is_null() {
        // SAFETY: NULL でない書き先はプラグインが用意したもの。
        unsafe { out.write(value) };
    }
}

unsafe extern "C" fn get_property_set(h: Handle, out: *mut Handle) -> OfxStatus {
    let Some(fx) = (unsafe { effect(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    unsafe { put(out, fx.props.handle()) };
    STAT_OK
}

unsafe extern "C" fn get_param_set(h: Handle, out: *mut Handle) -> OfxStatus {
    let Some(fx) = (unsafe { effect(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    unsafe { put(out, fx.params.handle()) };
    STAT_OK
}

unsafe extern "C" fn clip_define(h: Handle, name: *const c_char, out: *mut Handle) -> OfxStatus {
    let Some(fx) = (unsafe { effect(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let props = fx.define_clip(&unsafe { read_str(name) });
    unsafe { put(out, props) };
    STAT_OK
}

unsafe extern "C" fn clip_get_handle(
    h: Handle,
    name: *const c_char,
    out: *mut Handle,
    props: *mut Handle,
) -> OfxStatus {
    let Some(fx) = (unsafe { effect(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Some(c) = fx.clip(&unsafe { read_str(name) }) else {
        return STAT_ERR_UNKNOWN;
    };
    unsafe {
        put(out, c.handle());
        put(props, c.props.handle());
    }
    STAT_OK
}

unsafe extern "C" fn clip_get_property_set(h: Handle, out: *mut Handle) -> OfxStatus {
    let Some(c) = (unsafe { clip(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    unsafe { put(out, c.props.handle()) };
    STAT_OK
}

unsafe extern "C" fn clip_get_image(
    h: Handle,
    _t: f64,
    _region: *const OfxRectD,
    out: *mut Handle,
) -> OfxStatus {
    let Some(c) = (unsafe { clip(h) }) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let Some(px) = c.pixels() else {
        return STAT_FAILED;
    };
    let image = Box::new(ImageRef {
        props: image_props(c, &px),
        _pixels: px,
    });
    unsafe { put(out, Box::into_raw(image).cast()) };
    STAT_OK
}

unsafe extern "C" fn clip_release_image(h: Handle) -> OfxStatus {
    if h.is_null() {
        return STAT_ERR_BAD_HANDLE;
    }
    // SAFETY: clip_get_image が Box::into_raw で渡したハンドルで、プラグインは 1 度だけ返す。
    drop(unsafe { Box::from_raw(h.cast::<ImageRef>()) });
    STAT_OK
}

unsafe extern "C" fn clip_get_region_of_definition(
    h: Handle,
    _t: f64,
    out: *mut OfxRectD,
) -> OfxStatus {
    let (Some(c), false) = (unsafe { clip(h) }, out.is_null()) else {
        return STAT_ERR_BAD_HANDLE;
    };
    let size = c.props.numbers(FX_PROJECT_SIZE);
    let (w, h) = c.pixels().map_or(
        (
            size.first().copied().unwrap_or(0.0),
            size.get(1).copied().unwrap_or(0.0),
        ),
        |p| (f64::from(p.width_i32()), f64::from(p.height_i32())),
    );
    let rect = OfxRectD {
        x1: 0.0,
        y1: 0.0,
        x2: w,
        y2: h,
    };
    unsafe { out.write(rect) };
    STAT_OK
}

/// 描画の取り消しはまだ無い（1 フレームずつ最後まで描く）。
unsafe extern "C" fn abort(_h: Handle) -> c_int {
    0
}

unsafe extern "C" fn image_memory_alloc(_fx: Handle, bytes: usize, out: *mut Handle) -> OfxStatus {
    let Ok(px) = Pixels::raw(bytes) else {
        return STAT_ERR_MEMORY;
    };
    unsafe { put(out, Box::into_raw(Box::new(px)).cast()) };
    STAT_OK
}

unsafe extern "C" fn image_memory_free(h: Handle) -> OfxStatus {
    if h.is_null() {
        return STAT_ERR_BAD_HANDLE;
    }
    // SAFETY: image_memory_alloc が Box::into_raw で渡したハンドル。
    drop(unsafe { Box::from_raw(h.cast::<Pixels>()) });
    STAT_OK
}

unsafe extern "C" fn image_memory_lock(h: Handle, out: *mut *mut c_void) -> OfxStatus {
    // SAFETY: image_memory_alloc が渡したハンドル。
    let (Some(px), false) = (unsafe { h.cast::<Pixels>().as_ref() }, out.is_null()) else {
        return STAT_ERR_BAD_HANDLE;
    };
    unsafe { out.write(px.data_ptr()) };
    STAT_OK
}

unsafe extern "C" fn image_memory_unlock(_h: Handle) -> OfxStatus {
    STAT_OK
}

pub static SUITE: ImageEffectSuiteV1 = ImageEffectSuiteV1 {
    get_property_set,
    get_param_set,
    clip_define,
    clip_get_handle,
    clip_get_property_set,
    clip_get_image,
    clip_release_image,
    clip_get_region_of_definition,
    abort,
    image_memory_alloc,
    image_memory_free,
    image_memory_lock,
    image_memory_unlock,
};
