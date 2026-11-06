import { RENDER_TARGET_SCHEMA, type JsonSchema, type Rational, type RenderTarget, type SchemaObject } from "@mm/sdk";
import { formatRational } from "./format.ts";

/** 入力欄の種類。JSON Schema の形で決め、名前で決めるのは参照と出力先とコーデックだけ。 */
export type Control =
  | { kind: "text" }
  | { kind: "integer"; min?: number; max?: number }
  | { kind: "number" }
  | { kind: "bool" }
  | { kind: "choice"; options: string[] }
  | { kind: "rational"; min?: number; max?: number }
  | { kind: "json" }
  | { kind: "composition" }
  | { kind: "output" }
  | { kind: "codec" };

/** レンダーターゲットの欄 1 つ。 */
export interface FieldSpec {
  key: string;
  label: string;
  required: boolean;
  control: Control;
  /** スキーマの既定値。レンダーターゲットを新しく作るときの初期値に使う。 */
  fallback?: unknown;
}

/** 下書き。欄の値を、入力欄が持つ文字（真偽の欄だけ真偽）で持つ。 */
export type Draft = Record<string, string | boolean>;

/** 先頭に並べる欄。ここに無い欄（あとから増えたもの）はスキーマの順に続く。 */
const FIELD_ORDER = ["id", "composition", "width", "height", "fps", "codec", "output_path"];
/** 一覧の行が専用の形で見せる欄。残りは「その他」として key と値を並べる。 */
export const CORE_KEYS: readonly string[] = FIELD_ORDER;

const LABELS: Record<string, string> = {
  id: "Name",
  fps: "Frame rate",
  output_path: "Output file",
  loudness_lufs: "Loudness (LUFS)",
};

const NAME_CONTROLS: Record<string, Control> = {
  composition: { kind: "composition" },
  output_path: { kind: "output" },
  codec: { kind: "codec" },
};

const isObject = (s: JsonSchema | undefined): s is SchemaObject => typeof s === "object";

const humanize = (key: string): string => {
  const words = key.replace(/_/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
};

/** `null` を許す書き方（`type: [x, "null"]`、`anyOf` に null）を外して、中身のスキーマを返す。 */
const unwrapNullable = (s: SchemaObject): SchemaObject => {
  if (Array.isArray(s.type)) {
    const rest = s.type.filter((t) => t !== "null");
    return { ...s, type: rest.length === 1 ? rest[0] : rest };
  }
  const variants = (s.anyOf ?? s.oneOf)?.filter((v) => !(isObject(v) && v.type === "null"));
  const only = variants?.length === 1 ? variants[0] : undefined;
  return isObject(only) ? { ...s, ...only, anyOf: undefined, oneOf: undefined } : s;
};

const enumOptions = (s: SchemaObject): string[] | undefined => {
  if (s.enum) return s.enum.map(String);
  const consts = s.oneOf?.map((v) => (isObject(v) ? v.const : undefined));
  return consts?.every((c) => typeof c === "string") ? (consts as string[]) : undefined;
};

const isRational = (s: SchemaObject): boolean =>
  s.type === "object" && isObject(s.properties?.num) && isObject(s.properties?.den);

const controlOf = (key: string, s: SchemaObject): Control => {
  const named = NAME_CONTROLS[key];
  if (named && s.type === "string") return named;
  const options = enumOptions(s);
  if (options) return { kind: "choice", options };
  if (isRational(s)) {
    const num = s.properties?.num;
    return { kind: "rational", min: isObject(num) ? num.minimum : undefined, max: isObject(num) ? num.maximum : undefined };
  }
  if (s.type === "integer") return { kind: "integer", min: s.minimum, max: s.maximum };
  if (s.type === "number") return { kind: "number" };
  if (s.type === "boolean") return { kind: "bool" };
  if (s.type === "string") return { kind: "text" };
  return { kind: "json" };
};

/** レンダーターゲットのスキーマから欄の一覧を作る。欄が増えても、ここは直さずに済む。 */
export const fieldSpecs = (schema: JsonSchema = RENDER_TARGET_SCHEMA): FieldSpec[] => {
  if (!isObject(schema)) return [];
  const required = new Set(schema.required ?? []);
  const entries = Object.entries(schema.properties ?? {}).filter(([, v]) => isObject(v));
  const rank = (key: string): number => {
    const i = FIELD_ORDER.indexOf(key);
    return i < 0 ? FIELD_ORDER.length : i;
  };
  return entries
    .map(([key, v], order) => ({ key, v: v as SchemaObject, order }))
    .sort((a, b) => rank(a.key) - rank(b.key) || a.order - b.order)
    .map(({ key, v }) => {
      const inner = unwrapNullable(v);
      return {
        key,
        label: LABELS[key] ?? humanize(key),
        required: required.has(key),
        control: controlOf(key, inner),
        fallback: v.default,
      };
    });
};

const toText = (spec: FieldSpec, value: unknown): string | boolean => {
  if (value === undefined || value === null) return spec.control.kind === "bool" ? false : "";
  if (spec.control.kind === "bool") return value === true;
  if (spec.control.kind === "rational") return formatRational(value as Rational);
  if (spec.control.kind === "json") return JSON.stringify(value);
  return String(value);
};

/** レンダーターゲットから下書きを作る。 */
export const draftOf = (target: Partial<RenderTarget>, specs: FieldSpec[]): Draft =>
  Object.fromEntries(specs.map((s) => [s.key, toText(s, (target as Record<string, unknown>)[s.key])]));

/** 1 つの欄の下書きから値を取る。`undefined` は「無い」（省ける欄）、`Error` は入力の誤り。 */
const parseField = (spec: FieldSpec, text: string | boolean): unknown => {
  const { control } = spec;
  if (control.kind === "bool") return text === true;
  const raw = String(text).trim();
  if (raw === "") return spec.required ? new Error("Required") : undefined;
  return parseValue(control, raw);
};

const GCD_SEED = 0;
const gcd = (a: number, b: number): number => (b === GCD_SEED ? a : gcd(b, a % b));
const DECIMAL_SCALE = 1000;

/** `30`、`30000/1001`、`29.97` を有理数にする。 */
export const parseRational = (raw: string): Rational | null => {
  const frac = /^(\d+)\s*\/\s*(\d+)$/.exec(raw);
  if (frac) return { num: Number(frac[1]), den: Number(frac[2]) };
  if (!/^\d+(\.\d+)?$/.test(raw)) return null;
  const num = Math.round(Number(raw) * DECIMAL_SCALE);
  const d = gcd(num, DECIMAL_SCALE);
  return { num: num / d, den: DECIMAL_SCALE / d };
};

const within = (n: number, min?: number, max?: number): boolean =>
  (min === undefined || n >= min) && (max === undefined || n <= max);

const parseValue = (control: Control, raw: string): unknown => {
  switch (control.kind) {
    case "integer": {
      const n = Number(raw);
      if (!/^-?\d+$/.test(raw)) return new Error("Enter a whole number");
      return within(n, control.min, control.max) ? n : new Error(`Out of range${control.min === undefined ? "" : ` (min ${control.min})`}`);
    }
    case "number":
      return Number.isFinite(Number(raw)) ? Number(raw) : new Error("Enter a number");
    case "rational": {
      const r = parseRational(raw);
      const ok = r !== null && [r.num, r.den].every((n) => n > 0 && within(n, control.min, control.max));
      return ok ? r : new Error("Enter a rate like 30 or 30000/1001");
    }
    case "json":
      try {
        return JSON.parse(raw) as unknown;
      } catch {
        return new Error("Enter valid JSON");
      }
    default:
      return raw;
  }
};

export type Parsed = { ok: true; value: RenderTarget } | { ok: false; errors: Record<string, string> };

/** 下書きからレンダーターゲットを作る。スキーマに無い欄は `base` のまま残す。 */
export const targetOf = (draft: Draft, specs: FieldSpec[], base: Partial<RenderTarget> = {}): Parsed => {
  const values: [string, unknown][] = [];
  const errors: Record<string, string> = {};
  for (const spec of specs) {
    const parsed = parseField(spec, draft[spec.key] ?? "");
    if (parsed instanceof Error) errors[spec.key] = parsed.message;
    else values.push([spec.key, parsed]);
  }
  if (Object.keys(errors).length > 0) return { ok: false, errors };
  // 省いた欄（undefined）は base の値も含めて外し、スキーマに無い欄は base のまま残す。
  const merged = { ...base, ...Object.fromEntries(values) } as Record<string, unknown>;
  const kept = Object.entries(merged).filter(([, v]) => v !== undefined);
  return { ok: true, value: Object.fromEntries(kept) as unknown as RenderTarget };
};

const zeroOf = (control: Control): unknown => {
  switch (control.kind) {
    case "integer":
      return control.min ?? 0;
    case "number":
      return 0;
    case "bool":
      return false;
    case "choice":
      return control.options[0] ?? "";
    case "rational":
      return { num: 1, den: 1 };
    case "json":
      return null;
    default:
      return "";
  }
};

/** 必須の欄のうち `base` に無いものを、スキーマの既定値か型の初期値で埋める。 */
export const withRequired = (base: Record<string, unknown>, specs: FieldSpec[]): RenderTarget => {
  const filled = { ...base };
  for (const spec of specs) {
    if (spec.required && filled[spec.key] === undefined) filled[spec.key] = spec.fallback ?? zeroOf(spec.control);
  }
  return filled as unknown as RenderTarget;
};

/** 一覧の行の「その他」に出す欄。専用の形で見せる欄を除き、値のある欄だけ。 */
export const extraFields = (target: RenderTarget, specs: FieldSpec[]): { label: string; text: string }[] =>
  specs
    .filter((s) => !CORE_KEYS.includes(s.key))
    .flatMap((s) => {
      const value = (target as Record<string, unknown>)[s.key];
      if (value === undefined || value === null) return [];
      const text = s.control.kind === "rational" ? formatRational(value as Rational) : typeof value === "object" ? JSON.stringify(value) : String(value);
      return [{ label: s.label, text }];
    });
