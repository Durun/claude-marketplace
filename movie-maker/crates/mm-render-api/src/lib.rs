//! レンダラーのインターフェース。プロジェクトのデータと時刻 t からレイヤーを評価して合成する。
//!
//! 契約（`mm_testkit::render::contract`）:
//! - 同じプロジェクトのデータ、同じ t、同じ大きさで 2 回描いて bit 一致。
//! - 返す画像の大きさは頼んだ大きさ。
//! - `render_range` が返すフレームの番号は `range.frame_indices(fps)` と一致し、
//!   各フレームは `render_frame` で同じ時刻を描いたものと一致する（プレビューと `mm frame` で同じフレーム）。
//! - `FrameCacheKey::digest` は同じ入力で同じ値、どのフィールドを変えても別の値。
//! - `invalidate` の後も同じ画像（キャッシュを破棄しても結果は変わらない）。

use serde::{Deserialize, Serialize};

use mm_audio_api::AudioBlock;
use mm_decode_api::Encoder;
use mm_doc::Project;
use mm_node_api::{FrameRef, GpuHelper, NodeRegistry, Quality};
use mm_types::{CompositionId, Rational, RationalTime, Result, RgbaImage, Size, TimeRange};

pub use mm_node_api::Quality as RenderQuality;

mod mix;
pub use mix::{mixer_config, TrackLevel};

/// 画面上の大きさを丸める段階。キャッシュのキーに入れる。
pub const SIZE_BUCKETS: [f32; 7] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

/// 有効に保つ、直近の `render_frame` の返り値の数。実装はこれ以上保つ。
pub const KEPT_RENDERED_FRAMES: usize = 8;

/// 倍率を段階に丸める（切り上げ）。最大の段階を超えたら最大。
pub fn size_bucket(scale: f32) -> f32 {
    SIZE_BUCKETS
        .iter()
        .copied()
        .find(|b| scale <= *b)
        .unwrap_or(SIZE_BUCKETS[SIZE_BUCKETS.len() - 1])
}

/// フレームのキャッシュのキー。`time_varying` が偽のノードの種類は `frame_index` を `None` にする。
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
    /// 全フィールドをハッシュに含めた 64 bit。同じ入力で同じ値で、Rust のバージョンやプロセスをまたいでも変わらない
    /// （ディスクのキャッシュの名前に使える）。FNV-1a で、可変長のフィールドは長さを先にハッシュに含める。
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.str(&self.kind);
        h.u64(self.kind_version.into());
        h.u64(self.renderer_version.into());
        h.str(&self.params_json);
        h.u64(self.input_keys.len() as u64);
        for key in &self.input_keys {
            h.u64(*key);
        }
        h.opt(self.content_hash.as_deref(), Fnv1a::str);
        h.opt(self.frame_index, |h, v| h.u64(v as u64));
        h.u64(match self.quality {
            Quality::Preview => 0,
            Quality::Full => 1,
        });
        h.str(&self.color_config);
        h.u64(self.size_bucket_milli.into());
        h.0
    }
}

/// バイト列の FNV-1a（64 bit）。`FrameCacheKey::input_keys` に、入れ子の中身などの指紋を入れるのに使う。
pub fn digest_bytes(bytes: &[u8]) -> u64 {
    let mut h = Fnv1a::new();
    h.bytes(bytes);
    h.0
}

/// FNV-1a の 64 bit。`DefaultHasher` と違い、値が実装のバージョンで変わらない。
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(Self::PRIME);
        }
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }

    /// 有無のフラグを先にハッシュに含めるので、`None` と空の値が混ざらない。
    fn opt<T>(&mut self, v: Option<T>, put: impl FnOnce(&mut Self, T)) {
        match v {
            Some(v) => {
                self.u64(1);
                put(self, v);
            }
            None => self.u64(0),
        }
    }
}

/// 1 フレームの要求。
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

/// レンダラー。プレビューも書き出しも同じ経路で、最後にサーフェスかリードバックかが違うだけ。
pub trait Renderer: Send {
    /// 実装の名前（`wgpu`、モックは `sample`）。
    fn id(&self) -> &'static str;
    /// レンダラーのバージョン。合成の式を変えたら上げる（キャッシュのキーに入る）。
    fn version(&self) -> u32;
    /// 時刻 t のフレーム。
    ///
    /// 返す `FrameRef::Gpu` の寿命: 実装は直近 [`KEPT_RENDERED_FRAMES`] 回の `render_frame` が返した
    /// フレームを有効に保つ（古いほうからドロップしてよい）。長く持つ側は `GpuHelper::download` で CPU にコピーする。
    /// 実装をドロップした後は無効。
    fn render_frame(&mut self, req: &RenderRequest<'_>) -> Result<FrameRef>;
    /// 区間のフレームを順に `sink` に渡す。`sink` はリードバックのために GPU のヘルパーも受け取る。
    /// 返すのは渡したフレームの数。
    ///
    /// `sink` に渡す `FrameRef::Gpu` は、その `sink` の呼び出しの中だけ有効。`sink` が返ったら
    /// 実装がドロップするので、残したいときは `sink` の中で `download` する。`sink` が `Err` を返したら
    /// そこで止め、その `Err` をそのまま返す。
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
                time: RationalTime::from_frames(index, req.fps)?,
                size: req.size,
                quality: req.quality,
                nodes: req.nodes,
            })?;
            sink(index, frame, self.gpu())?;
            count += 1;
        }
        Ok(count)
    }
    /// 区間を描いてエンコーダーに押す（書き出し）。返すのは押したフレームの数。
    ///
    /// 既定は `render_range` で描いたフレームをリードバックし、`batch` 枚ずつ `Encoder::push_frames` に押す。
    /// 実装は、GPU で YUV にしてリードバックを先行させ `Encoder::push_nv12_frames` に押すなど、
    /// 速い経路に置き換えてよい。エンコーダーが NV12 を受けなければ既定の経路に戻る。
    /// `sink` は 1 フレーム押すごとにその番号で呼ばれる（段ごとの計測のため）。
    fn export_range(
        &mut self,
        req: &RangeRequest<'_>,
        encoder: &mut dyn Encoder,
        batch: usize,
        sink: &mut dyn FnMut(i64),
    ) -> Result<usize> {
        export_range_rgba(self, req, encoder, batch, sink)
    }
    /// 区間の音を混ぜる。コンポジションのミキサーの設定（[`mixer_config`]）と、レイヤーのミュートとソロを効かせる。
    fn render_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock>;
    /// 直前の `render_audio` の、要求したコンポジションの直下のレイヤーごとのピーク。Mixer のトラックのメーターが使う。
    /// 鳴らなかったレイヤーは入らない。測らない実装は空（既定）。
    fn audio_levels(&self) -> Vec<TrackLevel> {
        Vec::new()
    }
    /// プロジェクトのデータのこの範囲が変わった。キャッシュを破棄する。
    fn invalidate(&mut self, composition: &CompositionId, range: TimeRange);
    /// 操作が触った ID（`mm_ops::Touches`。レイヤー、素材、コンポジション）に頼ったキャッシュだけを捨てる。
    /// 素材の ID が入っていれば、その素材を閉じて次に開き直す。ほかの素材は閉じない。
    /// キャッシュを持たない実装は何もしない。
    fn invalidate_touched(&mut self, ids: &[String]) {
        let _ = ids;
    }
    /// GPU のヘルパー。リードバック（`mm frame`、書き出し）に使う。
    fn gpu(&mut self) -> &mut dyn GpuHelper;
}

/// `Renderer::export_range` の既定の経路。`render_range` で描いたフレームをリードバックし、`batch` 枚ずつ
/// `Encoder::push_frames` に押す。実装が速い経路から戻るときにも使う。
pub fn export_range_rgba<R: Renderer + ?Sized>(
    renderer: &mut R,
    req: &RangeRequest<'_>,
    encoder: &mut dyn Encoder,
    batch: usize,
    sink: &mut dyn FnMut(i64),
) -> Result<usize> {
    let batch = batch.max(1);
    let mut pending: Vec<RgbaImage> = Vec::with_capacity(batch);
    let count = renderer.render_range(req, &mut |index, frame, gpu| {
        pending.push((*gpu.download(&frame)?).clone());
        sink(index);
        if pending.len() == batch {
            encoder.push_frames(&pending)?;
            pending.clear();
        }
        Ok(())
    })?;
    if !pending.is_empty() {
        encoder.push_frames(&pending)?;
    }
    Ok(count)
}

/// 先読みの描画の順を決める。計算の結果のメモは実装の側（`mm-render` の `Memo`。キーで引き、頼った ID が変わったら捨て、上限で追い出す）。
/// rustc のクエリの型（時刻とキーで引く計算と、提供者の差し替え）に倣い、
/// 提供者（このインターフェースの実装）を差し替えられる。
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
    fn キーは同じ入力で同じ値になる() {
        assert_eq!(key().digest(), key().digest());
    }

    #[test]
    fn キーは全フィールドのどれを変えても変わる() {
        let base = key().digest();
        type Edit = fn(&mut FrameCacheKey);
        let edits: Vec<(&str, Edit)> = vec![
            ("kind", |k| k.kind = "text".into()),
            ("kind_version", |k| k.kind_version = 2),
            ("renderer_version", |k| k.renderer_version = 2),
            ("params_json", |k| k.params_json = "{\"a\":1}".into()),
            ("input_keys", |k| k.input_keys = vec![7]),
            ("content_hash", |k| k.content_hash = Some("abc".into())),
            ("frame_index", |k| k.frame_index = Some(4)),
            ("frame_index None", |k| k.frame_index = None),
            ("quality", |k| k.quality = Quality::Full),
            ("color_config", |k| k.color_config = "aces".into()),
            ("size_bucket_milli", |k| k.size_bucket_milli = 2000),
        ];
        for (name, edit) in edits {
            let mut k = key();
            edit(&mut k);
            assert_ne!(k.digest(), base, "{name} を変えると値が変わる");
        }
    }

    #[test]
    fn フィールドの境目をまたぐ移し替えは別の値になる() {
        let mut a = key();
        a.kind = "ab".into();
        a.params_json = "c".into();
        let mut b = key();
        b.kind = "a".into();
        b.params_json = "bc".into();
        assert_ne!(a.digest(), b.digest());
        let mut none = key();
        none.content_hash = None;
        let mut empty = key();
        empty.content_hash = Some(String::new());
        assert_ne!(none.digest(), empty.digest());
    }

    #[test]
    fn キーの値は実装のバージョンによらず固定() {
        // ディスクのキャッシュの名前になるので、この値を変える変更はキャッシュのバージョンを上げる変更。
        assert_eq!(key().digest(), 0x8c3f_77f0_eb41_6954);
    }
}
