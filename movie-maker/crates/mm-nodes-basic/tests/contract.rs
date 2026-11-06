//! ノードの契約テスト。種類ごとに 1 行。

use std::sync::Arc;

use mm_nodes_basic::{CompositionKind, FootageKind, ImageKind, SolidKind, TextKind};
use mm_testkit::decode::SampleMedia;
use mm_testkit::node::{contract, NodeFixture};
use mm_types::{ParamValue, Params, Rgba};

fn footage_fixture() -> NodeFixture {
    NodeFixture {
        footage: Some(SampleMedia::default()),
        ..NodeFixture::default()
    }
}

#[test]
fn solid_は契約を満たす() {
    let mut params = Params::new();
    params.set("color", ParamValue::Color(Rgba::new(0.2, 0.4, 0.6, 1.0)));
    contract(
        || Arc::new(SolidKind),
        &NodeFixture {
            params,
            ..NodeFixture::default()
        },
    );
}

#[test]
fn footage_は契約を満たす() {
    contract(|| Arc::new(FootageKind), &footage_fixture());
}

#[test]
fn image_は契約を満たす() {
    contract(|| Arc::new(ImageKind), &footage_fixture());
}

#[test]
fn text_は契約を満たす() {
    contract(|| Arc::new(TextKind), &NodeFixture::default());
}

#[test]
fn composition_は契約を満たす() {
    contract(|| Arc::new(CompositionKind), &NodeFixture::default());
}
