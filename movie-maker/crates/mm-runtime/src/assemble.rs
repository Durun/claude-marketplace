//! `assemble(config) -> Engine`。実装のクレートを知っているのはこのファイルだけ。

use std::sync::Arc;

use mm_audio_api::MixerFactory;
use mm_audio_dsp::DspMixerFactory;
use mm_cache_api::CacheStore;
use mm_color_api::ColorTransform;
use mm_color_lut::LutColor;
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
use mm_types::{MmError, Result};

use crate::config::{Backend, RuntimeConfig};
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
    let cache: Arc<dyn CacheStore> = match config.cache {
        Backend::Sample => Arc::new(MemoryCache::new(config.cache_limit_bytes)),
        other => return Err(unsupported("cache", other)),
    };
    let services = Services {
        text_shaper,
        text_rasterizer,
        color,
    };
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
        other => Err(unsupported("decode", other)),
    }
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
    let request = GpuRequest {
        adapter: config.gpu.clone(),
        ..GpuRequest::default()
    };
    Ok(Some(GpuContext::shared(&request)?))
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

/// 整形器と描画器の組。cpu と glyphon はフォントの読み込みを共有するため同じ `TextEngine` から両方を取る。
/// glyphon は GPU の文脈が要る（`RuntimeConfig::needs_gpu` が文脈を作らせる）。
fn text(
    backend: Backend,
    gpu: Option<&Arc<GpuContext>>,
) -> Result<(Arc<dyn TextShaper>, Arc<dyn TextRasterizer>)> {
    match backend {
        Backend::Sample => Ok((Arc::new(SampleShaper), Arc::new(SampleRasterizer))),
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
