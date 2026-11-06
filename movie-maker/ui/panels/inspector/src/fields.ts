import { PARAM_VALUE_SCHEMAS, type JsonSchema, type ParamValue, type SchemaObject } from "@mm/sdk";

/** 入力欄の種類。ParamValue の JSON Schema から決める。 */
export type FieldSpec =
  | { control: "number" }
  | { control: "vector"; size: number }
  | { control: "color" }
  | { control: "bool" }
  | { control: "text" }
  | { control: "choice" }
  | { control: "asset" }
  | { control: "json" };

export type ParamKind = ParamValue["type"];

const COLOR_KEYS = ["r", "g", "b", "a"];

const isObject = (s: JsonSchema | undefined): s is SchemaObject => typeof s === "object";

const vectorSize = (s: SchemaObject): number | undefined =>
  s.type === "array" && s.minItems !== undefined && s.minItems === s.maxItems ? s.minItems : undefined;

const isColor = (s: SchemaObject): boolean =>
  s.type === "object" && COLOR_KEYS.every((k) => isObject(s.properties?.[k]));

/**
 * パラメータの値の種類から、入力欄の種類を決める。
 * 形（数、固定長の配列、r/g/b/a の object、真偽、文字）は JSON Schema で決まり、
 * 文字の中での選択肢と素材の参照は JSON Schema に差が無いので種類の名前で分ける。
 */
export const fieldSpecFor = (kind: ParamKind): FieldSpec => {
  const schema = PARAM_VALUE_SCHEMAS[kind];
  if (!isObject(schema)) return { control: "json" };
  if (schema.type === "number") return { control: "number" };
  if (schema.type === "boolean") return { control: "bool" };
  const size = vectorSize(schema);
  if (size !== undefined) return { control: "vector", size };
  if (isColor(schema)) return { control: "color" };
  if (schema.type === "string") {
    if (kind === "choice") return { control: "choice" };
    if (kind === "asset_ref") return { control: "asset" };
    return { control: "text" };
  }
  return { control: "json" };
};
