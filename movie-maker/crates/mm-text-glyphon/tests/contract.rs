//! 文字の契約テスト。整形器とレンダラーを別々に作っても契約を満たす。

use std::sync::Arc;

use mm_testkit::text::contract;
use mm_text_glyphon::{CosmicShaper, CpuRasterizer, TextEngine};

#[test]
fn 文字の契約を満たす() {
    let engine = TextEngine::new();
    contract(|| engine.shaper(), || engine.rasterizer());
}

#[test]
fn 別々に作った整形器とレンダラーでも契約を満たす() {
    contract(
        || Arc::new(CosmicShaper::new()),
        || Arc::new(CpuRasterizer::new()),
    );
}
