//! `assemble(config) -> Engine`。実装のクレートを知っているのはこのファイルだけ。

use std::sync::Arc;

use mm_audio_api::MixerFactory;
use mm_audio_dsp::DspMixerFactory;
use mm_cache_api::CacheStore;
use mm_cache_fs::FsCache;
use mm_color_api::ColorTransform;
use mm_color_lut::LutColor;
use mm_decode_api::{DecoderFactory, EncoderFactory, GpuContextRef};
use mm_gpu::{GpuContext, GpuRequest};
use mm_node_api::Services;
use mm_render::{FrameCacheConfig, WgpuRenderer, DEFAULT_MEMORY_BYTES};
use mm_render_api::Renderer;
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::cache::MemoryCache;
use mm_testkit::color::SampleColor;
use mm_testkit::decode::{SampleDecoderFactory, SampleEncoderFactory};
use mm_testkit::node::{FootageKind, SolidKind, TextKind};
use mm_testkit::render::SampleRenderer;
use mm_testkit::text::{SampleRasterizer, SampleShaper};
use mm_text_api::{TextRasterizer, TextShaper};
use mm_text_glyphon::TextEngine;
use mm_types::{GpuAdapterId, MmError, Result};

use crate::config::{Backend, RuntimeConfig};
#[cfg(any(feature = "d3d12va", feature = "cuvid"))]
use crate::decode_select::WithAudio;
use crate::decode_select::{AutoDecoderFactory, GpuCandidate};
#[cfg(feature = "ffmpeg")]
use crate::encode_select::AutoEncoderFactory;
use crate::engine::{Engine, EngineParts};
use crate::registry::Registry;

/// 設定から実装を選んで `Engine` を作る。
pub fn assemble(config: &RuntimeConfig) -> Result<Engine> {
    let decoders = decoders(config.decode)?;
    let encoders = encoders(config.encode)?;
    let mixers = mixers(config.audio)?;
    let color = color(config.color)?;
    let gpu = gpu_context(config)?;
    let (text_shaper, text_rasterizer) = text(config.text, gpu.as_ref())?;
    let cache = cache(config)?;
    install_index_cache(&cache);
    let services = Services {
        text_shaper,
        text_rasterizer,
        color,
    };
    let renderer = renderer(config, gpu.as_ref(), &decoders, &mixers, &services, &cache)?;
    Ok(Engine::from_parts(EngineParts {
        config: config.clone(),
        registry: registry(config.render)?,
        renderer,
        decoders,
        encoders,
        mixers,
        cache,
        services,
        gpu: gpu.map(|g| g as Arc<dyn GpuContextRef>),
    }))
}

/// インターフェースに合わない実装を選んだときのエラー。
fn unsupported(port: &str, backend: Backend) -> MmError {
    MmError::Unsupported(format!(
        "{port} に実装 {} は選べない（または feature が切ってある）",
        backend.as_str()
    ))
}

fn decoders(backend: Backend) -> Result<Arc<dyn DecoderFactory>> {
    match backend {
        Backend::Sample => Ok(Arc::new(SampleDecoderFactory::default())),
        #[cfg(feature = "ffmpeg")]
        Backend::Ffmpeg => Ok(Arc::new(mm_decode_ffmpeg::FfmpegDecoderFactory)),
        #[cfg(feature = "d3d12va")]
        Backend::D3d12va => Ok(with_cpu_audio(Arc::new(mm_decode_d3d12va::D3d12vaFactory))),
        #[cfg(feature = "cuvid")]
        Backend::Cuvid => Ok(with_cpu_audio(Arc::new(mm_decode_cuvid::CuvidFactory))),
        Backend::Auto => auto_decoders(),
        other => Err(unsupported("decode", other)),
    }
}

/// GPU の実装は音を持たないので、FFmpeg が入っていれば音と素材の情報をそちらから付ける。
#[cfg(any(feature = "d3d12va", feature = "cuvid"))]
fn with_cpu_audio(video: Arc<dyn DecoderFactory>) -> Arc<dyn DecoderFactory> {
    #[cfg(feature = "ffmpeg")]
    {
        Arc::new(WithAudio::new(
            video,
            Arc::new(mm_decode_ffmpeg::FfmpegDecoderFactory),
        ))
    }
    #[cfg(not(feature = "ffmpeg"))]
    {
        video
    }
}

/// 入っている GPU の実装を、試す順（d3d12va、cuvid）に。d3d12va は `open` の中で別プロセスで試し済み。
fn gpu_decoders() -> Vec<GpuCandidate> {
    vec![
        #[cfg(feature = "d3d12va")]
        GpuCandidate::probed_on_open(with_cpu_audio(Arc::new(mm_decode_d3d12va::D3d12vaFactory))),
        #[cfg(feature = "cuvid")]
        GpuCandidate::new(with_cpu_audio(Arc::new(mm_decode_cuvid::CuvidFactory))),
    ]
}

/// `auto`: 入っている GPU の実装を d3d12va、cuvid の順に並べ、最後に ffmpeg。何も入っていなければ拒否する。
fn auto_decoders() -> Result<Arc<dyn DecoderFactory>> {
    let gpu = gpu_decoders();
    #[cfg(feature = "ffmpeg")]
    let fallback: Option<Arc<dyn DecoderFactory>> =
        Some(Arc::new(mm_decode_ffmpeg::FfmpegDecoderFactory));
    #[cfg(not(feature = "ffmpeg"))]
    let fallback: Option<Arc<dyn DecoderFactory>> = None;
    if gpu.is_empty() && fallback.is_none() {
        return Err(unsupported("decode", Backend::Auto));
    }
    Ok(Arc::new(AutoDecoderFactory::new(gpu, fallback)))
}

fn encoders(backend: Backend) -> Result<Arc<dyn EncoderFactory>> {
    match backend {
        Backend::Sample => Ok(Arc::new(SampleEncoderFactory)),
        #[cfg(feature = "ffmpeg")]
        Backend::Ffmpeg => Ok(Arc::new(mm_encode_ffmpeg::FfmpegEncoderFactory)),
        #[cfg(feature = "ffmpeg")]
        Backend::Nvenc => Ok(Arc::new(mm_encode_nvenc::NvencEncoderFactory)),
        #[cfg(feature = "ffmpeg")]
        Backend::Auto => Ok(Arc::new(AutoEncoderFactory::new(vec![
            Arc::new(mm_encode_nvenc::NvencEncoderFactory),
            Arc::new(mm_encode_ffmpeg::FfmpegEncoderFactory),
        ]))),
        other => Err(unsupported("encode", other)),
    }
}

fn mixers(backend: Backend) -> Result<Arc<dyn MixerFactory>> {
    match backend {
        Backend::Sample => Ok(Arc::new(SampleMixerFactory)),
        Backend::Dsp => Ok(Arc::new(DspMixerFactory)),
        other => Err(unsupported("audio", other)),
    }
}

/// GPU を使う実装があるときだけ、コンテキストを 1 回作る。設定のアダプターが無ければ高性能のもの。
/// 要否はレンダラーとデコーダーの両方の選択で決まる（GPU でデコードする実装はレンダラーと同じデバイスが要る）。
fn gpu_context(config: &RuntimeConfig) -> Result<Option<Arc<GpuContext>>> {
    gpu_context_with(config, GpuContext::shared)
}

/// `create` でコンテキストを作る。`decode: auto` だけが GPU を欲しがるときは、作れなくても `None` で続け、
/// ffmpeg にする（GPU の無い CI でも組める）。
fn gpu_context_with(
    config: &RuntimeConfig,
    create: impl FnOnce(&GpuRequest) -> Result<Arc<GpuContext>>,
) -> Result<Option<Arc<GpuContext>>> {
    if !config.needs_gpu() {
        return Ok(None);
    }
    match create(&gpu_request(config)?) {
        Ok(ctx) => Ok(Some(ctx)),
        Err(e) if !config.requires_gpu() => {
            tracing::warn!(error = %e, "GPU のコンテキストを作れないので decode: auto は ffmpeg で動く");
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// コンテキストの頼み方。GPU でデコードする実装は D3D12 のデバイスが要る（FFmpeg と CUDA に渡す）ので、
/// レンダラーもそのアダプターで作る。`auto` は D3D12 が無ければ既定の頼み方に戻し、ffmpeg で動く。
fn gpu_request(config: &RuntimeConfig) -> Result<GpuRequest> {
    let mut default = GpuRequest {
        adapter: config.gpu.clone(),
        ..GpuRequest::default()
    };
    if config.present && cfg!(windows) {
        default.backends = wgpu::Backends::DX12;
    }
    if !config.decode.needs_d3d12() {
        return Ok(default);
    }
    let request = d3d12_request(config.gpu.as_ref());
    match (request, config.decode) {
        (Some(r), Backend::Auto) => Ok(r.unwrap_or(default)),
        (Some(r), _) => r,
        (None, Backend::Auto) => Ok(default),
        (None, other) => Err(unsupported("decode", other)),
    }
}

/// 入っている GPU の実装の D3D12 での頼み方。どちらも入っていなければ `None`。
fn d3d12_request(wanted: Option<&GpuAdapterId>) -> Option<Result<GpuRequest>> {
    #[cfg(feature = "d3d12va")]
    return Some(mm_decode_d3d12va::gpu_request(wanted));
    #[cfg(all(feature = "cuvid", not(feature = "d3d12va")))]
    return Some(mm_decode_cuvid::gpu_request(wanted));
    #[cfg(not(any(feature = "d3d12va", feature = "cuvid")))]
    {
        let _ = wanted;
        None
    }
}

fn color(backend: Backend) -> Result<Arc<dyn ColorTransform>> {
    match backend {
        Backend::Sample => Ok(Arc::new(SampleColor)),
        Backend::Lut => Ok(Arc::new(LutColor)),
        #[cfg(feature = "ocio")]
        Backend::Ocio => Ok(Arc::new(mm_color_ocio::OcioColor::builtin()?)),
        other => Err(unsupported("color", other)),
    }
}

/// シェーパーとレンダラーの組。cpu と glyphon はフォントの読み込みを共有するため同じ `TextEngine` から両方を取る。
/// glyphon は GPU のコンテキストが要る（`RuntimeConfig::needs_gpu` がコンテキストを作らせる）。
fn text(
    backend: Backend,
    gpu: Option<&Arc<GpuContext>>,
) -> Result<(Arc<dyn TextShaper>, Arc<dyn TextRasterizer>)> {
    match backend {
        Backend::Sample => Ok((Arc::new(SampleShaper), Arc::new(SampleRasterizer))),
        Backend::Cef => {
            let exe = mm_host_cef::find_host().ok_or_else(|| unsupported("text", backend))?;
            let raster = mm_text_cef::CefRasterizer::new(Arc::new(mm_host_cef::CefLauncher), exe);
            Ok((TextEngine::new().shaper(), Arc::new(raster)))
        }
        Backend::Cpu => {
            let engine = TextEngine::new();
            Ok((engine.shaper(), engine.rasterizer()))
        }
        Backend::Glyphon => {
            let gpu = gpu.ok_or_else(|| unsupported("text", backend))?;
            let engine = TextEngine::new();
            Ok((engine.shaper(), engine.gpu_rasterizer(gpu.clone())))
        }
        other => Err(unsupported("text", other)),
    }
}

fn cache(config: &RuntimeConfig) -> Result<Arc<dyn CacheStore>> {
    match config.cache {
        Backend::Sample => Ok(Arc::new(MemoryCache::new(config.cache_limit_bytes))),
        Backend::Fs => {
            let dir = config.cache_dir.clone().ok_or_else(|| {
                MmError::InvalidArgument("cache: fs には cache_dir が要る".into())
            })?;
            Ok(Arc::new(FsCache::open(dir, config.cache_limit_bytes)?))
        }
        other => Err(unsupported("cache", other)),
    }
}

/// PTS の索引を同じキャッシュに置く。FFmpeg を使うデコーダーは全部が通る共通の道なので、組み立てで 1 回決める。
fn install_index_cache(cache: &Arc<dyn CacheStore>) {
    #[cfg(feature = "ffmpeg")]
    mm_decode_ffmpeg_core::index_cache::set_index_cache(Some(cache.clone()));
    #[cfg(not(feature = "ffmpeg"))]
    let _ = cache;
}

/// レンダラーのフレームのキャッシュの設定。追い出した画像の退避先はディスクのキャッシュのときだけ
/// （メモリのモックに退避すると、メモリが二重に増える）。
fn frame_cache(config: &RuntimeConfig, cache: &Arc<dyn CacheStore>) -> FrameCacheConfig {
    FrameCacheConfig {
        memory_bytes: config.frame_memory_bytes.unwrap_or(DEFAULT_MEMORY_BYTES),
        store: (config.cache == Backend::Fs).then(|| cache.clone()),
    }
}

fn renderer(
    config: &RuntimeConfig,
    gpu: Option<&Arc<GpuContext>>,
    decoders: &Arc<dyn DecoderFactory>,
    mixers: &Arc<dyn MixerFactory>,
    services: &Services,
    cache: &Arc<dyn CacheStore>,
) -> Result<Box<dyn Renderer>> {
    let backend = config.render;
    match backend {
        Backend::Sample => Ok(Box::new(SampleRenderer::new(
            decoders.clone(),
            mixers.clone(),
            services.clone(),
        ))),
        Backend::Wgpu => Ok(Box::new(
            WgpuRenderer::new(
                gpu.ok_or_else(|| unsupported("render", backend))?.clone(),
                decoders.clone(),
                mixers.clone(),
                services.clone(),
            )
            .with_frame_cache(frame_cache(config, cache)),
        )),
        other => Err(unsupported("render", other)),
    }
}

/// レンダラーに合うノードの種類のレジストリ。モックのレンダラーはモックのノード、wgpu のレンダラーは `mm-nodes-basic`。
fn registry(render: Backend) -> Result<Registry> {
    let mut r = Registry::new();
    if render == Backend::Wgpu {
        for kind in mm_nodes_basic::all_kinds() {
            r.register(kind)?;
        }
        r.register(Arc::new(web_cef()))?;
    } else {
        r.register(Arc::new(SolidKind))?;
        r.register(Arc::new(FootageKind))?;
        r.register(Arc::new(TextKind))?;
    }
    Ok(r)
}

/// `web.cef` の種類。CEF の配布物とホストの実行ファイルがあればそれで描き、無ければモックの矩形。
fn web_cef() -> mm_nodes_cef::WebCefKind {
    match mm_host_cef::find_host() {
        Some(exe) => mm_nodes_cef::WebCefKind::with_host(Arc::new(mm_host_cef::CefLauncher), exe),
        None => {
            tracing::info!("CEF のホストが無いので web.cef はモックの矩形で描く");
            mm_nodes_cef::WebCefKind::mock()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_gpu(_: &GpuRequest) -> Result<Arc<GpuContext>> {
        Err(MmError::Unsupported("GPU が無い".into()))
    }

    #[test]
    fn auto_だけなら_gpu_が無くても_none_で続ける() {
        let config = RuntimeConfig {
            decode: Backend::Auto,
            ..RuntimeConfig::sample()
        };
        assert!(gpu_context_with(&config, no_gpu).unwrap().is_none());
    }

    #[test]
    fn gpu_が必須の組み合わせは作れなければ失敗する() {
        for config in [
            RuntimeConfig {
                render: Backend::Wgpu,
                decode: Backend::Auto,
                ..RuntimeConfig::sample()
            },
            RuntimeConfig {
                text: Backend::Glyphon,
                ..RuntimeConfig::sample()
            },
        ] {
            assert!(gpu_context_with(&config, no_gpu).is_err(), "{config:?}");
        }
    }

    #[test]
    fn fs_のキャッシュは置き場所が要り_上限を渡して開く() {
        let missing = RuntimeConfig {
            cache: Backend::Fs,
            ..RuntimeConfig::sample()
        };
        assert!(cache(&missing).is_err());
        let dir = std::env::temp_dir().join(format!("mm-runtime-cache-{}", std::process::id()));
        let config = RuntimeConfig {
            cache_dir: Some(dir.clone()),
            cache_limit_bytes: Some(1024),
            ..missing
        };
        let store = cache(&config).unwrap();
        assert_eq!((store.id(), store.limit_bytes()), ("fs", Some(1024)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 退避先はディスクのキャッシュのときだけ渡す() {
        let sample = RuntimeConfig::sample();
        let memory = cache(&sample).unwrap();
        assert!(frame_cache(&sample, &memory).store.is_none());
        let fs = RuntimeConfig {
            cache: Backend::Fs,
            ..sample
        };
        assert!(frame_cache(&fs, &memory).store.is_some());
    }

    #[test]
    fn gpu_を使わない組み合わせは作ろうとしない() {
        let config = RuntimeConfig::sample();
        let ctx = gpu_context_with(&config, |_| panic!("作らない")).unwrap();
        assert!(ctx.is_none());
    }
}
