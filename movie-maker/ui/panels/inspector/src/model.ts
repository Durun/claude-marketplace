import type { Effect, Layer, Op, Param, ParamValue } from "@mm/sdk";

/** 1 つのパラメータの入力欄に付ける補足。JSON Schema に無い範囲や選択肢を渡す。 */
export interface ParamHint {
  label?: string;
  min?: number;
  max?: number;
  step?: number;
  /** `choice` の選択肢。 */
  choices?: readonly string[];
}

/** パラメータのパス（`setParam` の target.path）から補足を引く。 */
export type ParamHints = Readonly<Record<string, ParamHint>>;

const OPACITY_STEP = 0.01;

/** 型だけでは決まらない範囲。 */
export const DEFAULT_HINTS: ParamHints = {
  opacity: { min: 0, max: 1, step: OPACITY_STEP },
  "audio.pan": { min: -1, max: 1, step: OPACITY_STEP },
};

export interface FieldRow {
  /** `setParam` の target.path。 */
  path: string;
  label: string;
  param: Param;
  hint: ParamHint;
}

export interface Section {
  id: string;
  title: string;
  rows: FieldRow[];
}

/** キーフレームがあるか。ある間は `value` を使わない（mm-eval）。 */
export const hasKeyframes = (p: Param): boolean => (p.keyframes?.length ?? 0) > 0;

/** 入力欄に出す値。キーフレームがあれば最初のキーフレームの値。 */
export const shownValue = (p: Param): ParamValue => p.keyframes?.[0]?.value ?? p.value;

const DEFAULT_TRANSFORM: Record<string, ParamValue> = {
  position: { type: "vec2", value: [0, 0] },
  scale: { type: "number", value: 1 },
  rotation: { type: "number", value: 0 },
};

const row = (path: string, label: string, param: Param, hints: ParamHints): FieldRow => ({
  path,
  label: hints[path]?.label ?? label,
  param,
  hint: hints[path] ?? {},
});

const transformSection = (layer: Layer, hints: ParamHints): Section => {
  const rows = Object.entries(DEFAULT_TRANSFORM).map(([name, value]) =>
    row(`transform.${name}`, name, layer.transform?.[name as "position"] ?? { value }, hints),
  );
  const anchor = layer.transform?.anchor;
  if (anchor) rows.push(row("transform.anchor", "anchor", anchor, hints));
  return { id: "transform", title: "Transform", rows };
};

const paramRows = (prefix: string, params: Record<string, Param> | undefined, hints: ParamHints): FieldRow[] =>
  Object.entries(params ?? {})
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, p]) => row(`${prefix}.${name}`, name, p, hints));

const effectSection = (e: Effect, hints: ParamHints): Section => ({
  id: `effect-${e.id}`,
  title: `Effect: ${e.node.kind}`,
  rows: paramRows(`effects.${e.id}`, e.node.params, hints),
});

/** 選んだレイヤーを、Inspector に出す区画の並びにする。 */
export const describeLayer = (layer: Layer, hints: ParamHints = DEFAULT_HINTS): Section[] => {
  const sections: Section[] = [
    { id: "layer", title: "Layer", rows: [row("opacity", "opacity", layer.opacity, hints)] },
    transformSection(layer, hints),
    { id: "source", title: `Source: ${layer.source.kind}`, rows: paramRows("source", layer.source.params, hints) },
  ];
  if (layer.audio) {
    sections.push({
      id: "audio",
      title: `Audio: ${layer.audio.role}`,
      rows: [
        row("audio.gain_db", "gain_db", layer.audio.gain_db, hints),
        row("audio.pan", "pan", layer.audio.pan, hints),
      ],
    });
  }
  return [...sections, ...(layer.effects ?? []).map((e) => effectSection(e, hints))].filter(
    (s) => s.rows.length > 0,
  );
};

/** 値を変えたときに送る操作。固定の値だけを置き、キーフレームには触れない。 */
export const setParamOp = (layerId: string, path: string, value: ParamValue): Op => ({
  op: "setParam",
  target: { layer: layerId, path },
  value,
});
