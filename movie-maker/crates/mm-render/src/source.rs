//! 素材の供給元。デコーダーを時刻で引ける形にし、同じフレームの連続した要求はデコードし直さない。

use std::collections::HashMap;
use std::path::Path;

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, FrameSource, GpuContextRef, MediaInfo};
use mm_doc::Project;
use mm_types::{AssetId, MmError, ParamValue, Params, RationalTime, Result, TimeRange};

/// `Decoder` を `FrameSource` にする。直前のフレームを保持する。
pub struct DecoderSource {
    decoder: Box<dyn Decoder>,
    last: Option<DecodedFrame>,
}

impl DecoderSource {
    /// デコーダーをラップする。
    pub fn new(decoder: Box<dyn Decoder>) -> Self {
        Self {
            decoder,
            last: None,
        }
    }
}

impl FrameSource for DecoderSource {
    fn info(&self) -> &MediaInfo {
        self.decoder.info()
    }

    fn frame_at(&mut self, t: RationalTime) -> Result<DecodedFrame> {
        let index = self.decoder.info().frame_index_at(t);
        if let Some(last) = self.last.as_ref().filter(|l| l.index == index) {
            return Ok(last.clone());
        }
        self.decoder.seek(t)?;
        let frame = self
            .decoder
            .next_frame()?
            .ok_or_else(|| MmError::NotFound(format!("フレーム {index}（t = {t}）")))?;
        self.last = Some(frame.clone());
        Ok(frame)
    }

    fn audio_in(&mut self, range: TimeRange) -> Result<AudioBlock> {
        self.decoder.decode_audio(range)
    }
}

/// 開いた素材のキャッシュ。素材ごとに 1 度だけ開いて使い回す。
#[derive(Default)]
pub struct SourcePool {
    sources: HashMap<AssetId, Box<dyn FrameSource>>,
}

impl SourcePool {
    /// 全部閉じる。素材の内容が変わったとき。
    pub fn clear(&mut self) {
        self.sources.clear();
    }

    /// 評価済みのパラメータの `AssetRef` の素材を返す（無ければ開く）。`AssetRef` が無いか空なら `None`。
    pub fn footage_for<'s>(
        &'s mut self,
        decoders: &dyn DecoderFactory,
        gpu: &dyn GpuContextRef,
        project: &Project,
        params: &Params,
    ) -> Result<Option<&'s mut (dyn FrameSource + 's)>> {
        let Some(asset_id) = params.0.values().find_map(ParamValue::as_asset) else {
            return Ok(None);
        };
        if asset_id.as_str().is_empty() {
            return Ok(None);
        }
        if !self.sources.contains_key(asset_id) {
            let asset = project.asset(asset_id)?;
            let decoder = decoders.open(Path::new(&asset.path), Some(gpu))?;
            self.sources
                .insert(asset_id.clone(), Box::new(DecoderSource::new(decoder)));
        }
        Ok(mm_node_api::footage_ref(self.sources.get_mut(asset_id)))
    }
}
