//! `assemble(config) -> Engine`。実装のクレートを知っているのはこのファイルだけ。

use std::sync::Arc;

use mm_audio_api::MixerFactory;
use mm_audio_dsp::DspMixerFactory;
use mm_cache_api::CacheStore;
use mm_color_api::ColorTransform;
use mm_decode_api::{DecoderFactory, EncoderFactory, GpuContextRef};
use mm_gpu::{GpuContext, GpuRequest};
use mm_node_api::Services;
use mm_render::WgpuRenderer;
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
use crate::decode_select::AutoDecoderFactory;
#[cfg(any(feature = "d3d12va", feature = "cuvid"))]
use crate::decode_select::WithAudio;
use crate::engine::{Engine, EngineParts};
use crate::registry::Registry;

/// 設定から実装を選んで `Engine` を作る。
pub fn assemble(config: &RuntimeConfig) -> Result<Engine> {
    let decoders = decoders(config.decode)?;
    let encoders = encoders(config.encode)?;
    let mixers = mixers(config.audio)?;
    let color: Arc<dyn ColorTransform> = match config.color {
        Backend::Sample => Arc::new(SampleColor),
        other => return Err(unsupported("color", other)),
    };
    let (text_shaper, text_rasterizer) = text(config.text)?;
    let cache: Arc<dyn CacheStore> = match config.cache {
        Backend::Sample => Arc::new(MemoryCache::new(config.cache_limit_bytes)),
        other => return Err(unsupported("cache", other)),
    };
    let services = Services {
        text_shaper,
        text_rasterizer,
        color,
    };
    let gpu = gpu_context(config)?;
    let renderer = renderer(config.render, gpu.as_ref(), &decoders, &mixers, &services)?;
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

/// 口に合わない実装を選んだときの断り。
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

/// 入っている GPU の実装を、試す順（d3d12va、cuvid）に。
fn gpu_decoders() -> Vec<Arc<dyn DecoderFactory>> {
    vec![
        #[cfg(feature = "d3d12va")]
        with_cpu_audio(Arc::new(mm_decode_d3d12va::D3d12vaFactory)),
        #[cfg(feature = "cuvid")]
        with_cpu_audio(Arc::new(mm_decode_cuvid::CuvidFactory)),
    ]
}

/// `auto`: 入っている GPU の実装を d3d12va、cuvid の順に並べ、最後に ffmpeg。何も入っていなければ断る。
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

/// GPU を使う実装があるときだけ、文脈を 1 回作る。設定のアダプターが無ければ高性能のもの。
/// 要否は描画器とデコーダーの両方の選択で決まる（GPU で復号する実装は描画器と同じデバイスが要る）。
fn gpu_context(config: &RuntimeConfig) -> Result<Option<Arc<GpuContext>>> {
    if !config.needs_gpu() {
        return Ok(None);
    }
    Ok(Some(GpuContext::shared(&gpu_request(config)?)?))
}

/// 文脈の頼み方。GPU で復号する実装は D3D12 のデバイスが要る（FFmpeg と CUDA に渡す）ので、
/// 描画器もそのアダプターで作る。`auto` は D3D12 が無ければ既定の頼み方に戻し、ffmpeg で動く。
fn gpu_request(config: &RuntimeConfig) -> Result<GpuRequest> {
    let default = GpuRequest {
        adapter: config.gpu.clone(),
        ..GpuRequest::default()
    };
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

/// 整形器と描画器の組。glyphon はフォントの読み込みを共有するため同じ `TextEngine` から両方を取る。
fn text(backend: Backend) -> Result<(Arc<dyn TextShaper>, Arc<dyn TextRasterizer>)> {
    match backend {
        Backend::Sample => Ok((Arc::new(SampleShaper), Arc::new(SampleRasterizer))),
        Backend::Glyphon => {
            let engine = TextEngine::new();
            Ok((engine.shaper(), engine.rasterizer()))
        }
        other => Err(unsupported("text", other)),
    }
}

fn renderer(
    backend: Backend,
    gpu: Option<&Arc<GpuContext>>,
    decoders: &Arc<dyn DecoderFactory>,
    mixers: &Arc<dyn MixerFactory>,
    services: &Services,
) -> Result<Box<dyn Renderer>> {
    match backend {
        Backend::Sample => Ok(Box::new(SampleRenderer::new(
            decoders.clone(),
            mixers.clone(),
            services.clone(),
        ))),
        Backend::Wgpu => Ok(Box::new(WgpuRenderer::new(
            gpu.ok_or_else(|| unsupported("render", backend))?.clone(),
            decoders.clone(),
            mixers.clone(),
            services.clone(),
        ))),
        other => Err(unsupported("render", other)),
    }
}

/// 描画器に合う種類の登録簿。見本の描画器は見本の種類、wgpu の描画器は `mm-nodes-basic`。
fn registry(render: Backend) -> Result<Registry> {
    let mut r = Registry::new();
    if render == Backend::Wgpu {
        for kind in mm_nodes_basic::all_kinds() {
            r.register(kind)?;
        }
    } else {
        r.register(Arc::new(SolidKind))?;
        r.register(Arc::new(FootageKind))?;
        r.register(Arc::new(TextKind))?;
    }
    Ok(r)
}
