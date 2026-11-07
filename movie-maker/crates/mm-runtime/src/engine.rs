//! 組み立てた `Engine`。操作経路はこれだけを受け取り、実装のクレートに依存しない。
//! 書き出しの流れ（区間の描画、バッチの押し込み、段ごとの計測）は `mm-export` が
//! [`Engine::renderer`] と [`Engine::encoders`] を使って組む。

use std::sync::Arc;

use mm_audio_api::{AudioBlock, MixerFactory, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_cache_api::CacheStore;
use mm_decode_api::{DecoderFactory, EncoderFactory, GpuContextRef};
use mm_doc::Project;
use mm_gpu::{GpuContext, WgpuGpu};
use mm_node_api::{FrameRef, Quality, Services};
use mm_render_api::{AudioRequest, RenderRequest, Renderer, TrackLevel};
use mm_types::{
    CompositionId, GpuAdapterId, MmError, RationalTime, Result, RgbaImage, Size, TimeRange,
};

use crate::config::RuntimeConfig;
use crate::registry::Registry;

/// GPU のテクスチャのまま返したフレーム（[`Engine::render_frame_texture`]）。
#[derive(Clone)]
pub struct GpuFrame {
    /// 描いたフレーム。
    pub texture: wgpu::Texture,
    /// テクスチャを持つコンテキスト。デバイスが作り直されると別のものになるので、受け手はこれで持ち替えを知る。
    pub context: Arc<GpuContext>,
}

/// `assemble` が選んだ実装の一式。
pub struct EngineParts {
    pub config: RuntimeConfig,
    pub registry: Registry,
    pub renderer: Box<dyn Renderer>,
    pub decoders: Arc<dyn DecoderFactory>,
    pub encoders: Arc<dyn EncoderFactory>,
    pub mixers: Arc<dyn MixerFactory>,
    pub cache: Arc<dyn CacheStore>,
    pub services: Services,
    /// 組み立てで 1 回決めた GPU のコンテキスト。GPU を使わない組み合わせでは `None`。
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

    /// レンダラー。書き出し（`mm-export`）が段ごとに計測しながら呼ぶためのインターフェース。
    pub fn renderer(&mut self) -> &mut dyn Renderer {
        self.parts.renderer.as_mut()
    }

    /// エンコーダーを作るファクトリー。
    pub fn encoders(&self) -> &Arc<dyn EncoderFactory> {
        &self.parts.encoders
    }

    /// デコーダーを開くファクトリー。
    pub fn decoders(&self) -> &Arc<dyn DecoderFactory> {
        &self.parts.decoders
    }

    /// ミキサーを作るファクトリー。
    pub fn mixers(&self) -> &Arc<dyn MixerFactory> {
        &self.parts.mixers
    }

    /// キャッシュの保存先（`cache/`）。大きさの表示と「消す」に使う。
    pub fn cache(&self) -> &Arc<dyn CacheStore> {
        &self.parts.cache
    }

    /// 操作が触った ID（`mm_ops::Touches`）に頼ったキャッシュだけを捨てる。
    pub fn invalidate_touched(&mut self, ids: &[String]) {
        self.parts.renderer.invalidate_touched(ids);
    }

    /// 種類が使うほかのインターフェースの一式。
    pub fn services(&self) -> &Services {
        &self.parts.services
    }

    /// 組み立てで決めた GPU。別プロセスのホストの `HostSpec::gpu` に渡す。
    pub fn gpu_adapter(&self) -> Option<&GpuAdapterId> {
        self.parts.gpu.as_ref().map(|g| g.adapter())
    }

    /// レンダラーが描いている wgpu のコンテキスト。プレビューのサーフェスを同じデバイスで作るのに使う。
    /// デバイスが作り直されると新しいものを返す。wgpu のレンダラーでなければ `None`。
    pub fn gpu_context(&mut self) -> Option<Arc<GpuContext>> {
        let gpu = self
            .parts
            .renderer
            .gpu()
            .as_any()
            .downcast_mut::<WgpuGpu>()?;
        Some(gpu.context().clone())
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

    /// 時刻 t のフレームを CPU の画像で返す（`mm frame`、Claude の目）。
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

    /// 時刻 t のフレームを、CPU にリードバックせずに GPU のテクスチャのまま返す（プレビューのサーフェス）。
    ///
    /// テクスチャは中間形式（`FRAME_FORMAT`、線形の premultiplied）。中身はレンダラーの契約どおり直近
    /// `KEPT_RENDERED_FRAMES` 回の描画のあいだ保たれ、それより古いものは次のフレームに使い回される。
    /// 描いたコマンドは提出済みなので、同じ `context` のキューに積めばそのままサンプリングできる。
    /// wgpu のレンダラーでなければ `Unsupported`（呼び出し側は [`Engine::render_frame`] に戻る）。
    pub fn render_frame_texture(
        &mut self,
        project: &Project,
        composition: &CompositionId,
        time: RationalTime,
        size: Size,
        quality: Quality,
    ) -> Result<GpuFrame> {
        let span = tracing::info_span!("runtime.render_frame_texture", quality = ?quality);
        let _g = span.enter();
        let frame = self.parts.renderer.render_frame(&RenderRequest {
            project,
            composition,
            time,
            size,
            quality,
            nodes: &self.parts.registry,
        })?;
        let FrameRef::Gpu(tex) = frame else {
            return Err(MmError::Unsupported(
                "レンダラーが GPU のフレームを返さない".into(),
            ));
        };
        let gpu = self
            .parts
            .renderer
            .gpu()
            .as_any()
            .downcast_mut::<WgpuGpu>()
            .ok_or_else(|| MmError::Unsupported("wgpu のレンダラーでない".into()))?;
        let texture = gpu
            .texture(tex.id)
            .cloned()
            .ok_or_else(|| MmError::NotFound(format!("テクスチャ {}", tex.id)))?;
        Ok(GpuFrame {
            texture,
            context: gpu.context().clone(),
        })
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

    /// 直前の `render_audio` のトラックごとのピーク（Mixer のメーター）。
    pub fn audio_levels(&self) -> Vec<TrackLevel> {
        self.parts.renderer.audio_levels()
    }
}
