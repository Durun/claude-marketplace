import { useState } from "preact/hooks";
import type { DeliverApi, DeliverOverview, RenderTarget } from "@mm/sdk";
import { draftOf, targetOf, type Draft, type FieldSpec } from "./form.ts";

/** 一般的なコーデックの名前。入力欄の候補で、ほかの名前も書ける。 */
const CODEC_SUGGESTIONS = ["h264", "h264_nvenc", "hevc_nvenc", "prores_ks"];
/** よく使うフレームレート。入力欄の候補で、ほかの値も書ける。 */
const FPS_SUGGESTIONS = ["24", "25", "30", "60", "24000/1001", "30000/1001", "60000/1001"];

interface FieldProps {
  spec: FieldSpec;
  value: string | boolean;
  error?: string;
  overview: DeliverOverview;
  api: DeliverApi;
  onChange: (value: string | boolean) => void;
}

const Input = ({ spec, value, overview, api, onChange }: FieldProps) => {
  const id = `mm-deliver-field-${spec.key}`;
  const text = String(value);
  const common = { id, value: text, onInput: (e: Event) => onChange((e.target as HTMLInputElement).value) };
  switch (spec.control.kind) {
    case "bool":
      return <input id={id} type="checkbox" checked={value === true} onChange={(e) => onChange((e.target as HTMLInputElement).checked)} />;
    case "choice":
      return (
        <select id={id} value={text} onChange={(e) => onChange((e.target as HTMLSelectElement).value)}>
          {spec.control.options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      );
    case "composition":
      return (
        <select id={id} value={text} onChange={(e) => onChange((e.target as HTMLSelectElement).value)}>
          {!overview.compositions.some((c) => c.id === text) && <option value={text}>{text}</option>}
          {overview.compositions.map((c) => (
            <option key={c.id} value={c.id}>
              {c.id}
            </option>
          ))}
        </select>
      );
    case "output":
      return (
        <span class="mm-deliver-output-input">
          <input {...common} type="text" spellcheck={false} />
          <button type="button" class="mm-deliver-button" onClick={() => void api.pickOutput(text).then((p) => p && onChange(p))}>
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
    {props.error && <span class="mm-deliver-field-error">{props.error}</span>}
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
