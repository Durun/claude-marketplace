//! `mm-host-ofx` を子プロセスで起こし、試験のプラグイン（`build.rs` が公式のヘッダーで作る）で
//! describe、パラメータ、1 フレームの render、`mm-host-api` の契約、クラッシュの隔離を確かめる。
//! - `Invert`: OpenFX の公式のサンプル（`Examples/Invert`、BSD-3-Clause）。8 ビットだけに対応する。
//! - `MmTestGain`: このクレートの最小のプラグイン（`tests/plugins/gain.c`）。float と 4 つの型のパラメータ。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mm_host_api::{HostLauncher, HostProcess, HostRequest, HostResponse, HostSpec, SharedFrame};
use mm_host_ofx::desc::{ParamKind, PluginDesc};
use mm_host_ofx::{describe_bundle, OfxLauncher, PrepareParams, RenderParams};
use mm_testkit::host::HostFixture;
use mm_types::{MmError, ParamValue, Params, RationalTime, Rgba, RgbaImage, Size};

const EXE: &str = env!("CARGO_BIN_EXE_mm-host-ofx");
const PLUGINS: &str = env!("MM_OFX_TEST_PLUGINS");
const TIMEOUT: Duration = Duration::from_secs(5);
/// 契約の時間切れに使う描画の遅さ。`TIMEOUT` より長くする。
const SLOW_MS: f64 = 20_000.0;
/// 8 ビットの量子化の誤差。
const BYTE_TOL: f32 = 1.0 / 255.0 + 1e-6;

fn bundle(name: &str) -> PathBuf {
    Path::new(PLUGINS).join(format!("{name}.ofx.bundle"))
}

fn spec() -> HostSpec {
    HostSpec {
        executable: EXE.into(),
        args: Vec::new(),
        default_timeout: TIMEOUT,
        gpu: None,
        client_pid: None,
    }
}

fn prepare_params(name: &str) -> serde_json::Value {
    let p = PrepareParams {
        bundle: bundle(name),
        plugin: None,
        frame_rate: 30.0,
    };
    serde_json::to_value(p).unwrap()
}

fn render_params(source: Option<&RgbaImage>, values: &[(&str, ParamValue)]) -> serde_json::Value {
    let mut params = Params::new();
    for (name, v) in values {
        params.set(*name, v.clone());
    }
    let p = RenderParams {
        source: source.map(SharedFrame::from_image),
        values: params,
    };
    serde_json::to_value(p).unwrap()
}

fn start(name: &str) -> Box<dyn HostProcess> {
    let mut host = OfxLauncher.spawn(&spec()).expect("起動");
    let req = HostRequest::Prepare {
        params: prepare_params(name),
    };
    assert_eq!(
        host.request(&req, TIMEOUT).expect("Prepare"),
        HostResponse::Prepared
    );
    host
}

fn render(
    host: &mut dyn HostProcess,
    size: Size,
    params: serde_json::Value,
) -> mm_types::Result<HostResponse> {
    let req = HostRequest::RenderFrame {
        time: RationalTime::seconds(1),
        size,
        params,
    };
    host.request(&req, TIMEOUT)
}

fn image_of(response: HostResponse) -> RgbaImage {
    match response {
        HostResponse::Frame(SharedFrame::Cpu { size, data }) => {
            RgbaImage::from_raw(size, data).unwrap()
        }
        other => panic!("CPU の画像のはずが {other:?}"),
    }
}

/// 左上が赤、右下が半透明の青、ほかは灰色の 4x3。
fn source() -> RgbaImage {
    let mut img = RgbaImage::filled(Size::new(4, 3), Rgba::new(0.25, 0.25, 0.25, 1.0));
    img.set_pixel(0, 0, Rgba::new(1.0, 0.0, 0.0, 1.0));
    img.set_pixel(3, 2, Rgba::new(0.0, 0.0, 0.5, 0.5));
    img
}

fn describe_one(name: &str) -> PluginDesc {
    let desc = describe_bundle(Path::new(EXE), &bundle(name), TIMEOUT).expect("describe");
    assert!(desc.skipped.is_empty(), "{:?}", desc.skipped);
    assert_eq!(desc.plugins.len(), 1);
    desc.plugins.into_iter().next().unwrap()
}

#[test]
fn 公式の_invert_を列挙できる() {
    let p = describe_one("Invert");
    assert_eq!(p.id, "uk.co.thefoundry.OfxInvertExample");
    assert_eq!(p.label, "OFX Invert Example");
    assert_eq!(p.grouping, "OFX Example");
    assert_eq!(p.contexts, ["OfxImageEffectContextFilter"]);
    assert_eq!(p.pixel_depths, ["OfxBitDepthByte"]);
    assert_eq!(p.context.as_deref(), Some("OfxImageEffectContextFilter"));
    assert!(p.params.is_empty());
}

#[test]
fn パラメータの一覧を型と既定値と範囲つきで取れる() {
    let p = describe_one("MmTestGain");
    assert_eq!((p.id.as_str(), p.version), ("mm.test.Gain", [1, 2]));
    let names: Vec<_> = p.params.iter().map(|x| (x.name.as_str(), x.kind)).collect();
    assert_eq!(
        names,
        [
            ("gain", ParamKind::Double),
            ("channel", ParamKind::Choice),
            ("delay_ms", ParamKind::Integer),
            ("crash", ParamKind::Boolean),
        ]
    );
    let gain = &p.params[0];
    assert_eq!(
        (gain.label.as_str(), gain.hint.as_str()),
        ("Gain", "RGB multiplier")
    );
    assert_eq!(
        (gain.default.clone(), gain.min, gain.max),
        (serde_json::json!(1.0), Some(0.0), Some(4.0))
    );
    assert_eq!(p.params[1].choices, ["all", "red"]);
    assert_eq!(p.params[3].default, serde_json::json!(false));
    assert!(p.params[3].secret);
    let schema = p.param_schema();
    let names: Vec<_> = schema.params.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["gain", "channel", "delay_ms"],
        "隠しのパラメータは載せない"
    );
    assert_eq!(schema.params[1].default, ParamValue::Choice("all".into()));
}

#[test]
fn バンドルでないパスの_describe_は誤り() {
    let err = describe_bundle(
        Path::new(EXE),
        Path::new(PLUGINS).join("none.ofx.bundle").as_path(),
        TIMEOUT,
    );
    assert!(matches!(err, Err(MmError::HostCrashed(_))), "{err:?}");
}

#[test]
fn 公式の_invert_で_8_ビットの_1_フレームを描ける() {
    let mut host = start("Invert");
    let src = source();
    let out = image_of(
        render(host.as_mut(), src.size(), render_params(Some(&src), &[])).expect("描ける"),
    );
    let want =
        RgbaImage::from_raw(src.size(), src.data().iter().map(|v| 1.0 - v).collect()).unwrap();
    assert!(
        out.max_diff(&want).unwrap() <= BYTE_TOL,
        "{:?}",
        out.max_diff(&want)
    );
}

#[test]
fn パラメータの値を渡して_float_の_1_フレームを描ける() {
    let mut host = start("MmTestGain");
    let src = source();
    let size = src.size();
    let same = image_of(render(host.as_mut(), size, render_params(Some(&src), &[])).unwrap());
    assert_eq!(
        same, src,
        "既定の gain 1 はそのまま（標準出力への printf は応答に混ざらない）"
    );

    let values = [
        ("gain", ParamValue::Number(0.5)),
        ("channel", ParamValue::Choice("red".into())),
    ];
    let out = image_of(render(host.as_mut(), size, render_params(Some(&src), &values)).unwrap());
    assert_eq!(out.pixel(0, 0), Rgba::new(0.5, 0.0, 0.0, 1.0));
    assert_eq!(out.pixel(1, 1), Rgba::new(0.125, 0.25, 0.25, 1.0));
    assert_eq!(out.pixel(3, 2), src.pixel(3, 2), "赤の無い画素は変わらない");

    let bad = render(
        host.as_mut(),
        size,
        render_params(None, &[("nope", ParamValue::Number(1.0))]),
    );
    assert!(matches!(bad, Ok(HostResponse::Error { .. })), "{bad:?}");
    let empty = image_of(render(host.as_mut(), size, render_params(None, &[])).unwrap());
    assert_eq!(empty, RgbaImage::transparent(size), "入力が無ければ透明");
}

#[test]
fn ホストの契約を満たす() {
    let fx = HostFixture {
        spec: spec(),
        prepare_params: prepare_params("MmTestGain"),
        slow_params: render_params(None, &[("delay_ms", ParamValue::Number(SLOW_MS))]),
    };
    mm_testkit::host::contract(|| Arc::new(OfxLauncher) as Arc<dyn HostLauncher>, &fx);
}

#[test]
fn プラグインのクラッシュはホストだけが落ち_再起動で戻る() {
    let mut host = start("MmTestGain");
    let size = Size::new(2, 2);
    let crashed = render(
        host.as_mut(),
        size,
        render_params(None, &[("crash", ParamValue::Bool(true))]),
    );
    assert!(
        matches!(crashed, Err(MmError::HostCrashed(_))),
        "{crashed:?}"
    );
    assert!(!host.is_alive());
    host.restart().expect("restart");
    let req = HostRequest::Prepare {
        params: prepare_params("MmTestGain"),
    };
    assert_eq!(host.request(&req, TIMEOUT).unwrap(), HostResponse::Prepared);
    let out = image_of(render(host.as_mut(), size, render_params(None, &[])).unwrap());
    assert_eq!(out.size(), size);
}
