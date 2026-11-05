//! `assemble(config) -> Engine`。実装のクレートを知っているのはこのファイルだけ。

use std::sync::Arc;

use mm_audio_api::MixerFactory;
use mm_cache_api::CacheStore;
use mm_color_api::ColorTransform;
use mm_decode_api::{DecoderFactory, EncoderFactory};
use mm_node_api::Services;
use mm_render_api::Renderer;
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::cache::MemoryCache;
use mm_testkit::color::SampleColor;
use mm_testkit::decode::{SampleDecoderFactory, SampleEncoderFactory};
use mm_testkit::node::{FootageKind, SolidKind, TextKind};
use mm_testkit::render::SampleRenderer;
use mm_testkit::text::{SampleRasterizer, SampleShaper};
use mm_text_api::{TextRasterizer, TextShaper};
use mm_types::Result;

use crate::config::{Backend, RuntimeConfig};
use crate::engine::{Engine, EngineParts};
use crate::registry::Registry;

/// 設定から実装を選んで `Engine` を作る。
pub fn assemble(config: &RuntimeConfig) -> Result<Engine> {
    let decoders: Arc<dyn DecoderFactory> = match config.decode {
        Backend::Sample => Arc::new(SampleDecoderFactory::default()),
    };
    let encoders: Arc<dyn EncoderFactory> = match config.encode {
        Backend::Sample => Arc::new(SampleEncoderFactory),
    };
    let mixers: Arc<dyn MixerFactory> = match config.audio {
        Backend::Sample => Arc::new(SampleMixerFactory),
    };
    let color: Arc<dyn ColorTransform> = match config.color {
        Backend::Sample => Arc::new(SampleColor),
    };
    let (text_shaper, text_rasterizer): (Arc<dyn TextShaper>, Arc<dyn TextRasterizer>) =
        match config.text {
            Backend::Sample => (Arc::new(SampleShaper), Arc::new(SampleRasterizer)),
        };
    let cache: Arc<dyn CacheStore> = match config.cache {
        Backend::Sample => Arc::new(MemoryCache::new(config.cache_limit_bytes)),
    };
    let services = Services {
        text_shaper,
        text_rasterizer,
        color,
    };
    let renderer: Box<dyn Renderer> = match config.render {
        Backend::Sample => Box::new(SampleRenderer::new(
            decoders.clone(),
            mixers.clone(),
            services.clone(),
        )),
    };
    Ok(Engine::from_parts(EngineParts {
        config: config.clone(),
        registry: default_registry()?,
        renderer,
        decoders,
        encoders,
        mixers,
        cache,
        services,
    }))
}

/// 最初から使える種類。実装のクレートが増えたらここに足す。
fn default_registry() -> Result<Registry> {
    let mut r = Registry::new();
    r.register(Arc::new(SolidKind))?;
    r.register(Arc::new(FootageKind))?;
    r.register(Arc::new(TextKind))?;
    Ok(r)
}
