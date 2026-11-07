import type { Ducking } from "@mm/sdk";
import type { MixerState } from "./api.ts";
import { NumberField, PeakBar, Slider } from "./Controls.tsx";
import { FADER_MAX_DB, FADER_MIN_DB, FADER_STEP_DB, formatDb, METER_FLOOR_DB, METER_HOT_DB, METER_WARN_DB } from "./levels.ts";
import type { MeterEngine, ReadoutKey } from "./meterEngine.ts";
import { busGain, busGainOp, DEFAULT_DUCKING, duckingOps, mixerOp, type BuildOps } from "./ops.ts";

export interface SectionProps {
  state: MixerState;
  apply: (build: BuildOps) => void;
}

/** 目盛りの数字（dBFS）。メーターの下に置く。 */
const SCALE_DB = [METER_FLOOR_DB, METER_WARN_DB, METER_HOT_DB, 0];
const SCALE_SPAN_DB = -METER_FLOOR_DB;
const PERCENT = 100;
const DUCK_REDUCTION_MIN = -60;
const DUCK_THRESHOLD_MIN = -80;
const DUCK_TIME_MAX_MS = 5000;

export const dbSlider = (label: string, value: number, onCommit: (v: number) => void) => (
  <Slider
    label={label}
    value={value}
    min={FADER_MIN_DB}
    max={FADER_MAX_DB}
    step={FADER_STEP_DB}
    reset={0}
    format={(v) => `${formatDb(v)} dB`}
    onCommit={onCommit}
  />
);

const READOUT_ABBR: Record<ReadoutKey, string> = { true_peak: "TP", short_term: "S", integrated: "I" };

const Readout = ({ engine, id, label, unit }: { engine: MeterEngine; id: ReadoutKey; label: string; unit: string }) => (
  <span class="mm-mixer-readout" title={`${label} (${unit})`}>
    <abbr title={label}>{READOUT_ABBR[id]}</abbr>{" "}
    <b ref={(el) => engine.attachReadout(id, el)}>--</b> <span class="mm-mixer-unit">{unit}</span>
  </span>
);

const Scale = () => (
  <div class="mm-mixer-scale" aria-hidden="true">
    {SCALE_DB.map((db) => (
      <span key={db} style={{ left: `${((db + SCALE_SPAN_DB) / SCALE_SPAN_DB) * PERCENT}%` }}>
        {db}
      </span>
    ))}
  </div>
);

/** マスターのメーター（L、R のピーク、TP、短期、統合）とフェーダー。 */
export const MasterSection = ({ state, apply, engine }: SectionProps & { engine: MeterEngine }) => (
  <section class="mm-mixer-section" aria-label="Master">
    <h3>Master</h3>
    <div class="mm-mixer-master-meters">
      <span class="mm-mixer-channel">L</span>
      <PeakBar engine={engine} id="m:0" label="Master L peak" />
      <span class="mm-mixer-channel">R</span>
      <PeakBar engine={engine} id="m:1" label="Master R peak" />
    </div>
    <Scale />
    <div class="mm-mixer-readouts">
      <Readout engine={engine} id="true_peak" label="True peak" unit="dBTP" />
      <Readout engine={engine} id="short_term" label="Short-term loudness" unit="LUFS" />
      <Readout engine={engine} id="integrated" label="Integrated loudness" unit="LUFS" />
    </div>
    {dbSlider("Master volume", state.mixer?.master_db ?? 0, (v) => apply((s) => [mixerOp(s, { master_db: v })]))}
  </section>
);

/** バスごとの音量。 */
export const BusSection = ({ state, apply }: SectionProps) => (
  <section class="mm-mixer-section" aria-label="Buses">
    <h3>Buses</h3>
    {state.roles.map((role) => (
      <div class="mm-mixer-line" key={role}>
        <span class="mm-mixer-name">{role}</span>
        {dbSlider(`${role} bus volume`, busGain(state, role), (v) => apply((s) => [busGainOp(s, role, v)]))}
      </div>
    ))}
  </section>
);

const DuckingFields = ({ state, apply, ducking }: SectionProps & { ducking: Ducking }) => {
  const set = (patch: Partial<Ducking>) => apply(duckingOps(() => patch));
  const toggleTarget = (role: string, on: boolean) =>
    apply(duckingOps((d) => ({ targets: on ? [...d.targets.filter((t) => t !== role), role] : d.targets.filter((t) => t !== role) })));
  return (
    <>
      <div class="mm-mixer-line">
        <label class="mm-mixer-name" for="mm-mixer-trigger">
          Trigger
        </label>
        <select
          id="mm-mixer-trigger"
          value={ducking.trigger}
          onChange={(e) => {
            const trigger = (e.target as HTMLSelectElement).value;
            apply(duckingOps((d) => ({ trigger, targets: d.targets.filter((t) => t !== trigger) })));
          }}
        >
          {state.roles.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
      </div>
      <fieldset class="mm-mixer-targets">
        <legend>Duck</legend>
        {state.roles
          .filter((r) => r !== ducking.trigger)
          .map((r) => (
            <label key={r} class="mm-mixer-check">
              <input type="checkbox" checked={ducking.targets.includes(r)} onChange={(e) => toggleTarget(r, (e.target as HTMLInputElement).checked)} />
              {r}
            </label>
          ))}
      </fieldset>
      <div class="mm-mixer-numbers">
        <NumberField label="Reduction" value={ducking.reduction_db} min={DUCK_REDUCTION_MIN} max={0} unit="dB" onCommit={(v) => set({ reduction_db: v })} />
        <NumberField label="Threshold" value={ducking.threshold_db} min={DUCK_THRESHOLD_MIN} max={0} unit="dB" onCommit={(v) => set({ threshold_db: v })} />
        <NumberField label="Attack" value={ducking.attack_ms} min={0} max={DUCK_TIME_MAX_MS} unit="ms" onCommit={(v) => set({ attack_ms: v })} />
        <NumberField label="Release" value={ducking.release_ms} min={0} max={DUCK_TIME_MAX_MS} unit="ms" onCommit={(v) => set({ release_ms: v })} />
      </div>
    </>
  );
};

/** ダッキング。有効にすると、きっかけのバスが鳴る間、対象のバスを下げる。 */
export const DuckingSection = ({ state, apply }: SectionProps) => {
  const ducking = state.mixer?.ducking ?? null;
  return (
    <section class="mm-mixer-section" aria-label="Ducking">
      <h3>Ducking</h3>
      <label class="mm-mixer-check">
        <input
          type="checkbox"
          checked={ducking !== null}
          onChange={(e) => {
            const on = (e.target as HTMLInputElement).checked;
            apply((s) => [mixerOp(s, { ducking: on ? (s.mixer?.ducking ?? { ...DEFAULT_DUCKING }) : null })]);
          }}
        />
        Enabled
      </label>
      {ducking && <DuckingFields state={state} apply={apply} ducking={ducking} />}
    </section>
  );
};
