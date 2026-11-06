//! 全部の操作の JSON Schema。`mm op --schema`、MCP のツールの型、`ui/sdk` の型の生成の元。

use schemars::schema_for;

use crate::op::Op;

/// 凍結したスキーマの置き場（クレートの根からの相対パス）。テストが最新かを確かめる。
pub const SCHEMA_PATH: &str = "schema/ops.json";

/// [`Op`] のスキーマ。`op` の欄で枝が決まる `oneOf`。
pub fn ops_schema() -> schemars::Schema {
    schema_for!(Op)
}

/// スキーマを決定的な JSON の文字列にする（インデント 2、末尾に改行 1 つ）。
pub fn ops_schema_json() -> String {
    let mut s = serde_json::to_string_pretty(&ops_schema()).expect("スキーマは JSON になる");
    s.push('\n');
    s
}
