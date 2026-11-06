/** JSON Schema の部分集合（schemars が mm-ops から出す形）。 */
export type JsonSchema = boolean | SchemaObject;

export interface SchemaObject {
  $ref?: string;
  type?: string | string[];
  const?: unknown;
  enum?: unknown[];
  oneOf?: JsonSchema[];
  anyOf?: JsonSchema[];
  properties?: Record<string, JsonSchema>;
  required?: string[];
  additionalProperties?: JsonSchema;
  items?: JsonSchema;
  description?: string;
  format?: string;
  minimum?: number;
  maximum?: number;
  minItems?: number;
  maxItems?: number;
  default?: unknown;
}

/** ops.json の全体。`oneOf` の 1 つが 1 つの操作で、`op` の const が名前。 */
export interface OpsSchema extends SchemaObject {
  $defs: Record<string, JsonSchema>;
}

const REF_PREFIX = "#/$defs/";
const INDENT = "  ";
const JSON_INDENT = INDENT.length;

const refName = (ref: string): string => {
  if (!ref.startsWith(REF_PREFIX)) {
    throw new Error(`未対応の $ref: ${ref}`);
  }
  return ref.slice(REF_PREFIX.length);
};

const PRIMITIVES: Record<string, string> = {
  string: "string",
  integer: "number",
  number: "number",
  boolean: "boolean",
  null: "null",
};

const union = (parts: string[]): string => [...new Set(parts)].join(" | ");

const oneLine = (text: string): string => text.replace(/\s*\n\s*/g, " ").replace(/\*\//g, "* /");

const wrapUnion = (t: string): string => (t.includes(" | ") ? `(${t})` : t);

const capitalize = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

const docOf = (s: JsonSchema): string => (typeof s === "object" && s.description ? oneLine(s.description) : "");

const typeOfObject = (s: SchemaObject, depth: number): string => {
  const props = Object.entries(s.properties ?? {});
  if (props.length === 0) {
    const extra = s.additionalProperties;
    return extra === undefined || extra === false
      ? "Record<string, never>"
      : `Record<string, ${tsType(extra, depth)}>`;
  }
  const required = new Set(s.required ?? []);
  const pad = INDENT.repeat(depth + 1);
  const lines = props.map(([k, v]) => {
    const doc = docOf(v) ? `${pad}/** ${docOf(v)} */\n` : "";
    return `${doc}${pad}${JSON.stringify(k)}${required.has(k) ? "" : "?"}: ${tsType(v, depth + 1)};`;
  });
  return `{\n${lines.join("\n")}\n${INDENT.repeat(depth)}}`;
};

const typeOfType = (t: string, s: SchemaObject, depth: number): string => {
  if (t === "object") return typeOfObject(s, depth);
  if (t === "array") return `${wrapUnion(s.items === undefined ? "unknown" : tsType(s.items, depth))}[]`;
  const prim = PRIMITIVES[t];
  if (!prim) throw new Error(`未対応の type: ${t}`);
  return prim;
};

/** JSON Schema を TypeScript の型の式にする。 */
export const tsType = (schema: JsonSchema, depth = 0): string => {
  if (schema === true) return "unknown";
  if (schema === false) return "never";
  if (schema.$ref) return refName(schema.$ref);
  if (schema.const !== undefined) return JSON.stringify(schema.const);
  if (schema.enum) return union(schema.enum.map((v) => JSON.stringify(v)));
  const variants = schema.oneOf ?? schema.anyOf;
  if (variants) return union(variants.map((v) => tsType(v, depth)));
  const types = Array.isArray(schema.type) ? schema.type : schema.type ? [schema.type] : [];
  return union(types.map((t) => typeOfType(t, schema, depth)));
};

/** 操作 1 つの名前（`op` の const）。 */
export const opName = (variant: JsonSchema): string => {
  const op = typeof variant === "object" ? variant.properties?.op : undefined;
  const name = typeof op === "object" ? op.const : undefined;
  if (typeof name !== "string") throw new Error("操作の oneOf に op の const が無い");
  return name;
};

/** `$ref` をたどって、参照の無い形にする（パラメータの値の型の抜き出し用）。 */
export const inlineRefs = (schema: JsonSchema, defs: OpsSchema["$defs"]): JsonSchema => {
  if (typeof schema !== "object") return schema;
  if (schema.$ref) return inlineRefs(defs[refName(schema.$ref)] ?? false, defs);
  const out: SchemaObject = { ...schema };
  if (schema.properties) {
    out.properties = Object.fromEntries(
      Object.entries(schema.properties).map(([k, v]) => [k, inlineRefs(v, defs)]),
    );
  }
  if (schema.items !== undefined) out.items = inlineRefs(schema.items, defs);
  for (const key of ["oneOf", "anyOf"] as const) {
    const list = schema[key];
    if (list) out[key] = list.map((v) => inlineRefs(v, defs));
  }
  return out;
};

/** ParamValue の `type` ごとの `value` のスキーマ。Inspector が入力欄の種類を決めるのに使う。 */
export const paramValueSchemas = (schema: OpsSchema): Record<string, JsonSchema> => {
  const param = schema.$defs.ParamValue;
  if (typeof param !== "object" || !param.oneOf) throw new Error("ParamValue が oneOf でない");
  const entries = param.oneOf.map((variant): [string, JsonSchema] => {
    const props = typeof variant === "object" ? variant.properties : undefined;
    const tag = props?.type;
    const kind = typeof tag === "object" ? tag.const : undefined;
    if (typeof kind !== "string" || !props?.value) throw new Error("ParamValue の型に type か value が無い");
    return [kind, inlineRefs(props.value, schema.$defs)];
  });
  return Object.fromEntries(entries);
};

/** レンダーターゲットの JSON Schema（`$ref` を解いたもの）。Deliver が入力欄を作る。 */
export const renderTargetSchema = (schema: OpsSchema): JsonSchema => {
  const target = schema.$defs.RenderTarget;
  if (target === undefined) throw new Error("ops.json に RenderTarget が無い");
  return inlineRefs(target, schema.$defs);
};

const opType = (v: JsonSchema): string => {
  const doc = docOf(v) ? `/** ${docOf(v)} */\n` : "";
  return `${doc}export type ${capitalize(opName(v))}Op = ${tsType(v)};`;
};

/** ops.json から `generated/ops.ts` の中身を作る。同じ入力なら同じ出力。 */
export const generateOpsTs = (schema: OpsSchema): string => {
  const defs = Object.entries(schema.$defs).map(([name, def]) => {
    const doc = docOf(def) ? `/** ${docOf(def)} */\n` : "";
    return `${doc}export type ${name} = ${tsType(def)};`;
  });
  const variants = schema.oneOf ?? [];
  const names = variants.map(opName);
  return [
    "// ops.json から tools/gen-sdk.ts が生成する。直接は書き換えない。",
    'import type { JsonSchema } from "../codegen.ts";',
    "",
    ...defs,
    "",
    ...variants.map(opType),
    "",
    "/** 全部の操作。`op` が名前。 */",
    `export type Op =\n${names.map((n) => `${INDENT}| ${capitalize(n)}Op`).join("\n")};`,
    "",
    "/** 操作の名前の和。 */",
    'export type OpName = Op["op"];',
    "",
    "/** 名前から操作を引く。 */",
    "export type OpOf<N extends OpName> = Extract<Op, { op: N }>;",
    "",
    "/** 名前から、`op` を除いた引数の型を引く。 */",
    'export type OpArgs<N extends OpName> = Omit<OpOf<N>, "op">;',
    "",
    "/** 全部の操作の名前。 */",
    `export const OP_NAMES: readonly OpName[] = ${JSON.stringify(names, null, JSON_INDENT)};`,
    "",
    "/** パラメータの値の種類（`ParamValue.type`）ごとの、`value` の JSON Schema。 */",
    `export const PARAM_VALUE_SCHEMAS: Record<ParamValue["type"], JsonSchema> = ${JSON.stringify(paramValueSchemas(schema), null, JSON_INDENT)};`,
    "",
    "/** レンダーターゲットの JSON Schema。欄が増えても Deliver は入力欄をここから作る。 */",
    `export const RENDER_TARGET_SCHEMA: JsonSchema = ${JSON.stringify(renderTargetSchema(schema), null, JSON_INDENT)};`,
    "",
  ].join("\n");
};
