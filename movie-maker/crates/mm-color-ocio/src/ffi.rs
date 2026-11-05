//! bindgen が作った C の口と、それを包む安全な型。unsafe はこのファイルに閉じる。

use std::ffi::{c_char, c_int, CStr, CString};

use mm_types::{MmError, Result};

mod raw {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

fn last_error() -> String {
    // SAFETY: 直近のエラー文字列は NUL 終端でスレッドごとに保持される。
    unsafe { CStr::from_ptr(raw::mm_ocio_last_error()) }
        .to_string_lossy()
        .into_owned()
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| MmError::InvalidArgument(format!("NUL を含む名前: {s:?}")))
}

/// OCIO のバージョン文字列。
pub fn version() -> String {
    // SAFETY: 静的な NUL 終端文字列を返す。
    unsafe { CStr::from_ptr(raw::mm_ocio_version()) }
        .to_string_lossy()
        .into_owned()
}

/// OCIO の設定。
pub struct Config(*mut raw::MmOcioConfig);

/// 変換（CPU と GPU のシェーダーを持つ）。
pub struct Processor(*mut raw::MmOcioProcessor);

/// GPU のシェーダーとその付属の数。
pub struct Shader {
    pub text: String,
    pub num_textures: usize,
    pub num_uniforms: usize,
}

impl Config {
    /// `ocio://...` なら組み込み、それ以外はファイルのパス。
    pub fn open(uri_or_path: &str) -> Result<Self> {
        let arg = cstring(uri_or_path)?;
        // SAFETY: arg は呼び出しの間生きている。
        let ptr = unsafe { raw::mm_ocio_config_create(arg.as_ptr()) };
        if ptr.is_null() {
            return Err(MmError::Corrupt(format!("OCIO の設定: {}", last_error())));
        }
        Ok(Self(ptr))
    }

    /// 色空間の名前の一覧。
    pub fn colorspace_names(&self) -> Vec<String> {
        // SAFETY: self.0 は有効。名前は設定が生きている間有効で、すぐ複製する。
        unsafe {
            let n = raw::mm_ocio_config_num_colorspaces(self.0);
            (0..n)
                .map(|i| {
                    CStr::from_ptr(raw::mm_ocio_config_colorspace_name(self.0, i))
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        }
    }

    /// 名前の色空間があるか。
    pub fn has_colorspace(&self, name: &str) -> bool {
        let Ok(name) = cstring(name) else {
            return false;
        };
        // SAFETY: self.0 と name は有効。
        unsafe { raw::mm_ocio_config_has_colorspace(self.0, name.as_ptr()) != 0 }
    }

    /// 既定の display と、その既定の view。
    pub fn default_display_view(&self, src: &str) -> Option<(String, String)> {
        let src = cstring(src).ok()?;
        // SAFETY: self.0 と src は有効。返る文字列は設定が生きている間有効で、すぐ複製する。
        unsafe {
            let display = raw::mm_ocio_config_default_display(self.0);
            if display.is_null() {
                return None;
            }
            let view = raw::mm_ocio_config_default_view(self.0, display, src.as_ptr());
            if view.is_null() {
                return None;
            }
            Some((
                CStr::from_ptr(display).to_string_lossy().into_owned(),
                CStr::from_ptr(view).to_string_lossy().into_owned(),
            ))
        }
    }

    /// `src` を `display` と `view` で見る変換を作る（トーンマップを含む）。None は既定。
    pub fn display_processor(
        &self,
        src: &str,
        display: Option<&str>,
        view: Option<&str>,
    ) -> Result<Processor> {
        let s = cstring(src)?;
        let d = display.map(cstring).transpose()?;
        let v = view.map(cstring).transpose()?;
        let ptr_of = |c: &Option<CString>| c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        // SAFETY: self.0 と文字列は有効。NULL は既定を表す。
        let ptr = unsafe {
            raw::mm_ocio_processor_create_display(self.0, s.as_ptr(), ptr_of(&d), ptr_of(&v))
        };
        if ptr.is_null() {
            return Err(MmError::Unsupported(format!(
                "OCIO の display 変換 {src}: {}",
                last_error()
            )));
        }
        Ok(Processor(ptr))
    }

    /// `src` から `dst` への変換を作る。
    pub fn processor(&self, src: &str, dst: &str) -> Result<Processor> {
        let (s, d) = (cstring(src)?, cstring(dst)?);
        // SAFETY: self.0 と文字列は有効。
        let ptr = unsafe { raw::mm_ocio_processor_create(self.0, s.as_ptr(), d.as_ptr()) };
        if ptr.is_null() {
            return Err(MmError::Unsupported(format!(
                "OCIO の変換 {src} -> {dst}: {}",
                last_error()
            )));
        }
        Ok(Processor(ptr))
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        // SAFETY: create で得たポインタを 1 回だけ解放する。
        unsafe { raw::mm_ocio_config_free(self.0) }
    }
}

impl Processor {
    /// 何もしない変換か。
    pub fn is_noop(&self) -> bool {
        // SAFETY: self.0 は有効。
        unsafe { raw::mm_ocio_processor_is_noop(self.0) != 0 }
    }

    /// CPU で 1 画素に当てる。
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> Result<[f32; 3]> {
        let mut px = rgb;
        // SAFETY: px は 3 個の f32 を持つ。
        let rc = unsafe { raw::mm_ocio_processor_apply_rgb(self.0, px.as_mut_ptr()) };
        if rc != 0 {
            return Err(MmError::Corrupt(format!(
                "OCIO の CPU 変換: {}",
                last_error()
            )));
        }
        Ok(px)
    }

    /// GPU のシェーダー（Vulkan 向けの GLSL 4.6）。入口の関数名は `function_name`。
    pub fn gpu_shader(&mut self, function_name: &str) -> Result<Shader> {
        let name = cstring(function_name)?;
        let mut text: *const c_char = std::ptr::null();
        let (mut textures, mut uniforms): (c_int, c_int) = (0, 0);
        // SAFETY: self.0 と name は有効。text は processor が生きている間有効で、すぐ複製する。
        let rc = unsafe {
            raw::mm_ocio_processor_gpu_shader(
                self.0,
                name.as_ptr(),
                &mut text,
                &mut textures,
                &mut uniforms,
            )
        };
        if rc != 0 || text.is_null() {
            return Err(MmError::Unsupported(format!(
                "OCIO の GPU シェーダー: {}",
                last_error()
            )));
        }
        // SAFETY: 成功時 text は NUL 終端。
        let text = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        Ok(Shader {
            text,
            num_textures: usize::try_from(textures).unwrap_or(0),
            num_uniforms: usize::try_from(uniforms).unwrap_or(0),
        })
    }
}

impl Drop for Processor {
    fn drop(&mut self) {
        // SAFETY: create で得たポインタを 1 回だけ解放する。
        unsafe { raw::mm_ocio_processor_free(self.0) }
    }
}

// SAFETY: ポインタの先の OCIO のオブジェクトは、呼び出しを 1 つの Mutex で直列にして使う。
unsafe impl Send for Config {}
// SAFETY: 同上。
unsafe impl Send for Processor {}
