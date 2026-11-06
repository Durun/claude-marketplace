import type { Asset, ParamValue } from "@mm/sdk";
import { hexToRgba, rgbaToHex } from "./color.ts";
import type { FieldSpec } from "./fields.ts";
import type { ParamHint } from "./model.ts";

/** 表示する小数の桁。浮動小数の誤差（0.30000000000000004）を見せない。 */
const DISPLAY_DIGITS = 4;
const DEFAULT_STEP = 0.1;
const VEC2_SIZE = 2;
const UNIT_STEP = 0.01;

export interface EditorProps {
  spec: FieldSpec;
  value: ParamValue;
  hint: ParamHint;
  assets: readonly Asset[];
  disabled: boolean;
  label: string;
  onCommit: (next: ParamValue) => void;
}

const UNIT_HINT: ParamHint = { min: 0, max: 1, step: UNIT_STEP };

const fmt = (n: number): number => Number(n.toFixed(DISPLAY_DIGITS));

interface NumberInputProps {
  value: number;
  hint: ParamHint;
  label: string;
  disabled: boolean;
  onCommit: (n: number) => void;
}

const NumberInput = ({ value, hint, label, disabled, onCommit }: NumberInputProps) => (
  <input
    type="number"
    class="mm-insp-input"
    aria-label={label}
    value={fmt(value)}
    min={hint.min}
    max={hint.max}
    step={hint.step ?? DEFAULT_STEP}
    disabled={disabled}
    onChange={(e) => {
      const n = e.currentTarget.valueAsNumber;
      if (Number.isFinite(n)) onCommit(n);
    }}
  />
);

const COMPONENT_NAMES = ["x", "y", "z"];

const VectorEditor = ({ size, value, hint, label, disabled, onCommit }: EditorProps & { size: number }) => {
  const nums = value.type === "vec2" || value.type === "vec3" ? value.value : [];
  return (
    <div class="mm-insp-vector">
      {Array.from({ length: size }, (_, i) => (
        <NumberInput
          key={i}
          value={nums[i] ?? 0}
          hint={hint}
          label={`${label} ${COMPONENT_NAMES[i] ?? i}`}
          disabled={disabled}
          onCommit={(n) => {
            const next = Array.from({ length: size }, (_, j) => (j === i ? n : (nums[j] ?? 0)));
            onCommit(size === VEC2_SIZE ? { type: "vec2", value: next } : { type: "vec3", value: next });
          }}
        />
      ))}
    </div>
  );
};

const ColorEditor = ({ value, label, disabled, onCommit }: EditorProps) => {
  if (value.type !== "color") return null;
  const c = value.value;
  return (
    <div class="mm-insp-vector">
      <input
        type="color"
        class="mm-insp-color"
        aria-label={label}
        value={rgbaToHex(c)}
        disabled={disabled}
        onChange={(e) => {
          const next = hexToRgba(e.currentTarget.value, c.a);
          if (next) onCommit({ type: "color", value: next });
        }}
      />
      <NumberInput
        value={c.a}
        hint={UNIT_HINT}
        label={`${label} alpha`}
        disabled={disabled}
        onCommit={(a) => {
          const un = c.a > 0 ? a / c.a : 0;
          onCommit({ type: "color", value: { r: c.r * un, g: c.g * un, b: c.b * un, a } });
        }}
      />
    </div>
  );
};

interface SelectProps {
  options: readonly string[];
  current: string;
  label: string;
  disabled: boolean;
  onCommit: (v: string) => void;
}

const Select = ({ options, current, label, disabled, onCommit }: SelectProps) => {
  const all = options.includes(current) ? options : [current, ...options];
  return (
    <select
      class="mm-insp-input"
      aria-label={label}
      value={current}
      disabled={disabled}
      onChange={(e) => onCommit(e.currentTarget.value)}
    >
      {all.map((o) => (
        <option key={o} value={o}>
          {o}
        </option>
      ))}
    </select>
  );
};

const TextEditor = ({ value, label, disabled, onCommit }: EditorProps) => (
  <input
    type="text"
    class="mm-insp-input"
    aria-label={label}
    value={value.type === "text" || value.type === "choice" ? value.value : ""}
    disabled={disabled}
    onChange={(e) => onCommit({ type: value.type === "choice" ? "choice" : "text", value: e.currentTarget.value })}
  />
);

const NumberEditor = ({ value, hint, label, disabled, onCommit }: EditorProps) => (
  <NumberInput
    value={value.type === "number" ? value.value : 0}
    hint={hint}
    label={label}
    disabled={disabled}
    onCommit={(n) => onCommit({ type: "number", value: n })}
  />
);

const BoolEditor = ({ value, label, disabled, onCommit }: EditorProps) => (
  <input
    type="checkbox"
    class="mm-insp-check"
    aria-label={label}
    checked={value.type === "bool" && value.value}
    disabled={disabled}
    onChange={(e) => onCommit({ type: "bool", value: e.currentTarget.checked })}
  />
);

const ChoiceEditor = (props: EditorProps) => {
  const { value, hint, label, disabled, onCommit } = props;
  if (!hint.choices) return <TextEditor {...props} />;
  return (
    <Select
      options={hint.choices}
      current={value.type === "choice" ? value.value : ""}
      label={label}
      disabled={disabled}
      onCommit={(v) => onCommit({ type: "choice", value: v })}
    />
  );
};

const AssetEditor = ({ value, assets, label, disabled, onCommit }: EditorProps) => (
  <Select
    options={assets.map((a) => a.id)}
    current={value.type === "asset_ref" ? value.value : ""}
    label={label}
    disabled={disabled}
    onCommit={(v) => onCommit({ type: "asset_ref", value: v })}
  />
);

/** 値の種類に合った入力欄。値を変えたら onCommit に新しい値を渡す。 */
export const Editor = (props: EditorProps) => {
  const { spec, value } = props;
  switch (spec.control) {
    case "number":
      return <NumberEditor {...props} />;
    case "vector":
      return <VectorEditor {...props} size={spec.size} />;
    case "color":
      return <ColorEditor {...props} />;
    case "bool":
      return <BoolEditor {...props} />;
    case "choice":
      return <ChoiceEditor {...props} />;
    case "asset":
      return <AssetEditor {...props} />;
    case "text":
      return <TextEditor {...props} />;
    case "json":
      return <code class="mm-insp-json">{JSON.stringify(value.value)}</code>;
  }
};
