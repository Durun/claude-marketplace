//! JSON Schema の手書きの実装。serde の `try_from` で読む型は導出が形を表せないので、プロジェクトのデータに書く形を直接書く。

use std::borrow::Cow;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};

use crate::id::Id;
use crate::time::{Rational, RationalTime, TimeRange};

impl<T> JsonSchema for Id<T> {
    fn schema_name() -> Cow<'static, str> {
        "Id".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "ID（種類ごとにプロジェクトの中で重複しない文字列）"
        })
    }
}

impl JsonSchema for Rational {
    fn schema_name() -> Cow<'static, str> {
        "Rational".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "description": "正の有理数 num/den（fps、再生の速さ）",
            "properties": {
                "num": { "type": "integer", "minimum": 1, "maximum": u32::MAX },
                "den": { "type": "integer", "minimum": 1, "maximum": u32::MAX }
            },
            "required": ["num", "den"]
        })
    }
}

impl JsonSchema for RationalTime {
    fn schema_name() -> Cow<'static, str> {
        "RationalTime".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "description": "有理数の時刻 num/den 秒（負も表せる）",
            "properties": {
                "num": { "type": "integer", "format": "int64" },
                "den": { "type": "integer", "minimum": 1, "maximum": u32::MAX }
            },
            "required": ["num", "den"]
        })
    }
}

impl JsonSchema for TimeRange {
    fn schema_name() -> Cow<'static, str> {
        "TimeRange".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let time = generator.subschema_for::<RationalTime>();
        json_schema!({
            "type": "object",
            "description": "半開区間 [start, end)",
            "properties": { "start": time, "end": time },
            "required": ["start", "end"]
        })
    }
}
