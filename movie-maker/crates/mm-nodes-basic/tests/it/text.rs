//! `text` の種類のパラメータ（縁取り、折り返し）が `TextStyle` に渡ること。

use mm_node_api::{FrameCx, NoInputs, NodeKind, Quality};
use mm_nodes_basic::TextKind;
use mm_testkit::fixtures::sample_services;
use mm_testkit::gpu::CpuGpu;
use mm_text_api::{LineBreak, TextStroke, TextStyle};
use mm_types::{ParamValue, Params, RationalTime, Rgba, Size};

fn defaults() -> Params {
    TextKind::default().params().defaults()
}

#[test]
fn 既定のパラメータは_text_style_の既定と同じ() {
    assert_eq!(TextKind::style(&defaults()), TextStyle::default());
}

#[test]
fn 縁取りの色と幅が渡る() {
    let mut params = defaults();
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    params.set("stroke_color", ParamValue::Color(red));
    params.set("stroke_width", ParamValue::Number(3.0));
    assert_eq!(
        TextKind::style(&params).stroke,
        Some(TextStroke {
            color: red,
            width_px: 3.0
        })
    );
}

#[test]
fn 縁の幅が_0_なら縁なし() {
    let mut params = defaults();
    params.set("stroke_color", ParamValue::Color(Rgba::WHITE));
    assert_eq!(TextKind::style(&params).stroke, None);
}

#[test]
fn 折り返しの選択肢が_line_break_に渡る() {
    let mut params = defaults();
    params.set("max_width", ParamValue::Number(200.0));
    let style = TextKind::style(&params);
    assert_eq!(style.line_break, LineBreak::Anywhere);
    assert_eq!(style.max_width, Some(200.0));

    params.set("line_break", ParamValue::Choice("phrase".into()));
    let style = TextKind::style(&params);
    assert_eq!(style.line_break, LineBreak::Phrase);
    assert_eq!(style.max_width, Some(200.0));

    params.set("line_break", ParamValue::Choice("none".into()));
    assert_eq!(TextKind::style(&params).max_width, None, "折り返さない");
}

#[test]
fn 縁を付けても字の位置は動かない() {
    let size = Size::new(128, 72);
    let region_x = |params: &Params| {
        let (mut inputs, mut gpu) = (NoInputs, CpuGpu::new());
        let services = sample_services();
        let cx = FrameCx {
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
            services: &services,
        };
        TextKind::default().region_of_definition(&cx).x
    };
    let mut params = defaults();
    params.set("position", ParamValue::Vec2([10.0, 10.0]));
    let plain = region_x(&params);
    params.set("stroke_width", ParamValue::Number(4.0));
    assert_eq!(region_x(&params), plain - 4.0, "縁の分だけ左へ広がる");
}
