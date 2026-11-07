//! MCP のツールの定義。操作ごとのツールの `inputSchema` は `mm_ops::ops_schema()` から作る。

use std::collections::{BTreeSet, HashSet};
use std::sync::OnceLock;

use serde_json::{json, Map, Value};

/// 操作のツールが共通で受ける任意の引数。操作自身のフィールドと名前が衝突してはいけない。
pub(crate) const COMMON_ARGS: [&str; 3] = ["label", "base_version", "responds_to"];

const DEFS_PREFIX: &str = "#/$defs/";

/// ツールの一覧と、操作のツールの名前。
pub(crate) struct Tools {
    pub list: Vec<Value>,
    pub op_names: HashSet<String>,
}

/// 起動後に 1 度だけ作る。スキーマは凍結されていて実行中に変わらない。
pub(crate) fn tools() -> &'static Tools {
    static TOOLS: OnceLock<Tools> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let schema = serde_json::to_value(mm_ops::ops_schema()).expect("スキーマは JSON になる");
        build(&schema)
    })
}

/// 操作のスキーマ（トップの `oneOf` と `$defs`）からツールを作る。
fn build(schema: &Value) -> Tools {
    let empty = Map::new();
    let defs = schema["$defs"].as_object().unwrap_or(&empty);
    let variants = schema["oneOf"].as_array().cloned().unwrap_or_default();
    let mut list = Vec::new();
    let mut op_names = HashSet::new();
    for variant in &variants {
        if let Some(tool) = op_tool(variant, defs) {
            op_names.insert(tool["name"].as_str().unwrap_or_default().to_owned());
            list.push(tool);
        }
    }
    list.push(transaction_tool(
        "transaction",
        "操作の列を 1 つのトランザクションとして適用する。コメントへの対応 1 回をまとめて 1 つの取り消しの単位にするときに使う。途中で失敗すれば全部適用しない",
        &variants,
        defs,
    ));
    list.push(transaction_tool(
        "propose",
        "操作の列を適用せずに提案として保留する（引数は transaction と同じ）。人がアプリの Review で承認すると 1 つのトランザクションとして入り、却下すると捨てる。MCP からは承認も却下もできない",
        &variants,
        defs,
    ));
    list.extend(fixed_tools());
    Tools { list, op_names }
}

fn op_tool(variant: &Value, defs: &Map<String, Value>) -> Option<Value> {
    let name = variant["properties"]["op"]["const"].as_str()?;
    let mut properties = variant["properties"].as_object()?.clone();
    properties.remove("op");
    let required: Vec<Value> = variant["required"]
        .as_array()
        .map(|r| r.iter().filter(|k| *k != "op").cloned().collect())
        .unwrap_or_default();
    let mut used = BTreeSet::new();
    collect_refs(&Value::Object(properties.clone()), defs, &mut used);
    properties.extend(common_properties());
    let mut input = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
        "$defs": pick(defs, &used),
    });
    if !required.is_empty() {
        input["required"] = Value::Array(required);
    }
    Some(json!({
        "name": name,
        "description": variant["description"].as_str().unwrap_or(name),
        "inputSchema": input,
    }))
}

fn common_properties() -> Map<String, Value> {
    let Value::Object(map) = json!({
        "label": {"type": "string", "description": "トランザクションの名前。省くと操作名"},
        "base_version": {"type": "integer", "minimum": 0, "description": "元にしたプロジェクトのデータのバージョン。省くと今のバージョン"},
        "responds_to": {"type": "string", "description": "対応しているコメントの ID"},
    }) else {
        unreachable!("オブジェクトのリテラル")
    };
    map
}

fn transaction_tool(
    name: &str,
    description: &str,
    variants: &[Value],
    defs: &Map<String, Value>,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "トランザクションの名前"},
                "ops": {"type": "array", "items": {"oneOf": variants}, "description": "順に適用する操作（`op` に操作名を入れる）"},
                "base_version": {"type": "integer", "minimum": 0, "description": "元にしたプロジェクトのデータのバージョン。省くと今のバージョン"},
                "responds_to": {"type": "string", "description": "対応しているコメントの ID"},
            },
            "required": ["name", "ops"],
            "additionalProperties": false,
            "$defs": defs,
        },
    })
}

fn fixed_tools() -> Vec<Value> {
    let none = json!({"type": "object", "properties": {}, "additionalProperties": false});
    vec![
        json!({"name": "undo", "description": "直前のトランザクションを取り消す。戻すものが無ければ change が null", "inputSchema": none}),
        json!({"name": "redo", "description": "取り消したトランザクションをやり直す。無ければ change が null", "inputSchema": none}),
        json!({"name": "project", "description": "今のプロジェクトのデータ（version、undo_depth、project）を読む", "inputSchema": none}),
        json!({"name": "proposals", "description": "提案の一覧（新しい順）を読む。状態（pending、approved、rejected）、触る範囲 scope、後の編集と重なる conflict", "inputSchema": none}),
        json!({
            "name": "frame",
            "description": "1 フレームを描いた PNG を返す。仕上がりを目で確かめるのに使う。proposal を渡すと提案を適用したフレーム（書かない）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "t": {"type": "string", "description": "コンポジションの時間軸での秒。\"1.5\" や \"3/2\""},
                    "comp": {"type": "string", "description": "コンポジションの ID。省くと main、それも無ければ先頭"},
                    "width": {"type": "integer", "minimum": 1, "description": "出力の幅。省くとコンポジションの大きさ"},
                    "proposal": {"type": "integer", "description": "提案の ID。あれば提案を適用したフレームを描く"},
                    "ops": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "提案のうち適用する操作の番号。省くと全部"},
                },
                "required": ["t"],
                "additionalProperties": false,
            },
        }),
    ]
}

/// `value` が `#/$defs/X` で参照する定義を、定義の中の参照もたどって集める。
fn collect_refs(value: &Value, defs: &Map<String, Value>, used: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(name) = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.strip_prefix(DEFS_PREFIX))
            {
                if used.insert(name.to_owned()) {
                    if let Some(def) = defs.get(name) {
                        collect_refs(def, defs, used);
                    }
                }
            }
            map.values().for_each(|v| collect_refs(v, defs, used));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_refs(v, defs, used)),
        _ => {}
    }
}

fn pick(defs: &Map<String, Value>, used: &BTreeSet<String>) -> Value {
    Value::Object(
        used.iter()
            .filter_map(|k| defs.get(k).map(|v| (k.clone(), v.clone())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 操作のフィールドは共通の引数と名前が衝突しない() {
        let schema = serde_json::to_value(mm_ops::ops_schema()).unwrap();
        for variant in schema["oneOf"].as_array().unwrap() {
            for key in variant["properties"].as_object().unwrap().keys() {
                assert!(
                    !COMMON_ARGS.contains(&key.as_str()),
                    "{} が共通の引数 {key} と衝突する",
                    variant["properties"]["op"]["const"]
                );
            }
        }
    }
}
