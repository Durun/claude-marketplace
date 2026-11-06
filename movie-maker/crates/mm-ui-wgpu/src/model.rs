//! プロジェクトのデータから Timeline のモデルを作る。

use mm_doc::{Composition, Layer};
use mm_ui_layout::{Item, ItemKind, TimelineModel};

fn kind_of(layer: &Layer) -> ItemKind {
    match layer.source.kind.as_str() {
        "footage" => ItemKind::Footage,
        "image" => ItemKind::Image,
        "solid" => ItemKind::Solid,
        "text" => ItemKind::Text,
        "composition" => ItemKind::Composition,
        _ => ItemKind::Other,
    }
}

/// コンポジションのレイヤーを Timeline に並べる。上のレイヤーが先頭。時間軸が壊れたレイヤーは飛ばす。
pub fn timeline_model(comp: &Composition) -> TimelineModel {
    let items = comp
        .layers
        .iter()
        .rev()
        .filter_map(|layer| {
            let range = layer.range().ok()?;
            Some(Item {
                start: range.start.to_f64(),
                end: range.end.to_f64(),
                kind: kind_of(layer),
                enabled: layer.enabled,
            })
        })
        .collect();
    TimelineModel {
        duration: comp.duration.to_f64(),
        fps: comp.fps.to_f64(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::Node;
    use mm_types::{Rational, RationalTime};

    fn layer(id: &str, kind: &str, start: i64, in_s: i64, out_s: i64) -> Layer {
        Layer::new(
            id,
            Node::new(kind, 1),
            RationalTime::seconds(start),
            RationalTime::seconds(in_s),
            RationalTime::seconds(out_s),
        )
    }

    fn comp() -> Composition {
        Composition {
            id: mm_types::Id::new("c"),
            width: 64,
            height: 36,
            fps: Rational::new(30, 1).unwrap(),
            duration: RationalTime::seconds(10),
            layers: vec![
                layer("bottom", "footage", 0, 0, 4),
                layer("top", "text", 2, 0, 5),
            ],
            markers: Vec::new(),
        }
    }

    #[test]
    fn 上のレイヤーが先頭に来て_範囲は始まりから長さ() {
        let m = timeline_model(&comp());
        assert_eq!(m.items.len(), 2);
        assert_eq!(m.items[0].kind, ItemKind::Text);
        assert_eq!((m.items[0].start, m.items[0].end), (2.0, 7.0));
        assert_eq!(m.items[1].kind, ItemKind::Footage);
        assert_eq!((m.duration, m.fps), (10.0, 30.0));
    }

    #[test]
    fn 知らない種類は_other() {
        let mut c = comp();
        c.layers.push(layer("x", "web.cef", 0, 0, 1));
        assert_eq!(timeline_model(&c).items[0].kind, ItemKind::Other);
    }
}
