import { useState } from "preact/hooks";
import type { RenderTarget } from "@mm/sdk";
import type { DeliverApi, DeliverOverview } from "./api.ts";
import { draftOf, targetOf, type Draft, type FieldSpec } from "./form.ts";

/** 一般的なコーデックの名前。入力欄の候補で、ほかの名前も書ける。 */
const CODEC_SUGGESTIONS = ["h264", "h264_nvenc", "hevc_nvenc", "prores_ks"];
/** よく使うフレームレート。入力欄の候補で、ほかの値も書ける。 */
const FPS_SUGGESTIONS = ["24", "25", "30", "60", "24000/1001", "30000/1001", "60000/1001"];

interface FieldProps {
  spec: FieldSpec;
  value: string | boolean;
  error?: string;
  /** 下書きのコーデック。出力先の保存ダイアログを、合う拡張子に絞る。 */
  codec: string;
  overview: DeliverOverview;
  api: DeliverApi;
  onChange: (value: string | boolean) => void;
}

interface SelectProps {
  id: string;
  value: string;
  options: { value: string; label: string }[];
  /** `aria-invalid` と `aria-describedby`。 */
  extra: Record<string, string | boolean | undefined>;
  onChange: (value: string) => void;
}

const Select = ({ id, value, options, extra, onChange }: SelectProps) => (
  <select id={id} value={value} {...extra} onChange={(e) => onChange((e.target as HTMLSelectElement).value)}>
    {options.map((o) => (
      <option key={o.value} value={o.value}>
        {o.label}
      </option>
    ))}
  </select>
);

const Input = ({ spec, value, error, codec, overview, api, onChange }: FieldProps) => {
  const id = `mm-deliver-field-${spec.key}`;
  const text = String(value);
  const a11y = { "aria-invalid": error !== undefined, "aria-describedby": error === undefined ? undefined : `${id}-error` };
  const common = { id, value: text, onInput: (e: Event) => onChange((e.target as HTMLInputElement).value), ...a11y };
  switch (spec.control.kind) {
    case "bool":
      return <input id={id} type="checkbox" checked={value === true} {...a11y} onChange={(e) => onChange((e.target as HTMLInputElement).checked)} />;
    case "choice": {
      const options = spec.control.options.map((o) => ({ value: o, label: o }));
      return <Select id={id} value={text} options={spec.required ? options : [{ value: "", label: "Default" }, ...options]} extra={a11y} onChange={onChange} />;
    }
    case "composition": {
      const options = overview.compositions.map((c) => ({ value: c.id, label: c.id }));
      const missing = options.some((o) => o.value === text) ? [] : [{ value: text, label: text }];
      return <Select id={id} value={text} options={[...missing, ...options]} extra={a11y} onChange={onChange} />;
    }
    case "output":
      return (
        <span class="mm-deliver-output-input">
          <input {...common} type="text" spellcheck={false} />
          <button type="button" class="mm-deliver-button" onClick={() => void api.pickOutput(text, codec).then((p) => p && onChange(p))}>
            Browse…
          </button>
        </span>
      );
    case "codec":
      return <input {...common} type="text" list="mm-deliver-codecs" spellcheck={false} />;
    case "rational":
      return <input {...common} type="text" list="mm-deliver-fps" inputMode="decimal" spellcheck={false} />;
    case "integer":
    case "number":
      return <input {...common} type="text" inputMode="decimal" spellcheck={false} />;
    default:
      return <input {...common} type="text" spellcheck={false} />;
  }
};

const Field = (props: FieldProps) => (
  <div class="mm-deliver-field" data-invalid={props.error !== undefined}>
    <label for={`mm-deliver-field-${props.spec.key}`}>{props.spec.label}</label>
    <Input {...props} />
    {props.error && (
      <span class="mm-deliver-field-error" id={`mm-deliver-field-${props.spec.key}-error`}>
        {props.error}
      </span>
    )}
  </div>
);

export interface TargetFormProps {
  /** 直すレンダーターゲット。足すときは足す前の初期値。 */
  target: RenderTarget;
  specs: FieldSpec[];
  overview: DeliverOverview;
  api: DeliverApi;
  submitLabel: string;
  onSubmit: (value: RenderTarget) => Promise<boolean>;
  onCancel: () => void;
}

/** レンダーターゲットの入力欄。欄の種類と並びはスキーマから決まる（form.ts）。 */
export const TargetForm = ({ target, specs, overview, api, submitLabel, onSubmit, onCancel }: TargetFormProps) => {
  const [draft, setDraft] = useState<Draft>(() => draftOf(target, specs));
  const [errors, setErrors] = useState<Record<string, string>>({});
  const submit = async (e: Event) => {
    e.preventDefault();
    const parsed = targetOf(draft, specs, target);
    if (!parsed.ok) return setErrors(parsed.errors);
    setErrors({});
    await onSubmit(parsed.value);
  };
  return (
    <form class="mm-deliver-form" aria-label={`Edit ${target.id}`} onSubmit={submit} noValidate>
      {specs.map((spec) => (
        <Field
          key={spec.key}
          spec={spec}
          value={draft[spec.key] ?? ""}
          error={errors[spec.key]}
          codec={String(draft.codec ?? "")}
          overview={overview}
          api={api}
          onChange={(v) => setDraft((d) => ({ ...d, [spec.key]: v }))}
        />
      ))}
      <datalist id="mm-deliver-codecs">
        {CODEC_SUGGESTIONS.map((c) => (
          <option key={c} value={c} />
        ))}
      </datalist>
      <datalist id="mm-deliver-fps">
        {FPS_SUGGESTIONS.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
      <div class="mm-deliver-form-actions">
        <button type="submit" class="mm-deliver-button mm-deliver-primary">
          {submitLabel}
        </button>
        <button type="button" class="mm-deliver-button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
};
