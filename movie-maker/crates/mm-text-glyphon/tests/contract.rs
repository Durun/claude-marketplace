//! 文字の契約テスト。

use mm_testkit::text::contract;
use mm_text_glyphon::TextEngine;

#[test]
fn 文字の契約を満たす() {
    let engine = TextEngine::new();
    contract(|| engine.shaper(), || engine.rasterizer());
}
