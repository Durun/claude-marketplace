import { useEffect, useState } from "preact/hooks";
import type { MeterEngine } from "./meterEngine.ts";
import { clamp } from "./levels.ts";

export interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  /** ダブルクリックで戻す値。 */
  reset: number;
  format: (v: number) => string;
  disabled?: boolean;
  title?: string;
  /** 離したとき（change）に 1 回だけ呼ぶ。ドラッグ中は手元の値だけが動く。 */
  onCommit: (v: number) => void;
}

/** ドラッグ中は手元の値だけを動かし、離したときに 1 回だけ確定するスライダー。数字も横に出す。 */
export const Slider = ({ label, value, min, max, step, reset, format, disabled, title, onCommit }: SliderProps) => {
  const [local, setLocal] = useState(value);
  useEffect(() => setLocal(value), [value]);
  const resetNow = () => {
    setLocal(reset);
    if (reset !== value) onCommit(reset);
  };
  return (
    <span class="mm-mixer-slider" title={title}>
      <input
        type="range"
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={local}
        disabled={disabled}
        onInput={(e) => setLocal(Number((e.target as HTMLInputElement).value))}
        onChange={(e) => onCommit(Number((e.target as HTMLInputElement).value))}
        onDblClick={resetNow}
      />
      <output class="mm-mixer-value">{format(local)}</output>
    </span>
  );
};

export interface NumberFieldProps {
  label: string;
  value: number;
  min: number;
  max: number;
  unit: string;
  onCommit: (v: number) => void;
}

/** 範囲に収めて確定する数値の入力欄。数字でない入力は元の値に戻す。 */
export const NumberField = ({ label, value, min, max, unit, onCommit }: NumberFieldProps) => (
  <label class="mm-mixer-number">
    <span class="mm-mixer-number-label">{label}</span>
    <input
      type="number"
      aria-label={`${label} (${unit})`}
      min={min}
      max={max}
      value={value}
      onChange={(e) => {
        const el = e.target as HTMLInputElement;
        const v = Number(el.value);
        if (el.value.trim() === "" || !Number.isFinite(v)) {
          el.value = String(value);
          return;
        }
        onCommit(clamp(v, min, max));
      }}
    />
    <span class="mm-mixer-unit">{unit}</span>
  </label>
);

export interface PeakBarProps {
  engine: MeterEngine;
  /** `m:0`、`m:1`（マスターの L、R）か `t:<layer>`。 */
  id: string;
  label: string;
  thin?: boolean;
}

/** ピークの棒。長さと色は MeterEngine が DOM を直接書く。 */
export const PeakBar = ({ engine, id, label, thin }: PeakBarProps) => (
  <div class="mm-mixer-meter" data-thin={thin ? "true" : undefined} role="img" aria-label={label} ref={(el) => engine.attachBar(id, el)}>
    <div class="mm-mixer-meter-fill" />
    <div class="mm-mixer-meter-hold" hidden />
  </div>
);
