//! v7 → v8: 本編とショート。
//!
//! 増えたフィールド（`Cut.overrides`、`Cut.origin_base`、`Layer.framing`）は serde の既定で埋まる。
//! v7 は元に従う所と上書きした所を区別しないので、元と違う値を上書きとして残し、最初の操作の同期で消えないようにする。
//!
//! - 参照のショートのカットの元と違う in/out は `overrides` を立てる。
//! - 参照のショートに素材・速さ・有効が元と違うカットが 1 つでもあれば、ショートごと複製にする。
//!   参照はそれらを必ず元に従わせるので、参照のままでは値を失う。読み込みを拒否するより、値を全部残す複製を選ぶ。
//! - 参照と複製のカットの `origin_base` は今の元の値にする（v7 までの元の変化は分からないので、上げた時点から後を知らせる）。

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::sequence::CutBase;

/// 複製を表す `relation` の値。
const FORK: &str = "fork";
/// 参照を表す `relation` の値。
const REFERENCE: &str = "reference";
/// 元の値にコピーする必須のフィールド。速さと有効は無ければ既定（1、有効）。
const BASE_FIELDS: [&str; 3] = ["source", "in", "out"];

/// v7 → v8。
pub(crate) fn v7_to_v8(value: &mut Value) {
    let Some(seqs) = value.get_mut("sequences").and_then(Value::as_array_mut) else {
        return;
    };
    let bases: HashMap<String, Value> = seqs
        .iter()
        .filter_map(|s| s.get("cuts")?.as_array())
        .flatten()
        .filter_map(|c| Some((c.get("id")?.as_str()?.to_owned(), base_of(c)?)))
        .collect();
    for seq in seqs.iter_mut() {
        if relation(seq) == Some(REFERENCE) && has_unfollowable_cut(seq, &bases) {
            seq["origin"]["relation"] = Value::from(FORK);
        }
        migrate_cuts(seq, &bases);
    }
}

/// ショートのカットに元の値と、参照なら上書きのフラグを付ける。
fn migrate_cuts(seq: &mut Value, bases: &HashMap<String, Value>) {
    let Some(reference) = relation(seq).map(|r| r == REFERENCE) else {
        return;
    };
    let Some(cuts) = seq.get_mut("cuts").and_then(Value::as_array_mut) else {
        return;
    };
    for cut in cuts {
        let Some(base) = origin_base(cut, bases) else {
            continue;
        };
        if reference {
            cut["overrides"] = overrides_of(cut, base);
        }
        cut["origin_base"] = base.clone();
    }
}

/// ショートの `relation`。本編は `None`。
fn relation(seq: &Value) -> Option<&str> {
    seq.get("origin")?.get("relation")?.as_str()
}

fn origin_base<'a>(cut: &Value, bases: &'a HashMap<String, Value>) -> Option<&'a Value> {
    bases.get(cut.get("origin")?.as_str()?)
}

/// 元のカットとショートのカットの値を比べられる形にする。読めない値は `None`（比べずに v8 の検証に任せる）。
fn typed_pair(cut: &Value, base: &Value) -> Option<(CutBase, CutBase)> {
    let parse = |v: Value| serde_json::from_value::<CutBase>(v).ok();
    parse(base.clone()).zip(parse(base_of(cut)?))
}

/// 素材・速さ・有効のどれかが元と違うカットがあるか。
fn has_unfollowable_cut(seq: &Value, bases: &HashMap<String, Value>) -> bool {
    let Some(cuts) = seq.get("cuts").and_then(Value::as_array) else {
        return false;
    };
    cuts.iter().any(|cut| {
        let pair = origin_base(cut, bases).and_then(|b| typed_pair(cut, b));
        pair.is_some_and(|(main, own)| {
            main.source != own.source || main.speed != own.speed || main.enabled != own.enabled
        })
    })
}

/// 元と違う in/out を上書きのフラグにする。
fn overrides_of(cut: &Value, base: &Value) -> Value {
    let (in_point, out_point) = typed_pair(cut, base).map_or((false, false), |(main, own)| {
        (
            main.in_point != own.in_point,
            main.out_point != own.out_point,
        )
    });
    json!({"in": in_point, "out": out_point})
}

/// カットの JSON から元の値の JSON を作る。必須のフィールドが無ければ `None`。
fn base_of(cut: &Value) -> Option<Value> {
    let mut base = serde_json::Map::new();
    for key in BASE_FIELDS {
        base.insert(key.to_owned(), cut.get(key)?.clone());
    }
    let speed = cut
        .get("speed")
        .cloned()
        .unwrap_or(json!({"num": 1, "den": 1}));
    let enabled = cut.get("enabled").cloned().unwrap_or(Value::Bool(true));
    base.insert("speed".to_owned(), speed);
    base.insert("enabled".to_owned(), enabled);
    Some(Value::Object(base))
}

#[cfg(test)]
mod tests {
    use mm_types::{Id, RationalTime};

    use crate::{load, CutBase, OriginRelation};

    fn v7_with_fork(relation: &str) -> String {
        let cut = |id: &str, origin: &str| {
            format!(
                r#"{{"id": "{id}", "in": {{"num": 1, "den": 1}}, "out": {{"num": 3, "den": 1}},
                  "speed": {{"num": 1, "den": 1}}, "enabled": true, "transition_in": null,
                  "origin": {origin}, "source": {{"kind": "solid", "version": 1, "params": {{}}}}}}"#
            )
        };
        format!(
            r#"{{"schema_version": 7, "assets": [], "compositions": [], "render_targets": [],
              "sequences": [
                {{"id": "main", "origin": null, "cuts": [{}]}},
                {{"id": "short", "origin": {{"sequence": "main", "relation": "{relation}"}},
                  "cuts": [{}]}}
              ]}}"#,
            cut("m1", "null"),
            cut("s1", r#""m1""#)
        )
    }

    #[test]
    fn v7_の複製のカットは今の元の値を元の値にして上がる() {
        let p = load(&v7_with_fork("fork")).unwrap();
        let (_, s1) = p.cut(&Id::new("s1")).unwrap();
        let (_, m1) = p.cut(&Id::new("m1")).unwrap();
        assert_eq!(s1.origin_base, Some(CutBase::of(m1)));
        assert_eq!(
            s1.origin_base.as_ref().unwrap().out_point,
            RationalTime::seconds(3)
        );
        assert!(!s1.overrides.any());
    }

    #[test]
    fn 凍結した_v8_の本編とショートが読めて往復する() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/doc/v8/shorts.json");
        let p = load(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(p.cut(&Id::new("r1")).unwrap().1.overrides.out_point);
        let layer = &p.compositions[0].layers[0];
        let reframe = layer.framing.reframe(&Id::new("r1")).unwrap();
        assert_eq!(reframe.transform.position.keyframes.len(), 2);
        assert!(layer.framing.caption.is_some());
        let changes = p.origin_changes(&Id::new("zfork")).unwrap();
        assert_eq!(changes[0].fields, vec!["in"]);
        assert_eq!(p.short_uses(&Id::new("main"))[&Id::new("k1")].len(), 2);
        let back = load(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn v7_の元と同じ参照のカットは上書きなしで元の値を持つ() {
        let p = load(&v7_with_fork("reference")).unwrap();
        let (_, s1) = p.cut(&Id::new("s1")).unwrap();
        let (_, m1) = p.cut(&Id::new("m1")).unwrap();
        assert_eq!(s1.origin_base, Some(CutBase::of(m1)));
        assert!(!s1.overrides.any());
    }

    #[test]
    fn v7_の参照のショートで有効が元と違えば複製になる() {
        // ショートのカット s1 は後ろに書くので、最後の `enabled` だけを変える。
        let enabled = r#""enabled": true"#;
        let src = v7_with_fork("reference");
        let at = src.rfind(enabled).unwrap();
        let json = format!(
            r#"{}"enabled": false{}"#,
            &src[..at],
            &src[at + enabled.len()..]
        );
        let p = load(&json).unwrap();
        let short = p.sequence(&Id::new("short")).unwrap();
        assert_eq!(
            short.origin.as_ref().unwrap().relation,
            OriginRelation::Fork
        );
        assert!(!short.cuts[0].enabled);
        assert!(p.cut(&Id::new("m1")).unwrap().1.enabled);
    }
}
