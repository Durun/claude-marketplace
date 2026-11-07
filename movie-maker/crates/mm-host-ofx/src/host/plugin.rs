//! バンドルの読み込みと、プラグインの load・describe（OpenFX の「The Plug-in Life Cycle」の前半）。
//! 共有ライブラリは解放しない。プラグインがスレッドや終了時の処理を残していることがあり、
//! 解放するとプロセスの終わりにクラッシュする。プロセスごと捨てるので漏れにはならない。

use std::ffi::{c_char, c_int, CString};
use std::path::{Path, PathBuf};

use mm_host_ofx::desc::{BundleDesc, PluginDesc, SkippedPlugin, RENDERABLE_CONTEXTS};
use mm_host_ofx::scan::bundle_binary;

use super::effect::Effect;
use super::ffi::*;
use super::names::*;
use super::props::{read_str, PropSet};
use super::suites;

/// 読み込んだバンドル。
pub struct Bundle {
    pub path: PathBuf,
    pub binary: PathBuf,
    lib: libloading::Library,
}

impl Bundle {
    /// `<名前>.ofx.bundle` の中の共有ライブラリを読み込む。
    pub fn open(path: &Path) -> Result<Self, String> {
        let binary =
            bundle_binary(path).ok_or_else(|| format!("{} はバンドルでない", path.display()))?;
        // SAFETY: プラグインの共有ライブラリの初期化を走らせる。クラッシュしてもこのプロセスだけが落ちる。
        let lib = unsafe { libloading::Library::new(&binary) }
            .map_err(|e| format!("{} を読み込めない: {e}", binary.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            binary,
            lib,
        })
    }

    /// バンドルの中のプラグイン。`OfxSetHost`（1.5）があれば先に呼ぶ。
    pub fn plugins(&self) -> Result<Vec<Plugin>, String> {
        // SAFETY: OpenFX の約束どおりの型で、ホストの構造体はプロセスの間ずっと生きている。
        unsafe {
            if let Ok(set_host) = self
                .lib
                .get::<unsafe extern "C" fn(*const OfxHost) -> OfxStatus>(b"OfxSetHost\0")
            {
                set_host(suites::host());
            }
            let count = self
                .lib
                .get::<unsafe extern "C" fn() -> c_int>(b"OfxGetNumberOfPlugins\0")
                .map_err(|e| format!("OfxGetNumberOfPlugins が無い: {e}"))?;
            let get = self
                .lib
                .get::<unsafe extern "C" fn(c_int) -> *const OfxPlugin>(b"OfxGetPlugin\0")
                .map_err(|e| format!("OfxGetPlugin が無い: {e}"))?;
            Ok((0..count().max(0))
                .filter_map(|i| Plugin::new(i as usize, get(i), &self.path))
                .collect())
        }
    }
}

/// 1 つのプラグイン（`OfxPlugin` の写し）。
#[derive(Debug, Clone)]
pub struct Plugin {
    pub index: usize,
    pub id: String,
    pub api: String,
    pub api_version: c_int,
    pub version: [u32; 2],
    set_host: Option<unsafe extern "C" fn(*mut OfxHost)>,
    main_entry: Option<MainEntry>,
    bundle: PathBuf,
}

impl Plugin {
    fn new(index: usize, p: *const OfxPlugin, bundle: &Path) -> Option<Self> {
        // SAFETY: OfxGetPlugin が返した、ライブラリが生きている間有効な構造体（NULL は飛ばす）。
        let p = unsafe { p.as_ref()? };
        Some(Self {
            index,
            id: unsafe { read_str(p.plugin_identifier) },
            api: unsafe { read_str(p.plugin_api) },
            api_version: p.api_version,
            version: [p.version_major, p.version_minor],
            set_host: p.set_host,
            main_entry: p.main_entry,
            bundle: bundle.to_path_buf(),
        })
    }

    /// action を 1 つ呼ぶ。
    pub fn call(
        &self,
        action: &str,
        handle: Handle,
        in_args: Option<&PropSet>,
        out_args: Option<&PropSet>,
    ) -> OfxStatus {
        let Some(entry) = self.main_entry else {
            return STAT_ERR_UNSUPPORTED;
        };
        let action = CString::new(action).unwrap_or_default();
        let args = |a: Option<&PropSet>| a.map_or(std::ptr::null_mut(), PropSet::handle);
        // SAFETY: OpenFX の mainEntry に、このホストが作ったハンドルと C の文字列を渡す。
        unsafe {
            entry(
                action.as_ptr() as *const c_char,
                handle,
                args(in_args),
                args(out_args),
            )
        }
    }

    /// action を呼び、成功（OK か既定の動作）でなければ誤りにする。
    pub fn expect(
        &self,
        action: &str,
        handle: Handle,
        in_args: Option<&PropSet>,
    ) -> Result<(), String> {
        match self.call(action, handle, in_args, None) {
            STAT_OK | STAT_REPLY_DEFAULT => Ok(()),
            st => Err(self.failure(action, st)),
        }
    }

    /// action の失敗の文。プラグインが Message Suite で出したエラーの文を添える。
    pub fn failure(&self, action: &str, st: OfxStatus) -> String {
        let why = suites::take_last_error()
            .map(|m| format!("（{m}）"))
            .unwrap_or_default();
        format!("{} の {action} が {st} を返した{why}", self.id)
    }

    /// 画像の効果の API か。
    pub fn is_image_effect(&self) -> bool {
        self.api == IMAGE_EFFECT_API && self.api_version == 1
    }

    /// ホストを渡して `Load` を呼ぶ。
    pub fn load(&self) -> Result<(), String> {
        let set_host = self.set_host.ok_or("setHost が無い")?;
        // SAFETY: ホストの構造体はプロセスの間ずっと生きている。
        unsafe { set_host(suites::host()) };
        self.expect(ACTION_LOAD, std::ptr::null_mut(), None)
    }

    /// `Unload` を呼ぶ。失敗しても後片付けなので無視する。
    pub fn unload(&self) {
        let _ = self.call(ACTION_UNLOAD, std::ptr::null_mut(), None, None);
    }

    /// `Describe` と、描ける文脈があればその `DescribeInContext` を呼ぶ。
    pub fn describe(&self) -> Result<Described, String> {
        let base = Box::new(self.descriptor());
        self.expect(ACTION_DESCRIBE, base.handle(), None)?;
        let contexts = base.props.strings(FX_SUPPORTED_CONTEXTS);
        let Some(context) = RENDERABLE_CONTEXTS
            .into_iter()
            .find(|c| contexts.iter().any(|s| s == c))
        else {
            return Ok(Described {
                base,
                context: None,
            });
        };
        let in_context = Box::new(base.for_context());
        let args = PropSet::default();
        args.set_str(FX_CONTEXT, &[context]);
        self.expect(ACTION_DESCRIBE_IN_CONTEXT, in_context.handle(), Some(&args))?;
        Ok(Described {
            base,
            context: Some((context.to_string(), in_context)),
        })
    }

    fn descriptor(&self) -> Effect {
        let fx = Effect::default();
        let p = &fx.props;
        p.set_str(PROP_TYPE, &[TYPE_EFFECT]);
        for key in [
            PROP_LABEL,
            PROP_SHORT_LABEL,
            PROP_LONG_LABEL,
            PROP_VERSION_LABEL,
            PROP_PLUGIN_DESCRIPTION,
            FX_GROUPING,
        ] {
            p.set_str(key, &[""]);
        }
        p.set_int(
            PROP_VERSION,
            &[self.version[0] as c_int, self.version[1] as c_int],
        );
        p.set(FX_SUPPORTED_CONTEXTS, super::props::Value::Str(Vec::new()));
        p.set(FX_SUPPORTED_DEPTHS, super::props::Value::Str(Vec::new()));
        p.set_str(FX_RENDER_THREAD_SAFETY, &[RENDER_INSTANCE_SAFE]);
        p.set_str(PROP_PLUGIN_FILE_PATH, &[&self.bundle.to_string_lossy()]);
        for (key, v) in [
            (FX_SINGLE_INSTANCE, 0),
            (FX_HOST_FRAME_THREADING, 0),
            (FX_SUPPORTS_MULTI_RESOLUTION, 1),
            (FX_SUPPORTS_TILES, 1),
            (FX_TEMPORAL_CLIP_ACCESS, 0),
            (FX_MULTIPLE_CLIP_DEPTHS, 0),
            (FX_MULTIPLE_CLIP_PARS, 0),
        ] {
            p.set_int(key, &[v]);
        }
        fx
    }
}

/// describe の結果の記述子。プラグインがハンドルを持ち続けてもよいよう、プロセスの間は捨てない。
pub struct Described {
    pub base: Box<Effect>,
    /// このホストが使う文脈と、その文脈の記述子。
    pub context: Option<(String, Box<Effect>)>,
}

impl Described {
    /// 結果に載せる記述。
    pub fn desc(&self, plugin: &Plugin) -> PluginDesc {
        let p = &self.base.props;
        PluginDesc {
            id: plugin.id.clone(),
            version: plugin.version,
            label: p.string(PROP_LABEL),
            grouping: p.string(FX_GROUPING),
            description: p.string(PROP_PLUGIN_DESCRIPTION),
            contexts: p.strings(FX_SUPPORTED_CONTEXTS),
            pixel_depths: p.strings(FX_SUPPORTED_DEPTHS),
            context: self.context.as_ref().map(|(c, _)| c.clone()),
            params: self
                .context
                .as_ref()
                .map(|(_, fx)| fx.params.descs())
                .unwrap_or_default(),
        }
    }
}

/// バンドルの全部のプラグインを load して describe する（`mm-host-ofx describe`）。
pub fn describe_bundle(path: &Path) -> Result<BundleDesc, String> {
    let bundle = Bundle::open(path)?;
    let mut out = BundleDesc {
        bundle: bundle.path.clone(),
        binary: bundle.binary.clone(),
        plugins: Vec::new(),
        skipped: Vec::new(),
    };
    for plugin in bundle.plugins()? {
        let described = if plugin.is_image_effect() {
            // Load が失敗したプラグインには、以後の action（Unload も）を呼ばない。
            plugin.load().map(|()| {
                let d = plugin.describe();
                plugin.unload();
                d
            })
        } else {
            Err(format!(
                "画像の効果の API でない（{} {}）",
                plugin.api, plugin.api_version
            ))
        };
        match described.and_then(|d| d) {
            Ok(d) => out.plugins.push(d.desc(&plugin)),
            Err(reason) => out.skipped.push(SkippedPlugin {
                index: plugin.index,
                id: plugin.id.clone(),
                reason,
            }),
        }
    }
    std::mem::forget(bundle);
    Ok(out)
}
