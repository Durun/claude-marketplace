//! 描画器の口。文書と時刻 t からレイヤーを評価して合成する。
//!
//! 契約（`mm_testkit::render::contract`）:
//! - 同じ文書、同じ t、同じ大きさで 2 回描いて bit 一致。
//! - 返す絵の大きさは頼んだ大きさ。
//! - `render_range` が返すコマの番号は `range.frame_indices(fps)` と一致し、
//!   各コマは `render_frame` で同じ時刻を描いたものと一致する（プレビューと `mm frame` で同じコマ）。
//! - `FrameCacheKey::digest` は同じ入力で同じ値、どの欄を変えても別の値。
//! - `invalidate` の後も同じ絵（キャッシュを捨てても結果は変わらない）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use mm_audio_api::AudioBlock;
use mm_doc::Project;
use mm_node_api::{FrameRef, GpuHelper, NodeRegistry, Quality};
use mm_types::{CompositionId, Rational, RationalTime, Result, Size, TimeRange};

pub use mm_node_api::Quality as RenderQuality;

/// 画面上の大きさを丸める段階。キャッシュのキーに入れる。
pub const SIZE_BUCKETS: [f32; 7] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

/// 倍率を段階に丸める（切り上げ）。最大の段階を超えたら最大。
pub fn size_bucket(scale: f32) -> f32 {
    SIZE_BUCKETS
        .iter()
        .copied()
        .find(|b| scale <= *b)
        .unwrap_or(SIZE_BUCKETS[SIZE_BUCKETS.len() - 1])
}

/// コマのキャッシュのキー。`time_varying` が偽の種類は `frame_index` を `None` にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameCacheKey {
    pub kind: String,
    pub kind_version: u32,
    pub renderer_version: u32,
    /// 評価済みのパラメータの決定的な JSON。
    pub params_json: String,
    pub input_keys: Vec<u64>,
    pub content_hash: Option<String>,
    pub frame_index: Option<i64>,
    pub quality: Quality,
    /// 作業色空間と OCIO の設定を畳んだもの。
    pub color_config: String,
    /// `size_bucket` で丸めた倍率を 1000 倍した整数。
    pub size_bucket_milli: u32,
}

impl FrameCacheKey {
    /// 全欄を畳んだ 64 bit。同じ入力で同じ値。
    pub fn digest(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.kind.hash(&mut h);
        self.kind_version.hash(&mut h);
        self.renderer_version.hash(&mut h);
        self.params_json.hash(&mut h);
        self.input_keys.hash(&mut h);
        self.content_hash.hash(&mut h);
        self.frame_index.hash(&mut h);
        self.quality.hash(&mut h);
        self.color_config.hash(&mut h);
        self.size_bucket_milli.hash(&mut h);
        h.finish()
    }
}

/// 1 コマの要求。
pub struct RenderRequest<'a> {
    pub project: &'a Project,
    pub composition: &'a CompositionId,
    pub time: RationalTime,
    pub size: Size,
    pub quality: Quality,
    pub nodes: &'a dyn NodeRegistry,
}

/// 区間の要求。
pub struct RangeRequest<'a> {
    pub project: &'a Project,
    pub composition: &'a CompositionId,
    pub range: TimeRange,
    pub fps: Rational,
    pub size: Size,
    pub quality: Quality,
    pub nodes: &'a dyn NodeRegistry,
}

/// 音の要求。
pub struct AudioRequest<'a> {
    pub project: &'a Project,
    pub composition: &'a CompositionId,
    pub range: TimeRange,
    pub sample_rate: u32,
    pub channels: u16,
    pub nodes: &'a dyn NodeRegistry,
}

/// 描画器。プレビューも書き出しも同じ経路で、最後に面か読み戻しかが違うだけ。
pub trait Renderer: Send {
    /// 実装の名前（`wgpu`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// 描画器の版。合成の式を変えたら上げる（キャッシュのキーに入る）。
    fn version(&self) -> u32;
    /// 時刻 t のコマ。
    fn render_frame(&mut self, req: &RenderRequest<'_>) -> Result<FrameRef>;
    /// 区間のコマを順に `sink` に渡す。`sink` は読み戻しのために GPU のヘルパーも受け取る。
    /// 返すのは渡したコマの数。
    fn render_range(
        &mut self,
        req: &RangeRequest<'_>,
        sink: &mut dyn FnMut(i64, FrameRef, &mut dyn GpuHelper) -> Result<()>,
    ) -> Result<usize> {
        let mut count = 0;
        for index in req.range.frame_indices(req.fps) {
            let frame = self.render_frame(&RenderRequest {
                project: req.project,
                composition: req.composition,
                time: RationalTime::from_frames(index, req.fps),
                size: req.size,
                quality: req.quality,
                nodes: req.nodes,
            })?;
            sink(index, frame, self.gpu())?;
            count += 1;
        }
        Ok(count)
    }
    /// 区間の音を混ぜる。
    fn render_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock>;
    /// 文書のこの範囲が変わった。キャッシュを捨てる。
    fn invalidate(&mut self, composition: &CompositionId, range: TimeRange);
    /// GPU のヘルパー。読み戻し（`mm frame`、書き出し）に使う。
    fn gpu(&mut self) -> &mut dyn GpuHelper;
}

/// 先回りで描く順を決める。rustc のクエリの型（時刻とキーで引く計算と、提供者の差し替え）に寄せ、
/// 提供者（この口の実装）を差し替えられる。
pub trait Scheduler: Send {
    /// 再生位置と、変わった範囲から、先に描くべき範囲を優先順に返す。
    fn prioritize(&mut self, playhead: RationalTime, dirty: &[TimeRange]) -> Vec<TimeRange>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 倍率は段階に切り上げる() {
        assert_eq!(size_bucket(0.7), 1.0);
        assert_eq!(size_bucket(1.2), 1.5);
        assert_eq!(size_bucket(100.0), 8.0);
    }

    fn key() -> FrameCacheKey {
        FrameCacheKey {
            kind: "solid".into(),
            kind_version: 1,
            renderer_version: 1,
            params_json: "{}".into(),
            input_keys: vec![],
            content_hash: None,
            frame_index: Some(3),
            quality: Quality::Preview,
            color_config: "lin_rec709".into(),
            size_bucket_milli: 1000,
        }
    }

    #[test]
    fn キーは同じ入力で同じで欄を変えると変わる() {
        assert_eq!(key().digest(), key().digest());
        let mut k = key();
        k.frame_index = Some(4);
        assert_ne!(k.digest(), key().digest());
        let mut k = key();
        k.quality = Quality::Full;
        assert_ne!(k.digest(), key().digest());
    }
}
