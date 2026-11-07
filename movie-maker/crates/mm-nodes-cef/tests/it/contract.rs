//! CEF の要らない検査。モックの矩形と、モックのホスト（`SampleLauncher`、CPU の画像で返す）の経路で、
//! 契約、キャッシュ、静止のページの扱いを確かめる。

use std::sync::Arc;

use mm_node_api::{NoInputs, NodeKind};
use mm_nodes_cef::{Stats, WebCefKind, DATA_PARAM, HTML_PARAM, STATIC_PARAM};
use mm_testkit::host::SampleLauncher;
use mm_testkit::node::{contract, Harness, NodeFixture};
use mm_types::{ParamValue, Params, RationalTime, RgbaImage, Size};

const SIZE: Size = Size::new(64, 36);
const PAGE: &str = "<!doctype html><html><head></head><body>x</body></html>";

fn params(data: serde_json::Value, is_static: bool) -> Params {
    let mut p = Params::new();
    p.set(HTML_PARAM, ParamValue::Text(PAGE.into()))
        .set(DATA_PARAM, ParamValue::Json(data))
        .set(STATIC_PARAM, ParamValue::Bool(is_static));
    p
}

fn render(kind: &WebCefKind, p: &Params, t: RationalTime) -> Arc<RgbaImage> {
    let p = p.with_defaults(&kind.params());
    let mut h = Harness::new(None);
    let (image, _) = h
        .render(kind, &p, t, SIZE, 1.0, &mut NoInputs)
        .expect("描ける");
    image
}

fn sample_host() -> WebCefKind {
    WebCefKind::with_host(Arc::new(SampleLauncher), "sample".into())
}

#[test]
fn モックは契約を満たす() {
    contract(|| Arc::new(WebCefKind::mock()), &NodeFixture::default());
    let fx = NodeFixture {
        params: params(serde_json::json!({"text": "x"}), false),
        ..NodeFixture::default()
    };
    contract(|| Arc::new(WebCefKind::mock()), &fx);
}

#[test]
fn モックのホストでも契約を満たす() {
    let fx = NodeFixture {
        params: params(serde_json::json!({}), false),
        ..NodeFixture::default()
    };
    contract(|| Arc::new(sample_host()), &fx);
}

#[test]
fn モックは中央の矩形でパラメータごとに色が変わる() {
    let kind = WebCefKind::mock();
    let a = render(
        &kind,
        &params(serde_json::json!({"n": 1}), false),
        RationalTime::ZERO,
    );
    let b = render(
        &kind,
        &params(serde_json::json!({"n": 2}), false),
        RationalTime::ZERO,
    );
    assert_eq!(a.pixel(0, 0).a, 0.0, "外は透明");
    assert!(
        a.pixel(SIZE.width / 2, SIZE.height / 2).a > 0.5,
        "中央は塗る"
    );
    assert_ne!(a.pixel(32, 18), b.pixel(32, 18), "data が違えば色が違う");
}

#[test]
fn 同じパラメータと時刻はキャッシュから返す() {
    let kind = sample_host();
    let p = params(serde_json::json!({"n": 1}), false);
    let first = render(&kind, &p, RationalTime::seconds(1));
    let again = render(&kind, &p, RationalTime::seconds(1));
    assert_eq!(first, again);
    assert_eq!(kind.stats(), Stats { drawn: 1, hits: 1 });
    render(&kind, &p, RationalTime::seconds(2));
    render(
        &kind,
        &params(serde_json::json!({"n": 2}), false),
        RationalTime::seconds(1),
    );
    assert_eq!(
        kind.stats(),
        Stats { drawn: 3, hits: 1 },
        "時刻か data が違えば描き直す"
    );
}

#[test]
fn 静止のページは時刻によらず_1_回だけ描く() {
    let kind = sample_host();
    let p = params(serde_json::json!({"n": 1}), true);
    for s in 0..5 {
        render(&kind, &p, RationalTime::seconds(s));
    }
    assert_eq!(kind.stats(), Stats { drawn: 1, hits: 4 });
}

#[test]
fn html_が空なら何も描かずホストも起こさない() {
    let kind = sample_host();
    let image = render(&kind, &Params::new(), RationalTime::ZERO);
    assert!(image.data().iter().all(|v| *v == 0.0));
    assert_eq!(kind.stats(), Stats::default());
}

#[test]
fn 前置きはページの_mm_set_time_を包む() {
    let html = mm_nodes_cef::inject(PAGE);
    assert!(
        html.contains("defineProperty(window, 'mmSetTime'"),
        "{html}"
    );
    assert!(
        html.starts_with("<!doctype html><html><head><meta http-equiv=\"Content-Security-Policy\"")
    );
}
