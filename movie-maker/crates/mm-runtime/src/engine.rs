//! 組み立てた `Engine`。入口はこれだけを受け取り、実装のクレートに依存しない。

use std::path::PathBuf;
use std::sync::Arc;

use mm_audio_api::{AudioBlock, MixerFactory, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_cache_api::CacheStore;
use mm_decode_api::{
    AudioStreamInfo, DecoderFactory, EncodeReport, EncodeSettings, Encoder, EncoderFactory,
};
use mm_doc::Project;
use mm_node_api::{Quality, Services};
use mm_render_api::{AudioRequest, RangeRequest, RenderRequest, Renderer};
use mm_types::{
    CompositionId, MmError, RationalTime, RenderTargetId, Result, RgbaImage, Size, TimeRange,
};

use crate::config::RuntimeConfig;
use crate::registry::Registry;

/// エンコーダーに一度に押すコマの数。口の呼び出しを毎コマにしない。
pub const EXPORT_BATCH_FRAMES: usize = 8;

/// 書き出しの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub encode: EncodeReport,
    pub output_path: PathBuf,
}

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

    /// 種類の登録簿。
    pub fn registry(&self) -> &Registry {
        &self.parts.registry
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

    /// 時刻 t のコマを CPU の絵で返す（`mm frame`、Claude の目）。
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

    /// レンダーターゲットを書き出す。区間を描き、バッチでエンコーダーに押し、音を付けて閉じる。
    pub fn export(&mut self, project: &Project, target: &RenderTargetId) -> Result<ExportReport> {
        let span = tracing::info_span!("runtime.export");
        let _g = span.enter();
        let target = project.render_target(target)?;
        let comp = project.composition(&target.composition)?;
        if !self.parts.encoders.supports(&target.codec) {
            return Err(MmError::Unsupported(format!(
                "コーデック {} は {} が扱えない",
                target.codec,
                self.parts.encoders.id()
            )));
        }
        let settings = EncodeSettings {
            size: Size::new(target.width, target.height),
            fps: target.fps,
            codec: target.codec.clone(),
            output_path: PathBuf::from(&target.output_path),
            audio: Some(AudioStreamInfo {
                sample_rate: DEFAULT_SAMPLE_RATE,
                channels: DEFAULT_CHANNELS,
            }),
        };
        let mut encoder = self.parts.encoders.create()?;
        encoder.begin(&settings)?;

        let range = comp.range();
        self.export_frames(
            project,
            &target.composition,
            range,
            &settings,
            encoder.as_mut(),
        )?;
        let audio = self.render_audio(project, &target.composition, range)?;
        encoder.push_audio(&audio)?;
        let encode = encoder.finish()?;
        mm_trace::counter!("export.frames", encode.frames);
        Ok(ExportReport {
            encode,
            output_path: settings.output_path,
        })
    }

    /// 区間を描き、`EXPORT_BATCH_FRAMES` ずつエンコーダーに押す。
    fn export_frames(
        &mut self,
        project: &Project,
        composition: &CompositionId,
        range: TimeRange,
        settings: &EncodeSettings,
        encoder: &mut dyn Encoder,
    ) -> Result<()> {
        let mut batch: Vec<RgbaImage> = Vec::with_capacity(EXPORT_BATCH_FRAMES);
        let mut push_error: Option<MmError> = None;
        self.parts.renderer.render_range(
            &RangeRequest {
                project,
                composition,
                range,
                fps: settings.fps,
                size: settings.size,
                quality: Quality::Full,
                nodes: &self.parts.registry,
            },
            &mut |_index, frame, gpu| {
                batch.push((*gpu.download(&frame)?).clone());
                if batch.len() == EXPORT_BATCH_FRAMES {
                    if let Err(e) = encoder.push_frames(&batch) {
                        push_error = Some(e);
                    }
                    batch.clear();
                }
                Ok(())
            },
        )?;
        if let Some(e) = push_error {
            return Err(e);
        }
        if !batch.is_empty() {
            encoder.push_frames(&batch)?;
        }
        Ok(())
    }
}
