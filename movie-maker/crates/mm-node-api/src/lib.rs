//! ノードのインターフェース。OpenFX のプラグイン API に倣う（時刻で引ける入力、描く領域、
//! 恒等の宣言、性質の宣言）。
//!
//! GPU は [`GpuHelper`] で囲い、インターフェースには wgpu の型を出さない。`mm-render` が wgpu で実装し、
//! モックと契約テストは CPU の実装で回る。wgpu の `CommandEncoder` が要るノードの種類は
//! `GpuHelper::as_any` で実装の型に降りる（降りられない実装では `Err(Unsupported)`）。
//!
//! 契約（`mm_testkit::node::contract`）:
//! - `params().defaults()` で `render` が成功する。
//! - 端（t = 0、t = duration、範囲の外）で panic しない。
//! - `vector` なら `scale = 2.0` で描けて、画像の大きさが 2 倍になる。
//! - `deterministic` なら 2 回描いて bit 一致。
//! - `time_varying` が偽なら t を変えても同じ画像。
//! - `is_identity` が真なら入力 0 と同じ画像。
//! - `region_of_definition` の外は透明。
//! - `migrate(今のバージョンの値, 今のバージョン)` は恒等。
//! - `render` の `Err` はそのレイヤーだけ透明とフラグ（レンダラーの契約。種類は `Err` を返すだけでよい）。

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use mm_audio_api::AudioBlock;
use mm_color_api::ColorTransform;
use mm_decode_api::{FrameSource, GpuFrameHandle};
use mm_text_api::{TextRasterizer, TextShaper};
use mm_types::{ParamSchema, Params, RationalTime, Rect, Result, Rgba, RgbaImage, Size, TimeRange};

/// インターフェースのバージョン。`NodeKind` の形を変えたら上げる（RFC）。
pub const NODE_API_VERSION: u32 = 1;

/// 計算の重さ。スケジューラが先読みの描画の順を決める手がかり。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cost {
    Light,
    Medium,
    Heavy,
}

/// どこで動くか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// Rust の種類。`Err` と panic はレンダラーが受け、そのレイヤーだけ透明にする。
    InProcess,
    /// 別プロセス。クラッシュしたらフラグを立てて起動し直す。
    OutOfProcess,
}

/// ノードの種類の性質の自己申告。契約テストが `deterministic` と `time_varying` を確かめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Traits {
    /// 再生に間に合う。偽ならキャッシュ経由で先読みして描く。
    pub realtime: bool,
    /// 同じ入力で同じ画像。
    pub deterministic: bool,
    /// 生成 AI など、待つのが前提。
    pub offline: bool,
    /// t で画像が変わる。偽ならキャッシュのキーから t を外す。
    pub time_varying: bool,
    /// 音を出す。
    pub audio: bool,
    /// ベクター。ラスタにするのを最後まで遅らせる。
    pub vector: bool,
    /// 画像を出す。
    pub pixel: bool,
    pub cost: Cost,
    pub isolation: Isolation,
}

impl Traits {
    /// プロセス内で動く、決定的な、時間で変わる画像のノードの種類の既定。
    pub const PIXEL: Self = Self {
        realtime: true,
        deterministic: true,
        offline: false,
        time_varying: true,
        audio: false,
        vector: false,
        pixel: true,
        cost: Cost::Light,
        isolation: Isolation::InProcess,
    };
}

/// 入力 1 本の定義。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSpec {
    pub name: String,
    pub optional: bool,
}

/// 効果やトランジションが受ける映像の定義。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InputSchema {
    pub inputs: Vec<InputSpec>,
}

/// 描く品質。プレビューはプレビュー動画、書き出しは原本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Preview,
    Full,
}

/// GPU のテクスチャへの参照。内容は `GpuHelper` の実装だけが解釈する。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GpuTexture {
    pub id: u64,
    pub size: Size,
}

/// 描いたフレーム。`Rgba16Float` 相当、premultiplied、線形。
#[derive(Debug, Clone, PartialEq)]
pub enum FrameRef {
    /// 何も描かない（透明）。レンダラーは合成を飛ばせる。
    Transparent { size: Size },
    /// CPU の画像。`GpuHelper::upload` で GPU に載せる。
    Cpu(Arc<RgbaImage>),
    /// GPU のテクスチャ。
    Gpu(GpuTexture),
}

impl FrameRef {
    /// CPU の画像から。
    pub fn cpu(image: RgbaImage) -> Self {
        Self::Cpu(Arc::new(image))
    }

    /// 大きさ。
    pub fn size(&self) -> Size {
        match self {
            Self::Transparent { size } => *size,
            Self::Cpu(img) => img.size(),
            Self::Gpu(tex) => tex.size,
        }
    }
}

/// GPU のヘルパー。`wgpu::Device` そのものは渡さず、要る操作だけを出す（RFC 0001）。
///
/// テクスチャの寿命: `upload` / `allocate` / `import` が返した `Gpu` のフレームはヘルパーが持ち、
/// `release` で解放する。レンダラーは合成を提出したら、そのフレームで使ったテクスチャを `release` する。
pub trait GpuHelper {
    /// CPU の画像をテクスチャにする。
    fn upload(&mut self, image: &RgbaImage) -> Result<FrameRef>;
    /// テクスチャを CPU にリードバックする。`Cpu` と `Transparent` はそのまま画像にする。
    fn download(&mut self, frame: &FrameRef) -> Result<Arc<RgbaImage>>;
    /// 透明のテクスチャを確保する。
    fn allocate(&mut self, size: Size) -> Result<FrameRef>;
    /// 1 色（線形の premultiplied）で塗ったテクスチャを確保する。GPU で塗れる実装は CPU の画像を作らない。
    fn fill(&mut self, size: Size, color: Rgba) -> Result<FrameRef> {
        self.upload(&RgbaImage::filled(size, color))
    }
    /// テクスチャを解放する。`Cpu` と `Transparent` は何もしない。
    fn release(&mut self, frame: &FrameRef);
    /// デコーダーの GPU のフレームを、このヘルパーのテクスチャとして使えるようにする。
    /// 返すフレームは次の `submit` が終わるまでの借用で、レンダラーが `submit` の後に `release` する。
    /// 内部の型を知らないヘルパーは `Err(Unsupported)`。
    fn import(&mut self, frame: &GpuFrameHandle) -> Result<FrameRef> {
        Err(mm_types::MmError::Unsupported(format!(
            "GPU のフレーム {} を受けられない",
            frame.id
        )))
    }
    /// 実装の型へダウンキャストするためのメソッド。wgpu の `CommandEncoder` が要るノードの種類だけが使う。
    fn as_any(&mut self) -> &mut dyn Any;
}

/// ノードの種類が使うほかのインターフェースの一式。`mm-runtime` が組み立てて渡す。
#[derive(Clone)]
pub struct Services {
    pub text_shaper: Arc<dyn TextShaper>,
    pub text_rasterizer: Arc<dyn TextRasterizer>,
    pub color: Arc<dyn ColorTransform>,
}

/// 時刻で引ける入力の映像。
pub trait InputFrames {
    /// 入力 `index` の時刻 `t` のフレーム。
    fn frame(&mut self, index: usize, t: RationalTime) -> Result<FrameRef>;
    /// 入力の数。
    fn len(&self) -> usize;
    /// 入力が無いか。
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `prepare` のコンテキスト。
pub struct PrepareCx<'a> {
    /// 準備にかけてよい時間。過ぎたら `Err(Timeout)` で戻る。
    pub deadline: Duration,
    /// 素材の供給元。`footage` と `audio` のノードの種類が使う。
    pub footage: Option<&'a mut dyn FrameSource>,
}

/// 1 フレームを描くコンテキスト。
pub struct FrameCx<'a> {
    /// コンポジションの時間軸での時刻。レイヤーの写像はレンダラーが済ませてある。
    pub time: RationalTime,
    /// 評価済みのパラメータ（既定値で埋めてある）。
    pub params: &'a Params,
    /// 出力の大きさ（画素）。
    pub size: Size,
    /// このフレームで要る領域。外は描かなくてよい。
    pub region: Rect,
    /// 画面上の倍率。`vector` のノードの種類はこの倍率で描く。
    pub scale: f32,
    pub quality: Quality,
    /// 入力の映像。
    pub inputs: &'a mut dyn InputFrames,
    /// 素材。`footage` のノードの種類にレンダラーが渡す。時間軸はすでに素材のもの。
    pub footage: Option<&'a mut dyn FrameSource>,
    /// 素材の時間軸での時刻。`footage` が `Some` のときだけ意味を持つ。
    pub source_time: RationalTime,
    pub gpu: &'a mut dyn GpuHelper,
    pub services: &'a Services,
}

/// 音を出すコンテキスト。
pub struct AudioCx<'a> {
    /// コンポジションの時間軸での区間。
    pub range: TimeRange,
    /// 素材の時間軸での区間。
    pub source_range: TimeRange,
    pub params: &'a Params,
    pub sample_rate: u32,
    pub channels: u16,
    pub footage: Option<&'a mut dyn FrameSource>,
}

/// ノードの種類。実装は `mm-nodes-*` に 1 種類ずつ置く。
pub trait NodeKind: Send + Sync {
    /// ノードの種類の名前（`footage`、`text`、`web.cef`、`shader.wgsl`）。プロジェクトのデータの `Node.kind` に入る。
    fn id(&self) -> &'static str;
    /// ノードの種類のバージョン。パラメータの形を変えたら上げ、`migrate` で古い値を新しいバージョンにする。
    fn version(&self) -> u32;
    /// パラメータの定義。UI の入力フィールドと MCP の引数はここから作る。
    fn params(&self) -> ParamSchema;
    /// 受ける映像の定義。
    fn inputs(&self) -> InputSchema {
        InputSchema::default()
    }
    /// 性質。
    fn traits(&self) -> Traits;
    /// 自前の長さ（素材、音）。無ければ `None` で、レイヤーの in/out に従う。
    fn duration(&self, params: &Params) -> Option<RationalTime> {
        let _ = params;
        None
    }
    /// バージョン `from` のパラメータの値を今のバージョンに上げる。`from == version()` なら恒等。
    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params>;
    /// 重い準備（素材の読み込み、生成 AI、ホストの起動）。`render` の前に 1 回。
    fn prepare(&self, params: &Params, cx: &mut PrepareCx<'_>) -> Result<()> {
        let _ = (params, cx);
        Ok(())
    }
    /// このフレームを描くのに要る入力の時刻の範囲（モーションブラー、トランジション）。
    fn frames_needed(&self, cx: &FrameCx<'_>) -> Vec<TimeRange> {
        vec![TimeRange::new(cx.time, cx.time).expect("長さ 0 の範囲")]
    }
    /// 画像が存在する領域。画面の外ならレンダラーはスキップする。
    fn region_of_definition(&self, cx: &FrameCx<'_>) -> Rect {
        cx.size.to_rect()
    }
    /// 入力 0 をそのまま返すだけか。真ならレンダラーはスキップする。
    fn is_identity(&self, cx: &FrameCx<'_>) -> bool {
        let _ = cx;
        false
    }
    /// 時刻 t のフレーム。
    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef>;
    /// 区間の音。`traits().audio` が偽なら無音を返してよい。
    fn render_audio(&self, cx: &mut AudioCx<'_>) -> Result<AudioBlock> {
        let frames = audio_frames_in(cx.range, cx.sample_rate);
        Ok(AudioBlock::silence(cx.sample_rate, cx.channels, frames))
    }
}

/// 持っている `Box<dyn FrameSource>` を `FrameCx::footage` の形にする。
/// `&mut Box<dyn FrameSource + 'static>` から `&mut (dyn FrameSource + 'a)` への縮めは関数の返り値でだけ通る。
pub fn footage_ref<'a>(
    source: Option<&'a mut Box<dyn FrameSource>>,
) -> Option<&'a mut (dyn FrameSource + 'a)> {
    match source {
        Some(b) => Some(&mut **b),
        None => None,
    }
}

/// 区間に入る音のフレーム数。`floor(end * rate) - floor(start * rate)` で、隣の区間と重ならない。
pub fn audio_frames_in(range: TimeRange, sample_rate: u32) -> usize {
    // サンプルレート 0 は音が無いのと同じ。
    let Ok(rate) = mm_types::Rational::new(sample_rate, 1) else {
        return 0;
    };
    let n = range.end.frame_index(rate) - range.start.frame_index(rate);
    n.max(0) as usize
}

/// ノードの種類のレジストリ。`mm-runtime` が実装し、レンダラーが `Node.kind` から種類を引く。
pub trait NodeRegistry: Send + Sync {
    /// 名前で引く。
    fn get(&self, kind: &str) -> Option<Arc<dyn NodeKind>>;
    /// 登録されている名前。
    fn kinds(&self) -> Vec<&'static str>;
}

/// 入力の無い `InputFrames`。入力を取らないノードの種類に渡す。
#[derive(Debug, Default)]
pub struct NoInputs;

impl InputFrames for NoInputs {
    fn frame(&mut self, index: usize, _t: RationalTime) -> Result<FrameRef> {
        Err(mm_types::MmError::NotFound(format!("入力 {index} は無い")))
    }

    fn len(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 隣り合う区間の音のフレーム数は重ならない() {
        let a = TimeRange::new(RationalTime::ZERO, RationalTime::new(1, 3).unwrap()).unwrap();
        let b = TimeRange::new(a.end, RationalTime::new(2, 3).unwrap()).unwrap();
        let c = TimeRange::new(b.end, RationalTime::seconds(1)).unwrap();
        let total: usize = [a, b, c].iter().map(|r| audio_frames_in(*r, 48_000)).sum();
        assert_eq!(total, 48_000);
    }
}
