//! 操作の記録を、人が読める 1 行にする（`mm diff`、`mm log`）。
//!
//! 同じ所への連続した `setParam` は 1 行に集約する。知らない操作は名前とフィールドをそのまま出す。

use serde_json::Value;

use mm_store::StoredTxn;

/// 1 行の説明と、集約するためのキー（同じキーが続けば 1 行にする）。
struct Line {
    text: String,
    fold_key: Option<String>,
    count: usize,
}

/// 範囲のトランザクションを、操作ごとの文にして並べる。
pub fn describe_txns(txns: &[StoredTxn]) -> Vec<String> {
    let mut lines: Vec<Line> = Vec::new();
    for op in txns.iter().flat_map(|t| t.ops.iter()) {
        let (text, fold_key) = describe_op(&op.record.op);
        match lines.last_mut() {
            Some(last) if fold_key.is_some() && last.fold_key == fold_key => last.count += 1,
            _ => lines.push(Line {
                text,
                fold_key,
                count: 1,
            }),
        }
    }
    lines
        .into_iter()
        .map(|l| {
            if l.count > 1 {
                format!("{}（{} 回）", l.text, l.count)
            } else {
                l.text
            }
        })
        .collect()
}

/// 操作 1 つの文（`mm proposal list` が提案の操作を並べる）。
pub fn describe_op_text(op: &Value) -> String {
    describe_op(op).0
}

fn describe_op(op: &Value) -> (String, Option<String>) {
    let name = op["op"].as_str().unwrap_or("?");
    if name == "setParam" {
        return set_param(op);
    }
    let text = cut_op(name, op)
        .or_else(|| layer_op(name, op))
        .or_else(|| project_op(name, op))
        .unwrap_or_else(|| generic(name, op));
    (text, None)
}

/// シーケンスのカットを動かす操作。
fn cut_op(name: &str, op: &Value) -> Option<String> {
    let text = match name {
        "ripple" => ripple(op),
        "roll" => format!(
            "カット {} と次のカットの境目を {} 秒{}",
            id(op, "cut"),
            time_abs(&op["by"]),
            direction(&op["by"], "後ろへずらした", "前へずらした")
        ),
        "slip" => format!(
            "カット {} の中身を {} 秒{}",
            id(op, "cut"),
            time_abs(&op["by"]),
            direction(&op["by"], "先へずらした", "手前へずらした")
        ),
        "slide" => format!(
            "カット {} を {} 秒{}",
            id(op, "cut"),
            time_abs(&op["by"]),
            direction(&op["by"], "後ろへ動かした", "前へ動かした")
        ),
        "split" => format!(
            "カット {} を {} 秒で割り、{} を作った",
            id(op, "cut"),
            time_text(&op["at"]),
            id(op, "new_cut")
        ),
        "join" => format!("カット {} と {} をつないだ", id(op, "cut"), id(op, "next")),
        "insert" => format!(
            "カット {} をシーケンス {} に挿入した",
            cut_id_of(&op["cut"]),
            id(op, "sequence")
        ),
        "extract" => format!("カット {} を抜き取った", id(op, "cut")),
        "disable" => format!(
            "カット {} を{}",
            id(op, "cut"),
            if op["disabled"] == false {
                "有効にした"
            } else {
                "無効にした"
            }
        ),
        _ => return None,
    };
    Some(text)
}

/// レイヤーの操作。
fn layer_op(name: &str, op: &Value) -> Option<String> {
    let text = match name {
        "addLayer" => format!(
            "レイヤー {} をコンポジション {} に足した",
            op["layer"]["id"].as_str().unwrap_or("?"),
            id(op, "composition")
        ),
        "removeLayer" => format!("レイヤー {} を外した", id(op, "layer")),
        "moveLayer" => format!("レイヤー {} の重なりを変えた", id(op, "layer")),
        "lockLayer" => format!(
            "レイヤー {} を{}",
            id(op, "layer"),
            if op["lock"] == false {
                "ロック解除した"
            } else {
                "ロックした"
            }
        ),
        _ => return None,
    };
    Some(text)
}

/// シーケンス、コンポジション、マーカーの出し入れ。
fn project_op(name: &str, op: &Value) -> Option<String> {
    let text = match name {
        "addSequence" => format!(
            "シーケンス {}（カット {} 個）を足した",
            op["sequence"]["id"].as_str().unwrap_or("?"),
            op["sequence"]["cuts"].as_array().map_or(0, Vec::len)
        ),
        "removeSequence" => format!("シーケンス {} を外した", id(op, "sequence")),
        "addComposition" => format!(
            "コンポジション {} を足した",
            op["composition"]["id"].as_str().unwrap_or("?")
        ),
        "removeComposition" => format!("コンポジション {} を外した", id(op, "composition")),
        "addMarker" => format!(
            "マーカー {} を足した",
            op["marker"]["id"].as_str().unwrap_or("?")
        ),
        "removeMarker" => format!("マーカー {} を外した", id(op, "marker")),
        _ => return None,
    };
    Some(text)
}

fn ripple(op: &Value) -> String {
    let edge_in = op["edge"] == "in";
    let by = &op["by"];
    // イン点は正に動かすと短く、アウト点は正に動かすと長くなる。
    let shorter = edge_in == is_positive(by);
    format!(
        "カット {} の{}を {} 秒{}",
        id(op, "cut"),
        if edge_in { "イン点" } else { "アウト点" },
        time_abs(by),
        if shorter { "詰めた" } else { "伸ばした" }
    )
}

fn set_param(op: &Value) -> (String, Option<String>) {
    let target = &op["target"];
    let (kind, owner) = match (target["layer"].as_str(), target["cut"].as_str()) {
        (Some(l), _) => ("レイヤー", l),
        (None, Some(c)) => ("カット", c),
        _ => ("?", "?"),
    };
    let path = target["path"].as_str().unwrap_or("?");
    (
        format!(
            "{kind} {owner} の {path} を {} にした",
            value_text(&op["value"])
        ),
        Some(format!("setParam:{kind}:{owner}:{path}")),
    )
}

fn generic(name: &str, op: &Value) -> String {
    let mut fields = op.clone();
    if let Some(o) = fields.as_object_mut() {
        o.remove("op");
    }
    format!("{name} {fields}")
}

fn id(op: &Value, key: &str) -> String {
    op[key].as_str().unwrap_or("?").to_owned()
}

fn cut_id_of(cut: &Value) -> String {
    cut["id"].as_str().unwrap_or("?").to_owned()
}

fn value_text(v: &Value) -> String {
    v.get("value")
        .map_or_else(|| v.to_string(), Value::to_string)
}

fn is_positive(t: &Value) -> bool {
    t["num"].as_i64().unwrap_or(0) > 0
}

fn direction(t: &Value, positive: &str, negative: &str) -> String {
    (if is_positive(t) { positive } else { negative }).to_owned()
}

/// 時刻の絶対値を秒の文字にする。
fn time_abs(t: &Value) -> String {
    let abs = serde_json::json!({
        "num": t["num"].as_i64().unwrap_or(0).unsigned_abs(),
        "den": t["den"],
    });
    time_text(&abs)
}

/// `{num, den}` を秒の 10 進にする。割り切れなければ `num/den`。
fn time_text(t: &Value) -> String {
    let (Some(num), Some(den)) = (t["num"].as_i64(), t["den"].as_u64()) else {
        return t.to_string();
    };
    if den == 0 {
        return t.to_string();
    }
    let mut d = den;
    let (mut twos, mut fives) = (0u32, 0u32);
    while d % 2 == 0 {
        d /= 2;
        twos += 1;
    }
    while d % 5 == 0 {
        d /= 5;
        fives += 1;
    }
    if d != 1 {
        return format!("{num}/{den}");
    }
    let places = twos.max(fives) as usize;
    let scale = 10u128.pow(places as u32);
    let scaled = num.unsigned_abs() as u128 * scale / den as u128;
    let sign = if num < 0 { "-" } else { "" };
    if places == 0 {
        return format!("{sign}{scaled}");
    }
    let s = format!("{scaled:0>width$}", width = places + 1);
    let (int, frac) = s.split_at(s.len() - places);
    format!("{sign}{int}.{frac}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn 時刻は割り切れれば_10_進で出す() {
        assert_eq!(time_text(&json!({"num": 2, "den": 5})), "0.4");
        assert_eq!(time_text(&json!({"num": -3, "den": 2})), "-1.5");
        assert_eq!(time_text(&json!({"num": 4, "den": 1})), "4");
        assert_eq!(time_text(&json!({"num": 1, "den": 3})), "1/3");
    }

    #[test]
    fn アウト点を負に動かすと詰めたと読む() {
        let op = json!({"op": "ripple", "cut": "c12", "edge": "out", "by": {"num": -2, "den": 5}});
        assert_eq!(describe_op(&op).0, "カット c12 のアウト点を 0.4 秒詰めた");
        let op = json!({"op": "ripple", "cut": "c12", "edge": "in", "by": {"num": -2, "den": 5}});
        assert_eq!(describe_op(&op).0, "カット c12 のイン点を 0.4 秒伸ばした");
    }
}
