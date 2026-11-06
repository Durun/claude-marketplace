import { RENDER_TARGET_SCHEMA, type JsonSchema, type Rational, type RenderTarget, type SchemaObject } from "@mm/sdk";
import { formatRational } from "./format.ts";

/** 入力欄の種類。JSON Schema の形で決め、名前で決めるのは参照と出力先とコーデックだけ。 */
export type Control =
  | { kind: "text" }
  | { kind: "integer"; min?: number; max?: number; even?: boolean }
  | { kind: "number" }
  | { kind: "bool" }
  | { kind: "choice"; options: string[] }
  | { kind: "rational"; min?: number; max?: number; denMin?: number; denMax?: number }
  | { kind: "json" }
  | { kind: "composition" }
  | { kind: "output" }
  | { kind: "codec" };

/** レンダーターゲットの欄 1 つ。入れ子の `delivery` の子は `delivery.subtitles` のように点でつなぐ。 */
export interface FieldSpec {
  key: string;
  /** 値のある場所。`key` を点で分けたもの。 */
  path: string[];
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
  "delivery.true_peak_dbtp": "Delivery: True peak (dBTP)",
};

/** 幅と高さ。0 は書けず、エンコーダー（4:2:0 と 4:2:2）は偶数を要る。 */
const SIZE_KEYS = ["width", "height"];
const SIZE_MIN = 1;
const EVEN_DIVISOR = 2;

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
    const den = s.properties?.den;
    return {
      kind: "rational",
      min: isObject(num) ? num.minimum : undefined,
      max: isObject(num) ? num.maximum : undefined,
      denMin: isObject(den) ? den.minimum : undefined,
      denMax: isObject(den) ? den.maximum : undefined,
    };
  }
  if (s.type === "integer" && SIZE_KEYS.includes(key)) return { kind: "integer", min: Math.max(SIZE_MIN, s.minimum ?? SIZE_MIN), max: s.maximum, even: true };
  if (s.type === "integer") return { kind: "integer", min: s.minimum, max: s.maximum };
  if (s.type === "number") return { kind: "number" };
  if (s.type === "boolean") return { kind: "bool" };
  if (s.type === "string") return { kind: "text" };
  return { kind: "json" };
};

/** 子の欄を持つ入れ子の object（有理数と、子の無い辞書は欄 1 つのまま）。 */
const isNested = (s: SchemaObject): boolean =>
  s.type === "object" && !isRational(s) && Object.values(s.properties ?? {}).some(isObject);

/** `schema` の子を欄にする。入れ子の object は再帰して子の欄にし、親が任意なら子の必須も任意として扱う。 */
const specsOf = (entries: [string, SchemaObject][], required: Set<string>, parent: string[], parentRequired: boolean): FieldSpec[] =>
  entries.flatMap(([key, v]) => {
    const inner = unwrapNullable(v);
    const path = [...parent, key];
    const isRequired = parentRequired && required.has(key);
    if (isNested(inner)) {
      const children = Object.entries(inner.properties ?? {}).filter((e): e is [string, SchemaObject] => isObject(e[1]));
      return specsOf(children, new Set(inner.required ?? []), path, isRequired);
    }
    const dotted = path.join(".");
    return [
      {
        key: dotted,
        path,
        label: LABELS[dotted] ?? path.map(humanize).join(": "),
        required: isRequired,
        control: controlOf(dotted, inner),
        fallback: v.default,
      },
    ];
  });

/** レンダーターゲットのスキーマから欄の一覧を作る。欄が増えても、ここは直さずに済む。 */
export const fieldSpecs = (schema: JsonSchema = RENDER_TARGET_SCHEMA): FieldSpec[] => {
  if (!isObject(schema)) return [];
  const rank = (key: string): number => {
    const i = FIELD_ORDER.indexOf(key);
    return i < 0 ? FIELD_ORDER.length : i;
  };
  const entries = Object.entries(schema.properties ?? {})
    .filter((e): e is [string, SchemaObject] => isObject(e[1]))
    .map((e, order) => ({ e, order }))
    .sort((a, b) => rank(a.e[0]) - rank(b.e[0]) || a.order - b.order)
    .map(({ e }) => e);
  return specsOf(entries, new Set(schema.required ?? []), [], true);
};

type Rec = Record<string, unknown>;
const isRec = (v: unknown): v is Rec => typeof v === "object" && v !== null && !Array.isArray(v);

/** 入れ子の値を道筋で読む。 */
const getAt = (obj: Rec, path: string[]): unknown => path.reduce<unknown>((cur, k) => (isRec(cur) ? cur[k] : undefined), obj);

/** 入れ子の値を道筋で置いた複製（元は変えない）。 */
const setAt = (obj: Rec, path: string[], value: unknown): Rec => {
  const [head, ...rest] = path;
  if (head === undefined) return obj;
  if (rest.length === 0) return { ...obj, [head]: value };
  const child = obj[head];
  return { ...obj, [head]: setAt(isRec(child) ? child : {}, rest, value) };
};

/** 道筋の値を外した複製。外して空になった入れ子は、`base` に無かったなら残さない。 */
const removeAt = (obj: Rec, path: string[], base: Rec): Rec => {
  const [head, ...rest] = path;
  if (head === undefined) return obj;
  const { [head]: child, ...others } = obj;
  if (rest.length === 0) return others;
  const kept = removeAt(isRec(child) ? child : {}, rest, isRec(base[head]) ? base[head] : {});
  return Object.keys(kept).length === 0 && base[head] === undefined ? others : { ...others, [head]: kept };
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
  Object.fromEntries(specs.map((s) => [s.key, toText(s, getAt(target as Rec, s.path))]));

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
      if (!within(n, control.min, control.max)) return new Error(`Out of range${control.min === undefined ? "" : ` (min ${control.min})`}`);
      return control.even && n % EVEN_DIVISOR !== 0 ? new Error("Enter an even number (video encoders need it)") : n;
    }
    case "number":
      return Number.isFinite(Number(raw)) ? Number(raw) : new Error("Enter a number");
    case "rational": {
      const r = parseRational(raw);
      const ok = r !== null && r.num > 0 && r.den > 0 && within(r.num, control.min, control.max) && within(r.den, control.denMin, control.denMax);
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

/** ProRes と DNxHR は MOV に書く（アプリの書き出しも `.mov` 以外を断る）。 */
const MOV_CODEC_PREFIXES = ["prores", "dnx"];

/** 欄をまたぐ決まり。コーデックと出力先の拡張子の組。 */
const crossErrors = (values: Rec): Record<string, string> => {
  const codec = String(values.codec ?? "").toLowerCase();
  const output = String(values.output_path ?? "");
  const needsMov = MOV_CODEC_PREFIXES.some((p) => codec.startsWith(p));
  return needsMov && output !== "" && !output.toLowerCase().endsWith(".mov") ? { output_path: "ProRes and DNxHR need a .mov file" } : {};
};

/** 省ける真偽の欄が、元に無く偽のままなら「無い」にする（開いて保存しただけで既定の欄が増えないように）。 */
const omitDefaultBool = (spec: FieldSpec, value: unknown, base: Rec): unknown =>
  spec.control.kind === "bool" && !spec.required && value === false && getAt(base, spec.path) === undefined ? undefined : value;

/** 下書きからレンダーターゲットを作る。スキーマに無い欄は `base` のまま残す。 */
export const targetOf = (draft: Draft, specs: FieldSpec[], base: Partial<RenderTarget> = {}): Parsed => {
  const baseRec = base as Rec;
  const values: [FieldSpec, unknown][] = [];
  const errors: Record<string, string> = {};
  for (const spec of specs) {
    const parsed = parseField(spec, draft[spec.key] ?? "");
    if (parsed instanceof Error) errors[spec.key] = parsed.message;
    else values.push([spec, omitDefaultBool(spec, parsed, baseRec)]);
  }
  const flat = Object.fromEntries(values.map(([s, v]) => [s.key, v])) as Rec;
  for (const [key, message] of Object.entries(crossErrors(flat))) errors[key] ??= message;
  if (Object.keys(errors).length > 0) return { ok: false, errors };
  // 省いた欄（undefined）は base の値も含めて外し、スキーマに無い欄は base のまま残す。
  const merged = values.reduce<Rec>((acc, [spec, v]) => (v === undefined ? removeAt(acc, spec.path, baseRec) : setAt(acc, spec.path, v)), { ...baseRec });
  return { ok: true, value: merged as unknown as RenderTarget };
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
  let filled: Rec = { ...base };
  for (const spec of specs) {
    if (spec.required && getAt(filled, spec.path) === undefined) filled = setAt(filled, spec.path, spec.fallback ?? zeroOf(spec.control));
  }
  return filled as unknown as RenderTarget;
};

/** 一覧の行の「その他」に出す欄。専用の形で見せる欄と、既定値のままの欄を除き、値のある欄だけ。 */
export const extraFields = (target: RenderTarget, specs: FieldSpec[]): { label: string; text: string }[] =>
  specs
    .filter((s) => !CORE_KEYS.includes(s.key))
    .flatMap((s) => {
      const value = getAt(target as Rec, s.path);
      if (value === undefined || value === null || JSON.stringify(value) === JSON.stringify(s.fallback)) return [];
      const text = s.control.kind === "rational" ? formatRational(value as Rational) : typeof value === "object" ? JSON.stringify(value) : String(value);
      return [{ label: s.label, text }];
    });
