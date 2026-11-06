//! 組み立てた `Engine`。入口はこれだけを受け取り、実装のクレートに依存しない。
//! 書き出しの流れ（区間の描画、バッチの押し込み、段ごとの計測）は `mm-export` が
//! [`Engine::renderer`] と [`Engine::encoders`] を使って組む。

use std::sync::Arc;

use mm_audio_api::{AudioBlock, MixerFactory, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_cache_api::CacheStore;
use mm_decode_api::{DecoderFactory, EncoderFactory, GpuContextRef};
use mm_doc::Project;
use mm_node_api::{Quality, Services};
use mm_render_api::{AudioRequest, RenderRequest, Renderer};
use mm_types::{CompositionId, GpuAdapterId, RationalTime, Result, RgbaImage, Size, TimeRange};

use crate::config::RuntimeConfig;
use crate::registry::Registry;

/// `assemble` が選んだ実装の束。
pub struct EngineParts {
    pub config: RuntimeConfig,
    pub registry: Registry,
    pub renderer: Box<dyn Renderer>,
    pub decoders: Arc<dyn DecoderFactory>,
    pub encoders: Arc<dyn EncoderFactory>,
    pub mixers: Arc<dyn MixerFactory>,
    pub cache: Arc<dyn CacheStore>,
    pub services: Services,
    /// 組み立てで 1 回決めた GPU の文脈。GPU を使わない組み合わせでは `None`。
    pub gpu: Option<Arc<dyn GpuContextRef>>,
}

/// 組み立て済みのエンジン。
pub struct Engine {
    parts: EngineParts,
}

impl Engine {
    pub(crate) fn from_parts(parts: EngineParts) -> Self {
        Self { parts }
    }

    /// 組み立てに使った設定。
    pub fn config(&self) -> &RuntimeConfig {
        &self.parts.config
    }

    /// 種類のレジストリ。
    pub fn registry(&self) -> &Registry {
        &self.parts.registry
    }

    /// レンダラー。書き出し（`mm-export`）が段ごとに計測しながら呼ぶための口。
    pub fn renderer(&mut self) -> &mut dyn Renderer {
        self.parts.renderer.as_mut()
    }

    /// エンコーダーを作る口。
    pub fn encoders(&self) -> &Arc<dyn EncoderFactory> {
        &self.parts.encoders
    }

    /// デコーダーを開く口。
    pub fn decoders(&self) -> &Arc<dyn DecoderFactory> {
        &self.parts.decoders
    }

    /// ミキサーを作る口。
    pub fn mixers(&self) -> &Arc<dyn MixerFactory> {
        &self.parts.mixers
    }

    /// 種類が使うほかの口の束。
    pub fn services(&self) -> &Services {
        &self.parts.services
    }

    /// 組み立てで決めた GPU。別プロセスのホストの `HostSpec::gpu` に渡す。
    pub fn gpu_adapter(&self) -> Option<&GpuAdapterId> {
        self.parts.gpu.as_ref().map(|g| g.adapter())
    }

    /// 選ばれた実装の名前（Program に出す）。
    pub fn backend_names(&self) -> Vec<(&'static str, &'static str)> {
        let p = &self.parts;
        vec![
            ("render", p.renderer.id()),
            ("decode", p.decoders.id()),
            ("encode", p.encoders.id()),
            ("audio", p.mixers.id()),
            ("color", p.services.color.id()),
            ("text", p.services.text_shaper.id()),
            ("cache", p.cache.id()),
        ]
    }

    /// 時刻 t のフレームを CPU の絵で返す（`mm frame`、Claude の目）。
    pub fn render_frame(
        &mut self,
        project: &Project,
        composition: &CompositionId,
        time: RationalTime,
        size: Size,
        quality: Quality,
    ) -> Result<Arc<RgbaImage>> {
        let span = tracing::info_span!("runtime.render_frame", quality = ?quality);
        let _g = span.enter();
        let frame = self.parts.renderer.render_frame(&RenderRequest {
            project,
            composition,
            time,
            size,
            quality,
            nodes: &self.parts.registry,
        })?;
        self.parts.renderer.gpu().download(&frame)
    }

    /// 区間の音。
    pub fn render_audio(
        &mut self,
        project: &Project,
        composition: &CompositionId,
        range: TimeRange,
    ) -> Result<AudioBlock> {
        let span = tracing::info_span!("runtime.render_audio");
        let _g = span.enter();
        self.parts.renderer.render_audio(&AudioRequest {
            project,
            composition,
            range,
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
            nodes: &self.parts.registry,
        })
    }
}
