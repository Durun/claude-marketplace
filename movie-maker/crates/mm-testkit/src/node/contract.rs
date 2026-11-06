//! ノードの契約テストと、1 フレームを描くハーネス。

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecoderFactory, FrameSource};
use mm_node_api::{
    AudioCx, FrameCx, FrameRef, GpuHelper, InputFrames, NoInputs, NodeKind, Quality, Services,
};
use mm_types::{AssetId, MmError, Params, RationalTime, Rect, Result, RgbaImage, Size, TimeRange};

use crate::decode::{DecoderFrameSource, SampleDecoderFactory, SampleMedia};
use crate::gpu::CpuGpu;

/// 入力ごとに決まった画像を返す `InputFrames`。時刻によらず同じ画像。
pub struct ConstInputs(pub Vec<Arc<RgbaImage>>);

impl InputFrames for ConstInputs {
    fn frame(&mut self, index: usize, _t: RationalTime) -> Result<FrameRef> {
        self.0
            .get(index)
            .map(|img| FrameRef::Cpu(img.clone()))
            .ok_or_else(|| MmError::NotFound(format!("入力 {index}")))
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    /// プロジェクトのデータが無いので、素材の ID をそのままファイルのパスとして読む。
    fn asset_path(&self, id: &AssetId) -> Option<std::path::PathBuf> {
        Some(id.as_str().into())
    }
}

/// 契約テストに渡す種類の情報。
#[derive(Debug, Clone)]
pub struct NodeFixture {
    /// 既定値に重ねるパラメータ。
    pub params: Params,
    /// `footage` が要る種類に渡すフィクスチャの素材。
    pub footage: Option<SampleMedia>,
    pub size: Size,
    /// 端の検査に使う長さ。
    pub duration: RationalTime,
    /// 入力の映像。入力を見る効果の種類が渡す。空なら入力なしで描く。
    /// 全部の検査（描画、端、`is_identity` の問い合わせ）で同じ入力を使う。
    pub inputs: Vec<Arc<RgbaImage>>,
}

impl Default for NodeFixture {
    fn default() -> Self {
        Self {
            params: Params::new(),
            footage: None,
            size: Size::new(64, 36),
            duration: RationalTime::seconds(2),
            inputs: Vec::new(),
        }
    }
}

impl NodeFixture {
    /// 検査に渡す入力。`inputs` が空なら入力なし。
    fn input_frames(&self) -> Box<dyn InputFrames> {
        if self.inputs.is_empty() {
            Box::new(NoInputs)
        } else {
            Box::new(ConstInputs(self.inputs.clone()))
        }
    }
}

/// 1 フレームを描くハーネス。契約テストとモックのレンダラーが使う。
pub struct Harness {
    pub gpu: CpuGpu,
    pub services: Services,
    pub footage: Option<Box<dyn FrameSource>>,
}

impl Harness {
    /// モックのインターフェースで組む。
    pub fn new(footage: Option<Box<dyn FrameSource>>) -> Self {
        Self {
            gpu: CpuGpu::new(),
            services: crate::fixtures::sample_services(),
            footage,
        }
    }

    /// `kind` を `t` で描いて CPU の画像にする。`inputs` は入力の映像。
    pub fn render(
        &mut self,
        kind: &dyn NodeKind,
        params: &Params,
        t: RationalTime,
        size: Size,
        scale: f32,
        inputs: &mut dyn InputFrames,
    ) -> Result<(Arc<RgbaImage>, Rect)> {
        let mut cx = FrameCx {
            time: t,
            params,
            size,
            region: size.to_rect(),
            scale,
            quality: Quality::Full,
            inputs,
            footage: mm_node_api::footage_ref(self.footage.as_mut()),
            source_time: t,
            gpu: &mut self.gpu,
            services: &self.services,
        };
        let rod = kind.region_of_definition(&cx);
        let frame = kind.render(&mut cx)?;
        Ok((self.gpu.download(&frame)?, rod))
    }
}

fn open_footage(media: &SampleMedia) -> Box<dyn FrameSource> {
    let factory = SampleDecoderFactory {
        media: media.clone(),
    };
    let dec = factory
        .open(std::path::Path::new("sample.mp4"), None)
        .expect("モックは開ける");
    Box::new(DecoderFrameSource::new(dec))
}

/// 種類の契約。
pub fn contract(make: impl Fn() -> Arc<dyn NodeKind>, fx: &NodeFixture) {
    let kind = make();
    let params = fx.params.with_defaults(&kind.params());
    kind.params()
        .validate(&params)
        .expect("既定値はスキーマに合う");
    let mut h = Harness::new(fx.footage.as_ref().map(open_footage));
    let mut inputs = fx.input_frames();

    let (base, rod) = h
        .render(
            kind.as_ref(),
            &params,
            RationalTime::ZERO,
            fx.size,
            1.0,
            inputs.as_mut(),
        )
        .expect("既定値で描ける");
    assert_eq!(base.size(), fx.size, "画像の大きさは cx.size");
    check_edges(&mut h, kind.as_ref(), &params, fx);
    check_outside_rod(&base, rod);

    check_traits(&mut h, kind.as_ref(), &params, fx, &base, rod);
    check_identity(&mut h, kind.as_ref(), &params, fx);

    let json = serde_json::to_value(&params).expect("JSON");
    let migrated = kind
        .migrate(json, kind.version())
        .expect("今のバージョンは通る");
    assert_eq!(migrated, params, "今のバージョンの migrate は恒等");
}

/// `deterministic`、`time_varying`、`vector` の自己申告を確かめる。
fn check_traits(
    h: &mut Harness,
    kind: &dyn NodeKind,
    params: &Params,
    fx: &NodeFixture,
    base: &RgbaImage,
    rod: Rect,
) {
    let traits = kind.traits();
    let mut inputs = fx.input_frames();
    let mut render = |t: RationalTime, size: Size, scale: f32| {
        h.render(kind, params, t, size, scale, inputs.as_mut())
            .expect("描ける")
    };
    if traits.deterministic {
        let (again, _) = render(RationalTime::ZERO, fx.size, 1.0);
        assert_eq!(*base, *again, "deterministic なら 2 回描いて同じ");
    }
    if !traits.time_varying {
        let (later, _) = render(RationalTime::seconds(1), fx.size, 1.0);
        assert_eq!(*base, *later, "time_varying が偽なら t を変えても同じ");
    }
    if traits.vector {
        let doubled = Size::new(fx.size.width * 2, fx.size.height * 2);
        let (big, rod2) = render(RationalTime::ZERO, doubled, 2.0);
        assert_eq!(big.size(), doubled, "2 倍の大きさ");
        assert!(
            (rod2.width - rod.width * 2.0).abs() <= 2.0,
            "領域も 2 倍（{} vs {}）",
            rod2.width,
            rod.width
        );
        check_vector_content(base, &big);
    }
}

/// 描かれた範囲（不透明な画素の外接矩形）の縁が、2 倍の画像で 2 倍の位置に来ること。
/// 大きさだけ合わせて同じ画像を引き伸ばした実装は、縁の位置でなく太さや位置がずれる。
fn check_vector_content(base: &RgbaImage, big: &RgbaImage) {
    let (Some(small), Some(large)) = (painted_bounds(base), painted_bounds(big)) else {
        return;
    };
    for (s, l) in small.iter().zip(&large) {
        assert!(
            (i64::from(*l) - 2 * i64::from(*s)).abs() <= VECTOR_EDGE_TOLERANCE_PX,
            "ベクターの縁は 2 倍の位置（{small:?} → {large:?}）"
        );
    }
}

/// 縁の位置の許容差（画素）。アンチエイリアスの 1 画素ぶんの揺れ。
const VECTOR_EDGE_TOLERANCE_PX: i64 = 3;

/// 不透明度が 0 でない画素の外接矩形 `[左, 上, 右, 下]`（右と下は含まない）。無ければ `None`。
fn painted_bounds(image: &RgbaImage) -> Option<[u32; 4]> {
    let mut bounds: Option<[u32; 4]> = None;
    for y in 0..image.height() {
        for x in 0..image.width() {
            if image.pixel(x, y).a > 0.0 {
                let b = bounds.get_or_insert([x, y, x + 1, y + 1]);
                *b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
            }
        }
    }
    bounds
}

fn check_edges(h: &mut Harness, kind: &dyn NodeKind, params: &Params, fx: &NodeFixture) {
    let mut inputs = fx.input_frames();
    let beyond_end = fx
        .duration
        .checked_add(RationalTime::seconds(1))
        .expect("表せる");
    for t in [
        RationalTime::ZERO,
        fx.duration,
        RationalTime::seconds(-1),
        beyond_end,
    ] {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            h.render(kind, params, t, fx.size, 1.0, inputs.as_mut())
                .map(|_| ())
        }));
        assert!(result.is_ok(), "t = {t} で panic しない");
    }
}

fn check_outside_rod(image: &RgbaImage, rod: Rect) {
    for y in 0..image.height() {
        for x in 0..image.width() {
            let inside = (x as f32) >= rod.x.floor()
                && (x as f32) < rod.right().ceil()
                && (y as f32) >= rod.y.floor()
                && (y as f32) < rod.bottom().ceil();
            if !inside {
                assert_eq!(image.pixel(x, y).a, 0.0, "({x}, {y}) は領域の外なので透明");
            }
        }
    }
}

/// `is_identity` が真なら、同じ入力で描いた結果は入力 0 と同じ。問い合わせも描画も `fx.inputs`
/// （空なら入力なし）で行うので、入力を見て決める種類も検査される。
fn check_identity(h: &mut Harness, kind: &dyn NodeKind, params: &Params, fx: &NodeFixture) {
    let mut inputs = fx.input_frames();
    let is_identity = {
        let mut probe = fx.input_frames();
        let cx = FrameCx {
            time: RationalTime::ZERO,
            params,
            size: fx.size,
            region: fx.size.to_rect(),
            scale: 1.0,
            quality: Quality::Full,
            inputs: probe.as_mut(),
            footage: None,
            source_time: RationalTime::ZERO,
            gpu: &mut h.gpu,
            services: &h.services,
        };
        let needed = kind.frames_needed(&cx);
        assert!(!needed.is_empty(), "frames_needed は 1 つ以上の範囲を返す");
        kind.is_identity(&cx)
    };
    if is_identity {
        let input = fx.inputs.first().expect(
            "入力を見ない種類が is_identity を真にするのは契約外。入力のある fixture で検査する",
        );
        let (out, _) = h
            .render(
                kind,
                params,
                RationalTime::ZERO,
                fx.size,
                1.0,
                inputs.as_mut(),
            )
            .expect("描ける");
        assert_eq!(*out, **input, "is_identity なら入力と同じ");
    }
}

/// 区間の音を出すハーネス。
pub fn render_audio<'a>(
    kind: &dyn NodeKind,
    params: &'a Params,
    range: TimeRange,
    footage: Option<&'a mut dyn FrameSource>,
) -> Result<AudioBlock> {
    let mut cx = AudioCx {
        range,
        source_range: range,
        params,
        sample_rate: mm_audio_api::DEFAULT_SAMPLE_RATE,
        channels: mm_audio_api::DEFAULT_CHANNELS,
        footage,
    };
    kind.render_audio(&mut cx)
}
