//! `packs/vlog` の文字のスタイルの期待画像。テンプレートを適用したプロジェクトに背景とマクロを適用し、描いた画像を
//! `fixtures/golden/<バックエンド>/pack-vlog.png` と比べる（`mm_testkit::golden`）。文字はモックの矩形で描くので、
//! 見るのはプレートの位置、大きさ、色、縁、影、傾きと、文字の塊がプレートの内側に収まっているか。
//! `web.cef` の部品は CEF の代わりのモックの矩形（ページと data で決まる色）で描き、レイヤーの置き方と不透明度を見る
//! （`pack-vlog-web.png`。部品の HTML か引数を変えると矩形の色が変わるので、更新する）。
//! 更新は `cargo xtask golden-update --test pack_vlog`。GPU と期待画像のフォルダーが無い環境ではスキップする。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mm_color_lut::LutColor;
use mm_doc::{Layer, Node, Project};
use mm_gpu::enumerate_adapters;
use mm_node_api::{Quality, Services};
use mm_nodes_basic::all_kinds;
use mm_nodes_cef::WebCefKind;
use mm_ops::{Editor, OpContext, Transaction};
use mm_pack::{expand, Pack};
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::fixtures::sample_services;
use mm_testkit::golden::{backend_name, Golden, BACKEND_ENV, GOLDEN_SIZE};
use mm_testkit::node::MapRegistry;
use mm_types::{Id, ParamValue, RationalTime, Rgba};

/// WARP のアダプターの名前（D3D12 のソフトウェアの GPU）。
const WARP_ADAPTER: &str = "Microsoft Basic Render Driver";
/// 背景の色（暗い青灰色。プレートの白と墨の縁が見分けられる）。
const BACKGROUND: Rgba = Rgba::new(0.06, 0.09, 0.13, 1.0);

fn vlog_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/vlog")
}

/// 描くときと同じモックの文字（`sample_services`）と、実際の色の変換。
fn services() -> Services {
    Services {
        color: Arc::new(LutColor),
        ..sample_services()
    }
}

fn gpu() -> Option<Arc<GpuContext>> {
    let request = if std::env::var(BACKEND_ENV).as_deref() == Ok("warp") {
        let adapter = enumerate_adapters()
            .into_iter()
            .find(|a| a.name == WARP_ADAPTER)
            .expect("WARP のアダプターが無い");
        GpuRequest {
            adapter: Some(adapter),
            backends: wgpu::Backends::DX12,
            ..GpuRequest::default()
        }
    } else {
        GpuRequest::default()
    };
    match GpuContext::shared(&request) {
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

/// 適用するマクロと引数（`名前=値` を `;` で区切る）。2.5 秒で全部が出ている。
const DEMO: [(&str, &str); 5] = [
    ("episode", "n=4"),
    (
        "section",
        "id=s;text=ボーカルミックス;chapter=ミックス;at=0;dur=10",
    ),
    ("label", "id=l;text=FXを入れた後;at=1;dur=4"),
    ("telop", "id=t;text=シュイーーン;at=2;dur=2"),
    (
        "caption",
        "id=c;text=リバーブの設定を聞いてみます;at=0;dur=6",
    ),
];

fn args(list: &str) -> BTreeMap<String, String> {
    list.split(';')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// web の部品の見本。ステッカーが 1 秒に出て、2 秒には出きっている。
const WEB_DEMO: [(&str, &str); 2] = [
    ("episode", "n=4"),
    ("sticker", "id=st;text=社会人DTMer;tone=yellow;at=1;dur=3"),
];

/// テンプレート、背景、セクションの見出し、ラベル、テロップ、字幕を適用したプロジェクト。
fn demo_project(demo: &[(&str, &str)]) -> Project {
    let pack = Pack::load(&vlog_dir()).expect("packs/vlog を読める");
    let shaper = services().text_shaper;
    let ctx = OpContext::human();
    let mut editor = Editor::new(Project::new());
    for (i, (name, pairs)) in demo.iter().enumerate() {
        let txn = expand(&pack, name, &args(pairs), &*shaper).expect("展開できる");
        editor.apply(txn, &ctx).expect("適用できる");
        if i == 0 {
            editor.apply(background(), &ctx).expect("背景を適用できる");
        }
    }
    editor.project().clone()
}

fn background() -> Transaction {
    let node = Node::new("solid", 1).with_param("color", ParamValue::Color(BACKGROUND));
    let layer = Layer::new(
        "bg",
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(10),
    );
    let op =
        serde_json::json!({"op": "addLayer", "composition": "main", "index": 0, "layer": layer});
    Transaction::new("背景", vec![serde_json::from_value(op).unwrap()])
}

/// `demo` を適用したプロジェクトの `time` の 1 フレームを描き、期待画像 `name` と比べる。
fn check_golden(name: &str, demo: &[(&str, &str)], time: RationalTime) {
    let Some(ctx) = gpu() else { return };
    let backend = backend_name(&format!("{:?}", ctx.backend()), ctx.adapter_name());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden");
    let actual = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-actual");
    let Some(golden) = Golden::open(&root, &backend, &actual) else {
        return;
    };
    let project = demo_project(demo);
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    nodes.insert(Arc::new(WebCefKind::mock()));
    let mut r = WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        services(),
    );
    let frame = r
        .render_frame(&RenderRequest {
            project: &project,
            composition: &Id::new("main"),
            time,
            size: GOLDEN_SIZE,
            quality: Quality::Full,
            nodes: &nodes,
        })
        .expect("描ける");
    let errors = r.take_errors();
    assert!(errors.is_empty(), "どのレイヤーも描けている: {errors:?}");
    let img = r.gpu().download(&frame).expect("リードバックできる");
    if let Err(message) = golden.check(name, &img) {
        panic!("{message}");
    }
}

#[test]
fn pack_vlog_の文字のスタイルの期待画像() {
    check_golden("pack-vlog", &DEMO, RationalTime::new(5, 2).unwrap());
}

#[test]
fn pack_vlog_の_web_の部品の期待画像() {
    check_golden("pack-vlog-web", &WEB_DEMO, RationalTime::seconds(2));
}
