//! 変形、不透明度、入れ子、GPU の時間、期待画像。GPU が無い環境ではスキップし、
//! `MM_REQUIRE_GPU` があれば失敗にする。

use std::path::Path;
use std::sync::Arc;

use mm_doc::{Asset, Composition, Layer, Node, Param, Project, Transform};
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RangeRequest, RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, vertical_project, SAMPLE_ASSET_PATH};
use mm_testkit::node::MapRegistry;
use mm_testkit::render::SampleRenderer;
use mm_types::{
    ColorSpace, Id, ParamValue, Rational, RationalTime, Rgba, RgbaImage, Size, TimeRange,
};

use mm_node_api::Quality;

const SMALL: Size = Size::new(64, 36);
/// f16 の丸め（約 5e-4）と線形補間の誤差を見込んだ許容。
const TOLERANCE: f32 = 3e-3;
const RED: Rgba = Rgba::new(1.0, 0.0, 0.0, 1.0);

fn gpu() -> Option<Arc<GpuContext>> {
    match GpuContext::shared(&GpuRequest::default()) {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
    }
}

fn registry() -> MapRegistry {
    let mut r = MapRegistry::default();
    for kind in all_kinds() {
        r.insert(kind);
    }
    r
}

fn renderer(ctx: &Arc<GpuContext>) -> WgpuRenderer {
    WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        sample_services(),
    )
}

/// 種類のパラメータ `transform.*` を `Layer.transform` に移してレイヤーを作る（テストの書きやすさのため）。
fn layer(id: &str, mut source: Node) -> Layer {
    let mut transform = Transform::default();
    for (name, param) in std::mem::take(&mut source.params) {
        match name.as_str() {
            "transform.position" => transform.position = param,
            "transform.scale" => transform.scale = param,
            "transform.rotation" => transform.rotation = param,
            "transform.anchor" => transform.anchor = Some(param),
            _ => {
                source.params.insert(name, param);
            }
        }
    }
    Layer {
        transform,
        ..Layer::new(
            id,
            source,
            RationalTime::ZERO,
            RationalTime::ZERO,
            RationalTime::seconds(2),
        )
    }
}

fn solid(color: Rgba) -> Node {
    Node::new("solid", 1).with_param("color", ParamValue::Color(color))
}

fn number(node: Node, name: &str, v: f64) -> Node {
    node.with_param(name, ParamValue::Number(v))
}

fn comp(id: &str, size: Size, layers: Vec<Layer>) -> Composition {
    Composition::new(
        Id::new(id),
        size.width,
        size.height,
        Rational::integer(30),
        RationalTime::seconds(2),
    )
    .with_layers(layers)
}

/// フィクスチャの素材 `clip` を持つプロジェクト。
fn project(comps: Vec<Composition>) -> Project {
    let media = SampleMedia::default();
    let mut p = Project::new();
    p.assets.push(Asset {
        id: Id::new("clip"),
        path: SAMPLE_ASSET_PATH.into(),
        content_hash: "sample0000000000".into(),
        color_space: ColorSpace::bt709(),
        duration: media.duration,
        fps: Some(media.fps),
        timebase_offset: RationalTime::ZERO,
    });
    p.compositions = comps;
    p
}

fn request<'a>(
    project: &'a Project,
    id: &'a mm_types::CompositionId,
    nodes: &'a MapRegistry,
    size: Size,
) -> RenderRequest<'a> {
    RenderRequest {
        project,
        composition: id,
        time: RationalTime::ZERO,
        size,
        quality: Quality::Full,
        nodes,
    }
}

/// 1 フレーム描いて CPU の画像にする。
fn render_image(r: &mut WgpuRenderer, p: &Project, name: &str, size: Size) -> Arc<RgbaImage> {
    let id = Id::new(name);
    let nodes = registry();
    let frame = r
        .render_frame(&request(p, &id, &nodes, size))
        .expect("描ける");
    r.gpu().download(&frame).expect("リードバックできる")
}

fn assert_pixel(img: &RgbaImage, x: u32, y: u32, want: Rgba) {
    let got = img.pixel(x, y);
    let diff = [
        got.r - want.r,
        got.g - want.g,
        got.b - want.b,
        got.a - want.a,
    ];
    assert!(
        diff.iter().all(|d| d.abs() <= TOLERANCE),
        "({x}, {y}) が {want:?} のはずが {got:?}"
    );
}

#[test]
fn 変形が無ければ_cpu_のモックと同じ画像() {
    let Some(ctx) = gpu() else { return };
    let media = SampleMedia::default();
    let (project, id, _) = vertical_project(&media, Path::new("out.mp4"));
    let nodes = registry();
    let req = request(&project, &id, &nodes, media.size);
    let mut cpu = SampleRenderer::new(
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        sample_services(),
    );
    let want = {
        let f = cpu.render_frame(&req).expect("モックが描ける");
        cpu.gpu().download(&f).expect("リードバックできる")
    };
    let mut r = renderer(&ctx);
    let f = r.render_frame(&req).expect("描ける");
    let got = r.gpu().download(&f).expect("リードバックできる");
    let diff = got.max_diff(&want).expect("同じ大きさ");
    assert!(diff <= TOLERANCE, "CPU のモックとの差 {diff}");
    assert!(r.take_errors().is_empty());
}

#[test]
fn 半分に縮めると中央だけ塗れる() {
    let Some(ctx) = gpu() else { return };
    let node = number(solid(RED), "transform.scale", 0.5);
    let p = project(vec![comp("main", SMALL, vec![layer("a", node)])]);
    let img = render_image(&mut renderer(&ctx), &p, "main", SMALL);
    assert_pixel(&img, 32, 18, RED);
    assert_pixel(&img, 18, 10, RED);
    assert_pixel(&img, 5, 5, Rgba::TRANSPARENT);
    assert_pixel(&img, 60, 30, Rgba::TRANSPARENT);
}

#[test]
fn fit_が_none_の素材は素材の画素のまま左上に置く() {
    let Some(ctx) = gpu() else { return };
    let media = SampleMedia::default();
    let canvas = Size::new(media.size.width * 2, media.size.height * 2);
    let footage = |fit: &str| {
        Node::footage(&Id::new("clip")).with_param("fit", ParamValue::Choice(fit.into()))
    };
    let p = project(vec![
        comp("none", canvas, vec![layer("a", footage("none"))]),
        comp("contain", canvas, vec![layer("b", footage("contain"))]),
    ]);
    let mut r = renderer(&ctx);
    let none = render_image(&mut r, &p, "none", canvas);
    assert_eq!(none.pixel(10, 10).a, 1.0, "左上の素材の大きさの中は塗る");
    assert_eq!(
        none.pixel(media.size.width + 10, 10).a,
        0.0,
        "素材の幅の外は透明"
    );
    assert_eq!(
        none.pixel(10, media.size.height + 10).a,
        0.0,
        "素材の高さの外は透明"
    );
    let contain = render_image(&mut r, &p, "contain", canvas);
    assert_eq!(
        contain.pixel(canvas.width - 10, canvas.height - 10).a,
        1.0,
        "収めると画布の全体"
    );
    assert!(r.take_errors().is_empty());
}

#[test]
fn 位置は右に動かす() {
    let Some(ctx) = gpu() else { return };
    let node = solid(RED).with_param("transform.position", ParamValue::Vec2([32.0, 0.0]));
    let p = project(vec![comp("main", SMALL, vec![layer("a", node)])]);
    let img = render_image(&mut renderer(&ctx), &p, "main", SMALL);
    assert_pixel(&img, 10, 18, Rgba::TRANSPARENT);
    assert_pixel(&img, 50, 18, RED);
}

#[test]
fn 九十度回すと縦長になる() {
    let Some(ctx) = gpu() else { return };
    let node = number(solid(RED), "transform.rotation", 90.0)
        .with_param("transform.scale", ParamValue::Number(0.5));
    let p = project(vec![comp("main", SMALL, vec![layer("a", node)])]);
    let img = render_image(&mut renderer(&ctx), &p, "main", SMALL);
    assert_pixel(&img, 32, 4, RED);
    assert_pixel(&img, 32, 32, RED);
    assert_pixel(&img, 20, 18, Rgba::TRANSPARENT);
}

#[test]
fn 不透明度は下の画像と混ざる() {
    let Some(ctx) = gpu() else { return };
    let mut top = layer("top", solid(Rgba::WHITE));
    top.opacity = Param::constant(ParamValue::Number(0.5));
    let black = layer("bg", solid(Rgba::new(0.0, 0.0, 0.0, 1.0)));
    let p = project(vec![comp("main", SMALL, vec![black, top])]);
    let img = render_image(&mut renderer(&ctx), &p, "main", SMALL);
    assert_pixel(&img, 10, 10, Rgba::new(0.5, 0.5, 0.5, 1.0));
}

#[test]
fn 入れ子は中を描いたものと同じ画像() {
    let Some(ctx) = gpu() else { return };
    let inner = comp(
        "inner",
        SMALL,
        vec![layer(
            "g",
            number(solid(Rgba::new(0.0, 1.0, 0.0, 1.0)), "transform.scale", 0.5),
        )],
    );
    let nested =
        Node::new("composition", 1).with_param("composition", ParamValue::Text("inner".into()));
    let outer = comp("outer", SMALL, vec![layer("n", nested)]);
    let p = project(vec![inner, outer]);
    let mut r = renderer(&ctx);
    let want = render_image(&mut r, &p, "inner", SMALL);
    let got = render_image(&mut r, &p, "outer", SMALL);
    assert!(got.max_diff(&want).expect("同じ大きさ") <= TOLERANCE);
    assert_pixel(&got, 32, 18, Rgba::new(0.0, 1.0, 0.0, 1.0));
    assert!(r.take_errors().is_empty());
}

#[test]
fn 入れ子の循環は透明にして続ける() {
    let Some(ctx) = gpu() else { return };
    let me = Node::new("composition", 1).with_param("composition", ParamValue::Text("loop".into()));
    let p = project(vec![comp("loop", SMALL, vec![layer("self", me)])]);
    let mut r = renderer(&ctx);
    let img = render_image(&mut r, &p, "loop", SMALL);
    assert_pixel(&img, 10, 10, Rgba::TRANSPARENT);
    assert!(!r.take_errors().is_empty(), "循環はフラグを残す");
}

#[test]
fn 種類が無いレイヤーだけ透明になる() {
    let Some(ctx) = gpu() else { return };
    let ghost = layer("ghost", Node::new("no.such.kind", 1));
    let p = project(vec![comp(
        "main",
        SMALL,
        vec![layer("a", solid(RED)), ghost],
    )]);
    let mut r = renderer(&ctx);
    let img = render_image(&mut r, &p, "main", SMALL);
    assert_pixel(&img, 10, 10, RED);
    assert_eq!(r.take_errors().len(), 1);
}

#[test]
fn 区間を描いてもテクスチャが溜まらない() {
    let Some(ctx) = gpu() else { return };
    let p = project(vec![comp("main", SMALL, vec![layer("a", solid(RED))])]);
    let nodes = registry();
    let id = Id::new("main");
    let mut r = renderer(&ctx);
    let before = r.texture_count();
    let range = TimeRange::new(
        RationalTime::ZERO,
        RationalTime::from_frames(30, Rational::integer(30)).expect("表せる"),
    )
    .expect("範囲");
    let mut seen = 0;
    let n = r
        .render_range(
            &RangeRequest {
                project: &p,
                composition: &id,
                range,
                fps: Rational::integer(30),
                size: SMALL,
                quality: Quality::Full,
                nodes: &nodes,
            },
            &mut |_, frame, gpu| {
                gpu.download(&frame)?;
                seen += 1;
                Ok(())
            },
        )
        .expect("区間が描ける");
    assert_eq!((n, seen), (30, 30));
    let kept = r.frame_cache_stats().entries;
    assert_eq!(
        r.texture_count(),
        before + kept,
        "区間のフレームはドロップし、キャッシュが覚えた画像だけ残る"
    );
    for _ in 0..20 {
        render_image(&mut r, &p, "main", SMALL);
    }
    let kept = r.frame_cache_stats().entries;
    assert!(
        r.texture_count() <= 8 + kept,
        "単発のフレームは直近の分とキャッシュの分だけ持つ"
    );
}

/// 期待画像: 背景、素材、回した単色、入れ子、拡大した文字を重ねる。
fn showcase() -> (Project, Size) {
    let size = Size::new(640, 360);
    let footage = Node::new("footage", 1)
        .with_param("asset", ParamValue::AssetRef(Id::new("clip")))
        .with_param("transform.scale", ParamValue::Number(0.55))
        .with_param("transform.rotation", ParamValue::Number(-8.0))
        .with_param("transform.position", ParamValue::Vec2([-150.0, -50.0]));
    let mut orange = layer(
        "orange",
        solid(Rgba::new(1.0, 0.45, 0.05, 1.0))
            .with_param("transform.scale", ParamValue::Number(0.3))
            .with_param("transform.rotation", ParamValue::Number(30.0))
            .with_param("transform.position", ParamValue::Vec2([170.0, -20.0])),
    );
    orange.opacity = Param::constant(ParamValue::Number(0.8));
    let badge = comp(
        "badge",
        Size::new(200, 100),
        vec![
            layer("teal", solid(Rgba::new(0.0, 0.55, 0.5, 1.0))),
            layer(
                "label",
                Node::new("text", 1)
                    .with_param("text", ParamValue::Text("Nested".into()))
                    .with_param("size", ParamValue::Number(30.0))
                    .with_param("position", ParamValue::Vec2([12.0, 30.0])),
            ),
        ],
    );
    let nested = Node::new("composition", 1)
        .with_param("composition", ParamValue::Text("badge".into()))
        .with_param("transform.rotation", ParamValue::Number(10.0))
        .with_param("transform.position", ParamValue::Vec2([250.0, 190.0]));
    let hello = Node::new("text", 1)
        .with_param("text", ParamValue::Text("Hello".into()))
        .with_param("size", ParamValue::Number(40.0))
        .with_param("position", ParamValue::Vec2([40.0, 190.0]))
        .with_param("transform.scale", ParamValue::Number(1.5))
        .with_param("transform.anchor", ParamValue::Vec2([0.0, 0.0]));
    let main = comp(
        "main",
        size,
        vec![
            layer("bg", solid(Rgba::new(0.01, 0.02, 0.06, 1.0))),
            layer("footage", footage),
            orange,
            layer("nested", nested),
            layer("hello", hello),
        ],
    );
    (project(vec![badge, main]), size)
}

/// 線形の premultiplied を、灰色の市松の上に重ねて sRGB の 8 bit にする。
fn to_srgb8(img: &RgbaImage) -> Vec<u8> {
    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    let mut out = Vec::with_capacity(img.size().area() * 3);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let p = img.pixel(x, y);
            let checker = if (x / 16 + y / 16) % 2 == 0 {
                0.05
            } else {
                0.1
            };
            out.extend([p.r, p.g, p.b].map(|c| encode(c + checker * (1.0 - p.a))));
        }
    }
    out
}

#[test]
fn 期待画像を_png_に書く() {
    let Some(ctx) = gpu() else { return };
    let (p, size) = showcase();
    let mut r = renderer(&ctx);
    let img = render_image(&mut r, &p, "main", size);
    assert!(r.take_errors().is_empty(), "どのレイヤーも描けている");
    assert_pixel(&img, 5, 5, Rgba::new(0.01, 0.02, 0.06, 1.0));
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mm-render-showcase.png");
    let file = std::fs::File::create(&path).expect("PNG を作れる");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.width, size.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("ヘッダー")
        .write_image_data(&to_srgb8(&img))
        .expect("画素");
    eprintln!("期待画像: {}", path.display());
}
