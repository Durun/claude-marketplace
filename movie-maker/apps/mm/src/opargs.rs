//! `mm op <dir> <操作名> --引数 値 ...` の引数を、操作の JSON に組み立てる。
//!
//! 引数の名前と型は `mm_ops::ops_schema()` から引く。スキーマが無いフィールドは受けない。
//! 値はフィールドの型で読む（文字列のフィールドはそのまま、時刻のフィールドは `-0.4` や `3/2` から `{num, den}` へ、
//! それ以外は JSON、JSON でなければ文字列）。複雑なフィールドは `--json '{...}'` で一度に渡せる。

use serde_json::{json, Map, Value};

use mm_ops::Op;
use mm_types::{MmError, RationalTime, Result};

/// 操作のフィールドではなく、`mm op` 自身が読む引数。
const RESERVED_JSON: &str = "json";
const RESERVED_BASE: &str = "base";
const RESERVED_LABEL: &str = "label";
/// 10 進の小数を時刻にするときの小数点以下の桁数の上限（分母が `u32` に収まる範囲）。
const MAX_DECIMALS: usize = 9;

/// 組み立てた操作と、`mm op` 自身の引数。
#[derive(Debug)]
pub struct ParsedOp {
    pub op: Op,
    /// 元にしたバージョン（`--base`）。
    pub base: Option<u64>,
    /// 記録に残すトランザクションの名前（`--label`）。
    pub label: Option<String>,
}

/// 操作の名前と、名前の後ろの引数の並びから操作を作る。
pub fn parse_op(name: &str, tokens: &[String]) -> Result<ParsedOp> {
    let schema = branch_schema(name)?;
    let mut object = Map::new();
    let (mut base, mut label) = (None, None);
    let mut json_args: Option<Value> = None;
    for (key, value) in flag_pairs(tokens)? {
        match key.as_str() {
            RESERVED_JSON => json_args = Some(serde_json::from_str(&value)?),
            RESERVED_BASE => base = Some(parse_u64(&value)?),
            RESERVED_LABEL => label = Some(value),
            _ => {
                let (field, field_schema) = find_field(&schema, name, &key)?;
                object.insert(field, coerce(&field_schema, &value)?);
            }
        }
    }
    let mut merged = match json_args {
        None => Map::new(),
        Some(Value::Object(m)) => m,
        Some(_) => return Err(invalid("--json は JSON のオブジェクトで渡す".into())),
    };
    merged.extend(object);
    merged.insert("op".into(), Value::String(name.to_owned()));
    let op: Op = serde_json::from_value(Value::Object(merged))
        .map_err(|e| invalid(format!("操作 {name} の引数: {e}")))?;
    Ok(ParsedOp { op, base, label })
}

/// 操作の名前とフィールドの一覧（`mm op` の案内）。
pub fn usage() -> String {
    format!("操作: {}", Op::NAMES.join(", "))
}

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

fn parse_u64(s: &str) -> Result<u64> {
    s.parse()
        .map_err(|_| invalid(format!("非負の整数でない: {s}")))
}

/// `--key value` と `--key=value` の並びを組にする。値の先頭が `-` でも（`-0.4`）値として読む。
/// 値の無い `--flag` は `true`。
fn flag_pairs(tokens: &[String]) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let Some(body) = tokens[i].strip_prefix("--") else {
            return Err(invalid(format!(
                "`--名前 値` の形でない引数: {}",
                tokens[i]
            )));
        };
        i += 1;
        if let Some((k, v)) = body.split_once('=') {
            pairs.push((k.to_owned(), v.to_owned()));
        } else if i < tokens.len() && !is_flag(&tokens[i]) {
            pairs.push((body.to_owned(), tokens[i].clone()));
            i += 1;
        } else {
            pairs.push((body.to_owned(), "true".to_owned()));
        }
    }
    Ok(pairs)
}

/// `--abc` の形か（`--` の後に英字）。`-0.4` や `--5` は値として扱う。
fn is_flag(token: &str) -> bool {
    token
        .strip_prefix("--")
        .is_some_and(|r| r.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
}

/// 操作 `name` のバリアントのスキーマ（`$defs` を `defs` の名前で引けるよう、バリアントに `$defs` を付けて返す）。
fn branch_schema(name: &str) -> Result<Value> {
    let root = serde_json::to_value(mm_ops::ops_schema())?;
    let branch = root["oneOf"]
        .as_array()
        .and_then(|bs| bs.iter().find(|b| b["properties"]["op"]["const"] == name))
        .cloned()
        .ok_or_else(|| invalid(format!("知らない操作: {name}（{}）", usage())))?;
    Ok(json!({ "branch": branch, "defs": root["$defs"] }))
}

/// 引数の名前に合うフィールドを引く。`-` と `_` と大文字小文字を区別しない（`--new-cut` で `new_cut`）。
fn find_field(schema: &Value, op: &str, key: &str) -> Result<(String, Value)> {
    let props = schema["branch"]["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let want = normalize(key);
    if let Some((field, s)) = props
        .iter()
        .find(|(f, _)| *f != "op" && normalize(f) == want)
    {
        return Ok((field.clone(), resolve(schema, s)));
    }
    let names: Vec<&str> = props
        .keys()
        .map(String::as_str)
        .filter(|f| *f != "op")
        .collect();
    Err(invalid(format!(
        "操作 {op} に引数 --{key} は無い（使える引数: {}）",
        names.join(", ")
    )))
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// `$ref` を 1 段たどる。参照の名前は `x-ref` に残す（時刻と指し方の判定に使う）。
fn resolve(schema: &Value, field: &Value) -> Value {
    let Some(r) = field["$ref"].as_str() else {
        return field.clone();
    };
    let name = r.rsplit('/').next().unwrap_or_default();
    let mut out = schema["defs"][name].clone();
    out["x-ref"] = Value::String(name.to_owned());
    out
}

/// フィールドの型で値を読む。
fn coerce(field: &Value, raw: &str) -> Result<Value> {
    match field["x-ref"].as_str() {
        Some("RationalTime") => return parse_time(raw),
        Some("ParamRef") if !raw.trim_start().starts_with('{') => return parse_param_ref(raw),
        _ => {}
    }
    if raw == "null" && allows_null(field) {
        return Ok(Value::Null);
    }
    if is_string_field(field) {
        return Ok(Value::String(raw.to_owned()));
    }
    Ok(serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned())))
}

fn type_names(field: &Value) -> Vec<&str> {
    match &field["type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// 文字列だけを取るフィールドか（`"123"` のような ID を数に読まない）。
fn is_string_field(field: &Value) -> bool {
    let names = type_names(field);
    !names.is_empty() && names.iter().all(|t| *t == "string" || *t == "null")
}

/// null も取れるフィールドか（`--cut null` を文字の "null" にしない）。
fn allows_null(field: &Value) -> bool {
    type_names(field).contains(&"null")
}

/// `layer:<ID>:<パス>` か `cut:<ID>:<パス>` を `{layer|cut, path}` にする。
fn parse_param_ref(raw: &str) -> Result<Value> {
    let mut parts = raw.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(kind @ ("layer" | "cut")), Some(id), Some(path)) => {
            Ok(json!({ kind: id, "path": path }))
        }
        _ => Err(invalid(format!(
            "パラメータの指し方は `layer:<ID>:<パス>` か `cut:<ID>:<パス>`（JSON も可）: {raw}"
        ))),
    }
}

/// 秒の指定を `{num, den}` にする。`3/2` と 10 進（`-0.4`）を受ける。
pub fn parse_time(raw: &str) -> Result<Value> {
    let s = raw.trim();
    let (num, den) = if let Some((n, d)) = s.split_once('/') {
        (parse_i128(n)?, parse_den(d)?)
    } else {
        parse_decimal(s)?
    };
    let t = RationalTime::new(num, den).map_err(|e| invalid(format!("時刻 {raw}: {e}")))?;
    Ok(serde_json::to_value(t)?)
}

fn parse_i128(s: &str) -> Result<i128> {
    s.trim()
        .parse()
        .map_err(|_| invalid(format!("時刻の数として読めない: {s}")))
}

fn parse_den(s: &str) -> Result<u32> {
    s.trim()
        .parse()
        .map_err(|_| invalid(format!("時刻の分母として読めない: {s}")))
}

/// `-0.4` を (-4, 10) にする。指数表記は受けない。
fn parse_decimal(s: &str) -> Result<(i128, u32)> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if frac.len() > MAX_DECIMALS || int.is_empty() && frac.is_empty() {
        return Err(invalid(format!("時刻として読めない: {s}")));
    }
    let digits = format!("{int}{frac}");
    let magnitude: i128 = digits
        .parse()
        .map_err(|_| invalid(format!("時刻として読めない: {s}")))?;
    let den = 10u32.pow(frac.len() as u32);
    Ok((if neg { -magnitude } else { magnitude }, den))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn 負の小数が既約の有理数の時刻になる() {
        assert_eq!(parse_time("-0.4").unwrap(), json!({"num": -2, "den": 5}));
        assert_eq!(parse_time("3/6").unwrap(), json!({"num": 1, "den": 2}));
        assert_eq!(parse_time("2").unwrap(), json!({"num": 2, "den": 1}));
        assert!(parse_time("abc").is_err());
    }

    #[test]
    fn ripple_の引数が操作になる() {
        let p = parse_op("ripple", &tokens("--cut c12 --edge out --by -0.4 --base 3")).unwrap();
        assert_eq!(p.base, Some(3));
        let v = serde_json::to_value(&p.op).unwrap();
        assert_eq!(v["op"], "ripple");
        assert_eq!(v["cut"], "c12");
        assert_eq!(v["by"], json!({"num": -2, "den": 5}));
    }

    #[test]
    fn null_も取れるフィールドの_null_は_json_の_null() {
        let p = parse_op("attachLayer", &tokens("--layer l1 --cut null")).unwrap();
        let v = serde_json::to_value(&p.op).unwrap();
        assert_eq!(v["cut"], Value::Null, "{v}");
        let p = parse_op("attachLayer", &tokens("--layer l1 --cut c1")).unwrap();
        assert_eq!(serde_json::to_value(&p.op).unwrap()["cut"], "c1");
    }

    #[test]
    fn 数字だけの_id_は文字列のまま() {
        let p = parse_op("extract", &tokens("--cut 123")).unwrap();
        assert_eq!(serde_json::to_value(&p.op).unwrap()["cut"], "123");
    }

    #[test]
    fn パラメータの指し方の略記と_json_の併用() {
        let p = parse_op(
            "setParam",
            &tokens(
                "--target layer:l1:opacity --json {\"value\":{\"type\":\"number\",\"value\":0.5}}",
            ),
        )
        .unwrap();
        let v = serde_json::to_value(&p.op).unwrap();
        assert_eq!(v["target"], json!({"layer": "l1", "path": "opacity"}));
        assert_eq!(v["value"]["value"], 0.5);
    }

    #[test]
    fn 知らない操作と知らない引数は使える名前を添えて拒否する() {
        let e = parse_op("nope", &[]).unwrap_err().to_string();
        assert!(e.contains("ripple"), "{e}");
        let e = parse_op("ripple", &tokens("--cut c --zzz 1"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("--zzz") && e.contains("edge"), "{e}");
    }
}
