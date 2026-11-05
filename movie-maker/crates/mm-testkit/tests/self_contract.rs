//! 見本の実装が自分の契約テストを通すことを確かめる。

use std::sync::Arc;

use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::cache::MemoryCache;
use mm_testkit::color::SampleColor;
use mm_testkit::decode::{DecodeFixture, SampleDecoderFactory, SampleEncoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, vertical_project};
use mm_testkit::host::{HostFixture, SampleLauncher};
use mm_testkit::node::{FootageKind, MapRegistry, NodeFixture, SolidKind, TextKind};
use mm_testkit::render::{RenderFixture, SampleRenderer};
use mm_testkit::text::{SampleRasterizer, SampleShaper};
use mm_types::Size;

#[test]
fn 見本のデコーダーは契約を通す() {
    let media = SampleMedia::default();
    mm_testkit::decode::contract(
        || {
            Arc::new(SampleDecoderFactory {
                media: media.clone(),
            })
        },
        &DecodeFixture::sample(&media),
    );
}

#[test]
fn 見本のエンコーダーは契約を通す() {
    mm_testkit::decode::encoder_contract(
        || Arc::new(SampleEncoderFactory),
        mm_testkit::SAMPLE_ID,
        &std::env::temp_dir(),
    );
}

#[test]
fn 見本のミキサーは契約を通す() {
    mm_testkit::audio::contract(|| Arc::new(SampleMixerFactory));
}

#[test]
fn 見本の文字は契約を通す() {
    mm_testkit::text::contract(|| Arc::new(SampleShaper), || Arc::new(SampleRasterizer));
}

#[test]
fn 見本の色は契約を通す() {
    mm_testkit::color::contract(|| Arc::new(SampleColor));
}

#[test]
fn 見本のホストは契約を通す() {
    mm_testkit::host::contract(|| Arc::new(SampleLauncher), &HostFixture::sample());
}

#[test]
fn 見本のキャッシュは契約を通す() {
    mm_testkit::cache::contract(|limit| Arc::new(MemoryCache::new(Some(limit))), 1024);
}

#[test]
fn 付属ファイルの契約を通す() {
    mm_testkit::ingest::contract(&[]);
}

#[test]
fn 見本の種類は契約を通す() {
    mm_testkit::node::contract(|| Arc::new(SolidKind), &NodeFixture::default());
    mm_testkit::node::contract(|| Arc::new(TextKind), &NodeFixture::default());
    mm_testkit::node::contract(
        || Arc::new(FootageKind),
        &NodeFixture {
            footage: Some(SampleMedia::default()),
            ..NodeFixture::default()
        },
    );
    mm_testkit::node::contract(|| Arc::new(FootageKind), &NodeFixture::default());
}

#[test]
fn 見本の描画器は契約を通す() {
    let media = SampleMedia::default();
    let out = std::env::temp_dir().join("mm_testkit_render_contract.json");
    let (project, comp, _) = vertical_project(&media, &out);
    let nodes = MapRegistry::with_samples();
    mm_testkit::render::contract(
        || {
            Box::new(SampleRenderer::new(
                Arc::new(SampleDecoderFactory {
                    media: media.clone(),
                }),
                Arc::new(SampleMixerFactory),
                sample_services(),
            ))
        },
        &RenderFixture {
            project: &project,
            composition: &comp,
            nodes: &nodes,
            size: Size::new(media.size.width, media.size.height),
        },
    );
}
