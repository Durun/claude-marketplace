import type { MixerTrack, Op } from "@mm/sdk";
import { PeakBar, Slider } from "./Controls.tsx";
import { FADER_MAX_DB, FADER_MIN_DB, FADER_STEP_DB, formatDb, formatPan, PAN_STEP } from "./levels.ts";
import type { MeterEngine } from "./meterEngine.ts";
import { audioOf, paramNumber, trackGainOp, trackOp, trackPanOp } from "./ops.ts";

export interface TrackRowProps {
  track: MixerTrack;
  roles: string[];
  engine: MeterEngine;
  apply: (ops: Op[]) => void;
}

const ANIMATED_TITLE = "Animated with keyframes. Edit it in the Timeline.";

const ToggleButton = ({ kind, label, on, onClick }: { kind: "mute" | "solo"; label: string; on: boolean; onClick: () => void }) => (
  <button type="button" class="mm-mixer-toggle" aria-label={label} aria-pressed={on} data-kind={kind} onClick={onClick}>
    {kind === "mute" ? "M" : "S"}
  </button>
);

const TrackHead = ({ track, roles, apply }: Omit<TrackRowProps, "engine">) => {
  const audio = audioOf(track);
  const roleOptions = roles.includes(audio.role) ? roles : [...roles, audio.role];
  return (
    <div class="mm-mixer-track-head">
      <span class="mm-mixer-name" title={`${track.label} (${track.kind})`}>
        {track.label}
      </span>
      <ToggleButton kind="mute" label={`Mute ${track.label}`} on={audio.mute === true} onClick={() => apply([trackOp(track, { mute: audio.mute !== true })])} />
      <ToggleButton kind="solo" label={`Solo ${track.label}`} on={audio.solo === true} onClick={() => apply([trackOp(track, { solo: audio.solo !== true })])} />
      <select aria-label={`Bus for ${track.label}`} value={audio.role} onChange={(e) => apply([trackOp(track, { role: (e.target as HTMLSelectElement).value })])}>
        {roleOptions.map((r) => (
          <option key={r} value={r}>
            {r}
          </option>
        ))}
      </select>
    </div>
  );
};

const TrackControls = ({ track, apply }: Pick<TrackRowProps, "track" | "apply">) => {
  const audio = audioOf(track);
  const title = track.animated ? ANIMATED_TITLE : undefined;
  return (
    <div class="mm-mixer-track-controls">
      <Slider
        label={`Volume of ${track.label}`}
        value={paramNumber(audio.gain_db)}
        min={FADER_MIN_DB}
        max={FADER_MAX_DB}
        step={FADER_STEP_DB}
        reset={0}
        format={(v) => `${formatDb(v)} dB`}
        disabled={track.animated}
        title={title}
        onCommit={(v) => apply([trackGainOp(track, v)])}
      />
      <Slider
        label={`Pan of ${track.label}`}
        value={paramNumber(audio.pan)}
        min={-1}
        max={1}
        step={PAN_STEP}
        reset={0}
        format={formatPan}
        disabled={track.animated}
        title={title}
        onCommit={(v) => apply([trackPanOp(track, v)])}
      />
    </div>
  );
};

/** トラック 1 本。ミュート、ソロ、送り先のバス、音量、パン、ピークのメーター。 */
export const TrackRow = ({ track, roles, engine, apply }: TrackRowProps) => (
  <li class="mm-mixer-track" aria-label={track.label}>
    <TrackHead track={track} roles={roles} apply={apply} />
    <PeakBar engine={engine} id={`t:${track.layer}`} label={`${track.label} peak`} thin />
    <TrackControls track={track} apply={apply} />
  </li>
);
