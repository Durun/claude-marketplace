//! `mm-ops` の操作を模した見本。別クレートの関数が差し替わるかを見るため、先端のクレートの外に置く。

use mm_doc::{Composition, Layer, Node, Param, Project};
use mm_types::{Id, ParamValue, Rational, RationalTime};

/// 差し替えを目で確かめるための印。保存して書き換えると、反映後の出力が変わる。
pub const LIB_MARK: &str = "lib-v1";

/// レイヤーを 1 枚持つ見本の文書。
pub fn demo_project() -> Project {
    let mut p = Project::new();
    p.compositions.push(Composition {
        id: Id::new("c1"),
        width: 1920,
        height: 1080,
        fps: Rational::integer(60),
        duration: RationalTime::seconds(10),
        layers: vec![Layer {
            id: Id::new("l1"),
            source: Node::new("solid", 1),
            start: RationalTime::ZERO,
            in_point: RationalTime::ZERO,
            out_point: RationalTime::seconds(10),
            opacity: Param::constant(ParamValue::Number(1.0)),
            audio: None,
            lock: false,
        }],
    });
    p
}

/// 不透明度を変える操作（文書を受けて文書を変える純粋な関数）。印つきの報告を返す。
pub fn set_opacity(project: &mut Project, value: f64) -> String {
    let layer = &mut project.compositions[0].layers[0];
    layer.opacity = Param::constant(ParamValue::Number(value));
    format!("{LIB_MARK} opacity={value}")
}
