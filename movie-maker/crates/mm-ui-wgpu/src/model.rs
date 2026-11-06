//! プロジェクトのデータから Timeline のモデルを作る。

use mm_doc::{Composition, Layer, Project, Sequence, SequenceId, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_eval::{place_cuts, PlacedCut};
use mm_types::Id;
use mm_ui_layout::{CutBar, Item, ItemKind, TimelineModel};

fn kind_of(layer: &Layer) -> ItemKind {
    match layer.source.kind.as_str() {
        "footage" => ItemKind::Footage,
        "image" => ItemKind::Image,
        "solid" => ItemKind::Solid,
        "text" => ItemKind::Text,
        "composition" => ItemKind::Composition,
        SEQUENCE_KIND => ItemKind::Sequence,
        _ => ItemKind::Other,
    }
}

/// Timeline に並ぶレイヤーを、上が先頭の順に返す。時間軸が壊れたレイヤーはスキップする。
/// 並びの規則はモデルとレイヤーの ID の対応の両方が使う 1 つだけにする。
fn visible_layers(comp: &Composition) -> impl Iterator<Item = (&Layer, mm_types::TimeRange)> {
    comp.layers
        .iter()
        .rev()
        .filter_map(|layer| Some((layer, layer.range().ok()?)))
}

/// Timeline の `Item` と同じ並びのレイヤーの ID。Timeline の選択（添字）をレイヤーに引き直すのに使う。
pub fn timeline_layer_ids(comp: &Composition) -> Vec<String> {
    visible_layers(comp)
        .map(|(layer, _)| layer.id.to_string())
        .collect()
}

/// シーケンスのレイヤーが指すシーケンスと、有効なカットの配置。シーケンスのレイヤーでなければ `None`。
fn layer_cuts<'a>(project: &'a Project, layer: &Layer) -> Option<(&'a Sequence, Vec<PlacedCut>)> {
    if layer.source.kind != SEQUENCE_KIND {
        return None;
    }
    let id: SequenceId = Id::new(layer.source.text_param(SEQUENCE_PARAM).unwrap_or(""));
    let seq = project.sequence(&id).ok()?;
    Some((seq, place_cuts(seq).ok()?))
}

/// シーケンスの時刻を、レイヤーを通したコンポジションの時刻にする（`start + (t - in) / speed`）。
fn comp_secs(layer: &Layer, seq_secs: f64) -> f64 {
    layer.start.to_f64() + (seq_secs - layer.in_point.to_f64()) / layer.speed.to_f64()
}

/// Timeline の `Item` と同じ並びの、シーケンスのカットの ID。シーケンスでないレイヤーは空。
/// Timeline で選んだカット（添字）を操作の ID に引き直すのに使う。
pub fn timeline_cut_ids(project: &Project, comp: &Composition) -> Vec<Vec<String>> {
    visible_layers(comp)
        .map(|(layer, _)| {
            layer_cuts(project, layer)
                .map(|(_, placed)| placed.into_iter().map(|p| p.id.to_string()).collect())
                .unwrap_or_default()
        })
        .collect()
}

/// コンポジションのレイヤーを Timeline に並べる。上のレイヤーが先頭。時間軸が壊れたレイヤーはスキップする。
/// シーケンスのレイヤーには、有効なカットを先頭から並べる（隙間は空きとして印を付ける）。
pub fn timeline_model(project: &Project, comp: &Composition) -> TimelineModel {
    let items = visible_layers(comp)
        .map(|(layer, range)| Item {
            start: range.start.to_f64(),
            end: range.end.to_f64(),
            kind: kind_of(layer),
            enabled: layer.enabled,
            cuts: cut_bars(project, layer),
        })
        .collect();
    TimelineModel {
        duration: comp.duration.to_f64(),
        fps: comp.fps.to_f64(),
        items,
    }
}

fn cut_bars(project: &Project, layer: &Layer) -> Vec<CutBar> {
    let Some((seq, placed)) = layer_cuts(project, layer) else {
        return Vec::new();
    };
    placed
        .iter()
        .map(|p| CutBar {
            start: comp_secs(layer, p.range.start.to_f64()),
            end: comp_secs(layer, p.range.end.to_f64()),
            gap: seq.cuts[p.index].source.is_gap(),
        })
        .collect()
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
        let m = timeline_model(&Project::new(), &comp());
        assert_eq!(m.items.len(), 2);
        assert_eq!(m.items[0].kind, ItemKind::Text);
        assert_eq!((m.items[0].start, m.items[0].end), (2.0, 7.0));
        assert_eq!(m.items[1].kind, ItemKind::Footage);
        assert_eq!((m.duration, m.fps), (10.0, 30.0));
    }

    #[test]
    fn 選択の添字はモデルと同じ並びでレイヤーの_id_に引ける() {
        let mut c = comp();
        // 時間軸が壊れた（出が入より前の）レイヤーはモデルからスキップし、添字がずれない。
        c.layers.insert(1, layer("broken", "text", 0, 5, 1));
        let ids = timeline_layer_ids(&c);
        assert_eq!(ids, ["top", "bottom"]);
        assert_eq!(timeline_model(&Project::new(), &c).items.len(), ids.len());
    }

    #[test]
    fn シーケンスのレイヤーはカットを並べ_隙間に印を付ける() {
        use mm_doc::Cut;
        let cut = |id: &str, node: Node, a: i64, b: i64| {
            Cut::new(id, node, RationalTime::seconds(a), RationalTime::seconds(b))
        };
        let mut project = Project::new();
        project.sequences.push(Sequence {
            id: Id::new("s"),
            cuts: vec![
                cut("k1", Node::footage(&Id::new("a")), 0, 3),
                cut("k2", Node::gap(), 0, 1),
                cut("k3", Node::footage(&Id::new("a")), 5, 7),
            ],
            origin: None,
        });
        project.sequences[0].cuts[2].enabled = false;
        let mut seq = layer("seq", SEQUENCE_KIND, 1, 0, 9);
        seq.source = Node::new(SEQUENCE_KIND, 1)
            .with_param(SEQUENCE_PARAM, mm_types::ParamValue::Text("s".into()));
        let mut c = comp();
        c.layers.push(seq);
        let m = timeline_model(&project, &c);
        let item = &m.items[0];
        assert_eq!(item.kind, ItemKind::Sequence);
        let bars: Vec<_> = item.cuts.iter().map(|b| (b.start, b.end, b.gap)).collect();
        assert_eq!(
            bars,
            [(1.0, 4.0, false), (4.0, 5.0, true)],
            "レイヤーの開始分ずらし、無効なカットは出さない"
        );
        assert_eq!(timeline_cut_ids(&project, &c)[0], ["k1", "k2"]);
        assert!(timeline_cut_ids(&project, &c)[1].is_empty());
    }

    #[test]
    fn 知らない種類は_other() {
        let mut c = comp();
        c.layers.push(layer("x", "web.cef", 0, 0, 1));
        assert_eq!(
            timeline_model(&Project::new(), &c).items[0].kind,
            ItemKind::Other
        );
    }
}
