//! v6 → v7: 本編とショート。
//!
//! 増えたフィールド（`Cut.overrides`、`Cut.origin_base`、`Layer.framing`）は serde の既定で埋まる。
//! 複製のカットだけは比べ先が要るので、今の元のカットの値を元の値（`origin_base`）にする
//! （v6 までに元が変わっていても分からないので、上げた時点から後の変化を知らせる）。

use std::collections::HashMap;

use serde_json::{json, Value};

/// 複製を表す `relation` の値。
const FORK: &str = "fork";
/// 元の値に写す必須のフィールド。速さと有効は無ければ既定（1、有効）。
const BASE_FIELDS: [&str; 3] = ["source", "in", "out"];

/// v6 → v7。
pub(crate) fn v6_to_v7(value: &mut Value) {
    let Some(seqs) = value.get_mut("sequences").and_then(Value::as_array_mut) else {
        return;
    };
    let bases: HashMap<String, Value> = seqs
        .iter()
        .filter_map(|s| s.get("cuts")?.as_array())
        .flatten()
        .filter_map(|c| Some((c.get("id")?.as_str()?.to_owned(), base_of(c)?)))
        .collect();
    for seq in seqs.iter_mut().filter(|s| is_fork(s)) {
        let Some(cuts) = seq.get_mut("cuts").and_then(Value::as_array_mut) else {
            continue;
        };
        for cut in cuts {
            let base = cut
                .get("origin")
                .and_then(Value::as_str)
                .and_then(|o| bases.get(o));
            if let Some(base) = base {
                cut["origin_base"] = base.clone();
            }
        }
    }
}

fn is_fork(seq: &Value) -> bool {
    seq.get("origin")
        .and_then(|o| o.get("relation"))
        .and_then(Value::as_str)
        == Some(FORK)
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

    use crate::{load, CutBase};

    fn v6_with_fork(relation: &str) -> String {
        let cut = |id: &str, origin: &str| {
            format!(
                r#"{{"id": "{id}", "in": {{"num": 1, "den": 1}}, "out": {{"num": 3, "den": 1}},
                  "speed": {{"num": 1, "den": 1}}, "enabled": true, "transition_in": null,
                  "origin": {origin}, "source": {{"kind": "solid", "version": 1, "params": {{}}}}}}"#
            )
        };
        format!(
            r#"{{"schema_version": 6, "assets": [], "compositions": [], "render_targets": [],
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
    fn v6_の複製のカットは今の元の値を元の値にして上がる() {
        let p = load(&v6_with_fork("fork")).unwrap();
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
    fn v6_の参照のカットは元の値を持たない() {
        let p = load(&v6_with_fork("reference")).unwrap();
        assert_eq!(p.cut(&Id::new("s1")).unwrap().1.origin_base, None);
    }
}
