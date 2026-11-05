//! OCIO の無い環境では feature `native` を付けず、このファイルは空になる（README を見る）。
#![cfg(feature = "native")]

use std::sync::Arc;

use mm_color_ocio::OcioColor;

#[test]
fn 色の契約を満たす() {
    mm_testkit::color::contract(|| Arc::new(OcioColor::builtin().expect("組み込みの設定")));
}
