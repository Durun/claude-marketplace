use std::sync::{Arc, Mutex};

use mm_node_api::{FrameCx, FrameRef, NodeKind, Traits};
use mm_text_api::{LineBreak, ShapedText, TextAlign, TextStroke, TextStyle};
use mm_types::{
    MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Rect, Result, Rgba, RgbaImage,
    Size,
};

use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// 覚えておくシェーピングの結果の数。字幕の入れ替わりと、アニメーションの数値が数通り行き来する程度。
const KEPT_SHAPES: usize = 8;
/// 覚えておく描いた画布の数。倍率や領域が変わるたび（ズームの途中）に増えるので小さく保つ。
const KEPT_CANVASES: usize = 8;

/// `line_break` の選択肢。`none` は `max_width` を無視して折り返さず、残りは `LineBreak` の値に対応する。
const LINE_BREAK_NONE: &str = "none";
const LINE_BREAK_ANYWHERE: &str = "anywhere";
const LINE_BREAK_PHRASE: &str = "phrase";

/// シェーピングの結果の持ち主を決める入力。
#[derive(Debug, Clone, PartialEq)]
struct ShapeKey {
    text: String,
    style: TextStyle,
}

/// 画布の内容を決める入力。位置は領域の原点からの差だけが効くので、文字を動かしても同じ画布を使い回せる。
#[derive(Debug, Clone, PartialEq)]
struct CanvasKey {
    shape: ShapeKey,
    scale_bits: u32,
    canvas: [u32; 2],
    offset: [i64; 2],
}

/// シェーピングの結果と描いた画布のキャッシュ。新しい順に並べ、上限を超えたら古いものから捨てる。
#[derive(Debug, Default)]
struct TextCache {
    shapes: Vec<(ShapeKey, Arc<ShapedText>)>,
    canvases: Vec<(CanvasKey, Arc<RgbaImage>)>,
}

fn remember<K: PartialEq, V: Clone>(entries: &mut Vec<(K, V)>, key: &K, limit: usize) -> Option<V> {
    let i = entries.iter().position(|(k, _)| k == key)?;
    let hit = entries.remove(i);
    let value = hit.1.clone();
    entries.insert(0, hit);
    entries.truncate(limit);
    Some(value)
}

/// 文字。`position`（1 倍の画素）に `size` の字で描く。シェーピングと描画は `Services` のインターフェースに任せ、
/// 倍率 `cx.scale` で描くので拡大してもぼやけない（`vector`）。
///
/// `time_varying: false` なので同じパラメータが毎フレーム来る。シェーピングの結果（`region_of_definition` と `render` で共有）と
/// 描いた画布を覚え、2 回目からはシェーピングも描画も画布の確保もしない。
#[derive(Debug, Default)]
pub struct TextKind {
    cache: Mutex<TextCache>,
}

impl TextKind {
    /// パラメータから `TextStyle` を作る（`tests/it/text.rs` が直接確かめる）。
    pub fn style(params: &Params) -> TextStyle {
        let line_break = params
            .get("line_break")
            .and_then(ParamValue::as_text)
            .unwrap_or(LINE_BREAK_ANYWHERE);
        let max_width = params.number_or("max_width", 0.0);
        let stroke_width = params.number_or("stroke_width", 0.0);
        TextStyle {
            size_px: params.number_or("size", 32.0) as f32,
            color: params
                .get("color")
                .and_then(ParamValue::as_color)
                .unwrap_or(Rgba::WHITE),
            // 折り返しなしは幅を無視する（幅を残したまま切り替えられる）。
            max_width: (max_width > 0.0 && line_break != LINE_BREAK_NONE)
                .then_some(max_width as f32),
            stroke: (stroke_width > 0.0).then(|| TextStroke {
                color: params
                    .get("stroke_color")
                    .and_then(ParamValue::as_color)
                    .unwrap_or(Rgba::BLACK),
                width_px: stroke_width as f32,
            }),
            line_break: if line_break == LINE_BREAK_PHRASE {
                LineBreak::Phrase
            } else {
                LineBreak::Anywhere
            },
            align: TextAlign::parse(
                params
                    .get("align")
                    .and_then(ParamValue::as_text)
                    .unwrap_or("left"),
            ),
            ..TextStyle::default()
        }
    }

    fn text(params: &Params) -> &str {
        params
            .get("text")
            .and_then(ParamValue::as_text)
            .unwrap_or("")
    }

    /// 倍率をかけた左上の画素。縁は字の外側に出るので、縁の分だけ左上へ寄せ、縁の有無で字の位置が動かないようにする。
    fn origin(params: &Params, scale: f32) -> (f32, f32) {
        let [x, y] = params
            .get("position")
            .and_then(ParamValue::as_vec2)
            .unwrap_or([0.0, 0.0]);
        let stroke = Self::style(params).stroke.map_or(0.0, |s| s.width_px);
        (
            ((x as f32 - stroke) * scale).floor(),
            ((y as f32 - stroke) * scale).floor(),
        )
    }

    fn cache(&self) -> Result<std::sync::MutexGuard<'_, TextCache>> {
        self.cache
            .lock()
            .map_err(|_| MmError::Internal("文字のキャッシュが壊れた".into()))
    }

    fn shape_key(params: &Params) -> ShapeKey {
        ShapeKey {
            text: Self::text(params).to_string(),
            style: Self::style(params),
        }
    }

    /// シェーピングの結果。同じ文字とスタイルならキャッシュから返す。
    fn shaped(&self, cx: &FrameCx<'_>, key: &ShapeKey) -> Result<Arc<ShapedText>> {
        if let Some(hit) = remember(&mut self.cache()?.shapes, key, KEPT_SHAPES) {
            return Ok(hit);
        }
        let shaped = Arc::new(cx.services.text_shaper.shape(&key.text, &key.style)?);
        let mut cache = self.cache()?;
        cache.shapes.insert(0, (key.clone(), shaped.clone()));
        cache.shapes.truncate(KEPT_SHAPES);
        Ok(shaped)
    }
}

impl NodeKind for TextKind {
    fn id(&self) -> &'static str {
        "text"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        let number = |min| ParamType::Number {
            min: Some(min),
            max: None,
        };
        ParamSchema::new(vec![
            ParamSpec::new("text", ParamType::Text, ParamValue::Text("Text".into())),
            ParamSpec::new("size", number(1.0), ParamValue::Number(32.0)),
            ParamSpec::new("color", ParamType::Color, ParamValue::Color(Rgba::WHITE)),
            ParamSpec::new("position", ParamType::Vec2, ParamValue::Vec2([0.0, 0.0])),
            ParamSpec::new("max_width", number(0.0), ParamValue::Number(0.0)),
            ParamSpec::new("align", ParamType::Text, ParamValue::Text("left".into())),
            ParamSpec::new(
                "stroke_color",
                ParamType::Color,
                ParamValue::Color(Rgba::BLACK),
            ),
            ParamSpec::new("stroke_width", number(0.0), ParamValue::Number(0.0)),
            ParamSpec::new(
                "line_break",
                ParamType::Choice {
                    options: vec![
                        LINE_BREAK_NONE.into(),
                        LINE_BREAK_ANYWHERE.into(),
                        LINE_BREAK_PHRASE.into(),
                    ],
                },
                ParamValue::Choice(LINE_BREAK_ANYWHERE.into()),
            ),
        ])
    }

    fn traits(&self) -> Traits {
        Traits {
            time_varying: false,
            vector: true,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn region_of_definition(&self, cx: &FrameCx<'_>) -> Rect {
        let Ok(shaped) = self.shaped(cx, &Self::shape_key(cx.params)) else {
            return Rect::EMPTY;
        };
        let (x, y) = Self::origin(cx.params, cx.scale);
        Rect::new(
            x,
            y,
            (shaped.bounds.width * cx.scale).ceil(),
            (shaped.bounds.height * cx.scale).ceil(),
        )
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let shape = Self::shape_key(cx.params);
        let (x, y) = Self::origin(cx.params, cx.scale);
        // 画布は頼まれた領域の分だけにし、領域の原点を引いた位置に置く（領域が画布の全体なら元のまま）。
        let region = cx.region;
        let canvas_size = Size::new(
            (region.width.ceil() as u32).max(1),
            (region.height.ceil() as u32).max(1),
        );
        let key = CanvasKey {
            shape,
            scale_bits: cx.scale.to_bits(),
            canvas: [canvas_size.width, canvas_size.height],
            offset: [
                x as i64 - region.x.floor() as i64,
                y as i64 - region.y.floor() as i64,
            ],
        };
        if let Some(hit) = remember(&mut self.cache()?.canvases, &key, KEPT_CANVASES) {
            return Ok(FrameRef::Cpu(hit));
        }
        let shaped = self.shaped(cx, &key.shape)?;
        let glyphs = cx
            .services
            .text_rasterizer
            .rasterize(&shaped, &key.shape.style, cx.scale)?;
        let mut canvas = RgbaImage::transparent(canvas_size);
        canvas.blit_over(&glyphs, key.offset[0], key.offset[1], 1.0);
        let canvas = Arc::new(canvas);
        let mut cache = self.cache()?;
        cache.canvases.insert(0, (key, canvas.clone()));
        cache.canvases.truncate(KEPT_CANVASES);
        Ok(FrameRef::Cpu(canvas))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use mm_node_api::{NoInputs, Quality, Services};
    use mm_testkit::fixtures::sample_services;
    use mm_testkit::gpu::CpuGpu;
    use mm_text_api::{TextRasterizer, TextShaper};
    use mm_types::RationalTime;

    use super::*;

    /// 呼ばれた回数を数えて、中身はラップしたモックに任せる。
    struct Counting {
        inner: Services,
        shapes: Arc<AtomicUsize>,
        rasters: Arc<AtomicUsize>,
    }

    struct CountShaper(Arc<dyn TextShaper>, Arc<AtomicUsize>);
    struct CountRaster(Arc<dyn TextRasterizer>, Arc<AtomicUsize>);

    impl TextShaper for CountShaper {
        fn id(&self) -> &'static str {
            "count"
        }

        fn shape(&self, text: &str, style: &TextStyle) -> Result<ShapedText> {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.shape(text, style)
        }
    }

    impl TextRasterizer for CountRaster {
        fn id(&self) -> &'static str {
            "count"
        }

        fn rasterize(
            &self,
            shaped: &ShapedText,
            style: &TextStyle,
            scale: f32,
        ) -> Result<RgbaImage> {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.rasterize(shaped, style, scale)
        }
    }

    fn counting() -> Counting {
        let inner = sample_services();
        let (shapes, rasters) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        Counting {
            inner,
            shapes,
            rasters,
        }
    }

    impl Counting {
        fn services(&self) -> Services {
            Services {
                text_shaper: Arc::new(CountShaper(
                    self.inner.text_shaper.clone(),
                    self.shapes.clone(),
                )),
                text_rasterizer: Arc::new(CountRaster(
                    self.inner.text_rasterizer.clone(),
                    self.rasters.clone(),
                )),
                color: self.inner.color.clone(),
            }
        }
    }

    fn draw_frame(kind: &TextKind, services: &Services, params: &Params) -> FrameRef {
        let size = Size::new(128, 72);
        let mut inputs = NoInputs;
        let mut gpu = CpuGpu::new();
        let mut cx = FrameCx {
            time: RationalTime::ZERO,
            params,
            size,
            region: size.to_rect(),
            scale: 1.0,
            quality: Quality::Full,
            inputs: &mut inputs,
            footage: None,
            source_time: RationalTime::ZERO,
            gpu: &mut gpu,
            services,
        };
        // レンダラーと同じく、領域は種類が答えた矩形にする。
        cx.region = kind.region_of_definition(&cx);
        kind.render(&mut cx).expect("描ける")
    }

    #[test]
    fn 同じパラメータの_2_フレーム目はシェーピングも描画もしない() {
        let counting = counting();
        let services = counting.services();
        let kind = TextKind::default();
        let params = kind.params().defaults();
        let first = draw_frame(&kind, &services, &params);
        let second = draw_frame(&kind, &services, &params);
        assert_eq!(
            counting.shapes.load(Ordering::SeqCst),
            1,
            "シェーピングは 1 回"
        );
        assert_eq!(counting.rasters.load(Ordering::SeqCst), 1, "描画は 1 回");
        match (first, second) {
            (FrameRef::Cpu(a), FrameRef::Cpu(b)) => {
                assert!(Arc::ptr_eq(&a, &b), "同じ画布を返す")
            }
            other => panic!("CPU の画像のはず: {other:?}"),
        }
    }

    #[test]
    fn 文字が変わったらシェーピングし直す() {
        let counting = counting();
        let services = counting.services();
        let kind = TextKind::default();
        let mut params = kind.params().defaults();
        draw_frame(&kind, &services, &params);
        params.set("text", ParamValue::Text("Other".into()));
        draw_frame(&kind, &services, &params);
        assert_eq!(counting.shapes.load(Ordering::SeqCst), 2);
        assert_eq!(counting.rasters.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn 位置だけが変わっても同じ画布を使い回す() {
        let counting = counting();
        let services = counting.services();
        let kind = TextKind::default();
        let mut params = kind.params().defaults();
        params.set("position", ParamValue::Vec2([4.0, 4.0]));
        draw_frame(&kind, &services, &params);
        params.set("position", ParamValue::Vec2([10.0, 6.0]));
        draw_frame(&kind, &services, &params);
        assert_eq!(
            counting.rasters.load(Ordering::SeqCst),
            1,
            "領域の原点からの差が同じ"
        );
    }
}
