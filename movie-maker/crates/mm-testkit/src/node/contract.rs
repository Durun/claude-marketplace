//! 動画オブジェクトの契約テストと、1 コマを描く足場。

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecoderFactory, FrameSource};
use mm_node_api::{
    AudioCx, FrameCx, FrameRef, GpuHelper, InputFrames, NoInputs, NodeKind, Quality, Services,
};
use mm_types::{Params, RationalTime, Rect, Result, Rgba, RgbaImage, Size, TimeRange};

use crate::decode::{DecoderFrameSource, SampleDecoderFactory, SampleMedia};
use crate::gpu::CpuGpu;

/// 入力 0 に決まった絵を返す `InputFrames`。`is_identity` の検査に使う。
pub struct ConstInputs(pub Arc<RgbaImage>);

impl InputFrames for ConstInputs {
    fn frame(&mut self, _index: usize, _t: RationalTime) -> Result<FrameRef> {
        Ok(FrameRef::Cpu(self.0.clone()))
    }

    fn len(&self) -> usize {
        1
    }
}

/// 契約テストに渡す種類の情報。
#[derive(Debug, Clone)]
pub struct NodeFixture {
    /// 既定値に重ねるパラメータ。
    pub params: Params,
    /// `footage` が要る種類に渡す見本の素材。
    pub footage: Option<SampleMedia>,
    pub size: Size,
    /// 端の検査に使う長さ。
    pub duration: RationalTime,
}

impl Default for NodeFixture {
    fn default() -> Self {
        Self {
            params: Params::new(),
            footage: None,
            size: Size::new(64, 36),
            duration: RationalTime::seconds(2),
        }
    }
}

/// 1 コマを描く足場。契約テストと見本の描画器が使う。
pub struct Harness {
    pub gpu: CpuGpu,
    pub services: Services,
    pub footage: Option<Box<dyn FrameSource>>,
}

impl Harness {
    /// 見本の口で組む。
    pub fn new(footage: Option<Box<dyn FrameSource>>) -> Self {
        Self {
            gpu: CpuGpu::new(),
            services: crate::fixtures::sample_services(),
            footage,
        }
    }

    /// `kind` を `t` で描いて CPU の絵にする。`inputs` は入力の映像。
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
        .open(std::path::Path::new("sample.mp4"))
        .expect("見本は開ける");
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
    let mut no_inputs = NoInputs;

    let (base, rod) = h
        .render(
            kind.as_ref(),
            &params,
            RationalTime::ZERO,
            fx.size,
            1.0,
            &mut no_inputs,
        )
        .expect("既定値で描ける");
    assert_eq!(base.size(), fx.size, "絵の大きさは cx.size");
    check_edges(&mut h, kind.as_ref(), &params, fx);
    check_outside_rod(&base, rod);

    check_traits(&mut h, kind.as_ref(), &params, fx, &base, rod);
    check_identity(&mut h, kind.as_ref(), &params, fx);

    let json = serde_json::to_value(&params).expect("JSON");
    let migrated = kind.migrate(json, kind.version()).expect("今の版は通る");
    assert_eq!(migrated, params, "今の版の migrate は恒等");
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
    let mut no_inputs = NoInputs;
    let mut render = |t: RationalTime, size: Size, scale: f32| {
        h.render(kind, params, t, size, scale, &mut no_inputs)
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
    }
}

fn check_edges(h: &mut Harness, kind: &dyn NodeKind, params: &Params, fx: &NodeFixture) {
    let mut no_inputs = NoInputs;
    for t in [
        RationalTime::ZERO,
        fx.duration,
        RationalTime::seconds(-1),
        fx.duration + RationalTime::seconds(1),
    ] {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            h.render(kind, params, t, fx.size, 1.0, &mut no_inputs)
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

fn check_identity(h: &mut Harness, kind: &dyn NodeKind, params: &Params, fx: &NodeFixture) {
    let input = Arc::new(RgbaImage::filled(fx.size, Rgba::new(0.2, 0.4, 0.6, 1.0)));
    let mut inputs = ConstInputs(input.clone());
    let is_identity = {
        let mut probe = NoInputs;
        let cx = FrameCx {
            time: RationalTime::ZERO,
            params,
            size: fx.size,
            region: fx.size.to_rect(),
            scale: 1.0,
            quality: Quality::Full,
            inputs: &mut probe,
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
        let (out, _) = h
            .render(kind, params, RationalTime::ZERO, fx.size, 1.0, &mut inputs)
            .expect("描ける");
        assert_eq!(*out, *input, "is_identity なら入力と同じ");
    }
}

/// 区間の音を出す足場。
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
