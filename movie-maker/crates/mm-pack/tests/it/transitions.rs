//! `packs/vlog/transitions` の WGSL のトランジション。2 つのテストパターンのカットの境に `transition`（巻き戻しは
//! `rewind`）のマクロで置き、全部がコンパイルできて描けることを確かめる。
//!
//! 目で見るときは `MM_TRANSITION_PNG=<フォルダー>`（`MM_TRANSITION=<名前>` で 1 つに絞る）で、進み具合 0.1..0.9 の
//! 960x540 の PNG を書き出す。`MM_TRANSITION_BENCH=1` で、1920x1080 の 1 コマを描く時間を種類ごとに出す。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use mm_doc::{Composition, Cut, Layer, Node, Project, Sequence};
use mm_node_api::Quality;
use mm_nodes_basic::all_kinds;
use mm_ops::{AddSequence, Editor, OpContext, Transaction};
use mm_pack::{expand, EstimateShaper, Pack};
use mm_render::{GpuContext, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::golden::to_srgb8;
use mm_testkit::node::MapRegistry;
use mm_types::{Id, ParamValue, Rational, RationalTime, RgbaImage, Size};

use crate::golden::{gpu, services, vlog_dir};

/// 出ていくカット: 暖色の 6 色の帯、灰色のランプ、時刻で右へ動く白い縦線（巻き戻しで逆に動くのが見える）。
const SCENE_A: &str = r"
fn mm_main(uv: vec2<f32>) -> vec4<f32> {
    var c = vec3<f32>(0.0);
    if (uv.y < 0.6) {
        let i = floor(uv.x * 6.0);
        c = vec3<f32>(0.75, 0.12 + 0.1 * i, 0.04 + 0.03 * i);
    } else {
        c = vec3<f32>(uv.x * 0.8);
    }
    if (abs(uv.x - fract(u.time * 0.25)) < 0.008) {
        c = vec3<f32>(1.0);
    }
    return vec4<f32>(c, 1.0);
}
";

/// 入ってくるカット: 寒色の斜めの縞と、中央の明るい丸。
const SCENE_B: &str = r"
fn mm_main(uv: vec2<f32>) -> vec4<f32> {
    let s = step(0.5, fract((uv.x - uv.y * 0.6) * 8.0 + u.time * 0.2));
    var c = mix(vec3<f32>(0.02, 0.05, 0.18), vec3<f32>(0.1, 0.45, 0.8), s);
    let d = length((uv - 0.5) * vec2<f32>(1.78, 1.0));
    c = mix(c, vec3<f32>(0.95, 0.9, 0.6), step(d, 0.15));
    return vec4<f32>(c, 1.0);
}
";

/// トランジションの長さ（秒）と、境の時刻。カット a は 0..3 秒、b は 3..6 秒、掛かるのは 2.5..3.5 秒。
const DURATION: i64 = 1;
const BOUNDARY: f64 = 3.0;
/// コンポジションの画面（vlog の本編と同じ）。
const CANVAS: Size = Size::new(1920, 1080);
/// 目で見る PNG の大きさ。
const PREVIEW: Size = Size::new(960, 540);
/// 描く時間を測るコマの数。
const BENCH_FRAMES: u32 = 20;

fn scene(code: &str) -> Node {
    Node::new("shader.wgsl", 1).with_param("code", ParamValue::Text(code.into()))
}

fn base_project() -> Project {
    let seconds = RationalTime::seconds;
    let mut p = Project::new();
    let seq = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("s".into()));
    p.compositions.push(
        Composition::new(
            Id::new("main"),
            CANVAS.width,
            CANVAS.height,
            Rational::integer(30),
            seconds(6),
        )
        .with_layers(vec![Layer::new(
            "seq",
            seq,
            seconds(0),
            seconds(0),
            seconds(6),
        )]),
    );
    let mut editor = Editor::new(p);
    let op = AddSequence {
        sequence: Sequence {
            id: Id::new("s"),
            cuts: vec![
                Cut::new("a", scene(SCENE_A), seconds(0), seconds(3)),
                Cut::new("b", scene(SCENE_B), seconds(0), seconds(3)),
            ],
            origin: None,
        },
    };
    editor
        .apply(
            Transaction::new("sequence", vec![op.into()]),
            &OpContext::human(),
        )
        .expect("sequence を足せる");
    editor.project().clone()
}

/// カット b の頭に `name` のトランジションを置いたプロジェクト。`rewind` は専用のマクロで置く。
pub(crate) fn project_with(pack: &Pack, name: &str) -> Project {
    let dur = DURATION.to_string();
    let mut args: BTreeMap<String, String> = [("cut", "b"), ("dur", dur.as_str())]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let macro_name = if name == "rewind" {
        "rewind"
    } else {
        args.insert("kind".into(), name.into());
        "transition"
    };
    let txn = expand(pack, macro_name, &args, &EstimateShaper).expect("展開できる");
    let mut editor = Editor::new(base_project());
    editor.apply(txn, &OpContext::human()).expect("適用できる");
    editor.project().clone()
}

/// 進み具合 `progress` の時刻。
fn time_at(progress: f64) -> RationalTime {
    let t = BOUNDARY - DURATION as f64 / 2.0 + progress * DURATION as f64;
    RationalTime::new((t * 1000.0).round() as i128, 1000).expect("時刻")
}

pub(crate) struct Stage {
    pub renderer: WgpuRenderer,
    nodes: MapRegistry,
}

impl Stage {
    pub(crate) fn new(ctx: Arc<GpuContext>) -> Self {
        let mut nodes = MapRegistry::default();
        for kind in all_kinds() {
            nodes.insert(kind);
        }
        let renderer = WgpuRenderer::new(
            ctx,
            Arc::new(SampleDecoderFactory::default()),
            Arc::new(SampleMixerFactory),
            services(),
        );
        Self { renderer, nodes }
    }

    /// 進み具合 `progress` の 1 コマを描く。エラーがあれば種類の名前を添えて落とす。
    pub(crate) fn render(&mut self, p: &Project, progress: f64, size: Size) -> Arc<RgbaImage> {
        let frame = self
            .renderer
            .render_frame(&RenderRequest {
                project: p,
                composition: &Id::new("main"),
                time: time_at(progress),
                size,
                quality: Quality::Full,
                nodes: &self.nodes,
            })
            .expect("描ける");
        let errors = self.renderer.take_errors();
        assert!(errors.is_empty(), "描けている: {errors:?}");
        self.renderer
            .gpu()
            .download(&frame)
            .expect("リードバックできる")
    }
}

fn write_png(path: &Path, image: &RgbaImage) {
    let file = std::fs::File::create(path).expect("PNG を作れる");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), image.width(), image.height());
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&to_srgb8(image)))
        .expect("PNG を書ける");
}

fn selected(pack: &Pack) -> Vec<String> {
    let only = std::env::var("MM_TRANSITION").ok();
    pack.transitions
        .names()
        .filter(|n| only.as_deref().is_none_or(|o| o == *n))
        .map(str::to_string)
        .collect()
}

#[test]
fn vlog_のトランジションは全部描ける() {
    let Some(ctx) = gpu() else { return };
    let pack = Pack::load(&vlog_dir()).expect("packs/vlog を読める");
    let out = std::env::var_os("MM_TRANSITION_PNG").map(std::path::PathBuf::from);
    let mut stage = Stage::new(ctx);
    for name in selected(&pack) {
        let p = project_with(&pack, &name);
        for progress in [0.2, 0.5, 0.8] {
            stage.render(&p, progress, Size::new(192, 108));
        }
        let Some(dir) = &out else { continue };
        std::fs::create_dir_all(dir).expect("フォルダーを作れる");
        for k in 1..=9 {
            let image = stage.render(&p, f64::from(k) / 10.0, PREVIEW);
            write_png(&dir.join(format!("{name}-{k}.png")), &image);
        }
    }
    if std::env::var_os("MM_TRANSITION").is_none() {
        assert_eq!(pack.transitions.code.len(), 20, "19 種と巻き戻し");
    }
}

#[test]
fn vlog_のトランジションの_1080p_の描く時間() {
    if std::env::var_os("MM_TRANSITION_BENCH").is_none() {
        return;
    }
    let Some(ctx) = gpu() else { return };
    let pack = Pack::load(&vlog_dir()).expect("packs/vlog を読める");
    let mut stage = Stage::new(ctx);
    let mut measure = |p: &Project| {
        stage.render(p, 0.5, CANVAS);
        let start = Instant::now();
        for k in 0..BENCH_FRAMES {
            stage.render(
                p,
                0.3 + 0.4 * f64::from(k) / f64::from(BENCH_FRAMES),
                CANVAS,
            );
        }
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(BENCH_FRAMES)
    };
    let cut = measure(&base_project());
    eprintln!("トランジション無し（境の前後の 2 つのカット）: {cut:.2} ms");
    for name in selected(&pack) {
        let ms = measure(&project_with(&pack, &name));
        eprintln!("{name}: {ms:.2} ms（無しとの差 {:.2} ms）", ms - cut);
    }
}
