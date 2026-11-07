import type { MixerApi } from "./api.ts";
import { BusSection, DuckingSection, MasterSection } from "./Sections.tsx";
import { TrackRow } from "./TrackRow.tsx";
import { useMeters, useMixer } from "./useMixer.ts";

export interface MixerPanelProps {
  api: MixerApi;
  /** 増えるたびに state を読み直す（外から編集されたとき）。 */
  refreshKey: number;
}

/** Mixer。マスター、ダッキング、バス、トラックの縦の並び。操作は変えた後の全体を 1 回だけ送る。 */
export const MixerPanel = ({ api, refreshKey }: MixerPanelProps) => {
  const { state, error, epoch, apply } = useMixer(api, refreshKey);
  const engine = useMeters(api);
  return (
    <section class="mm-mixer" aria-label="Mixer">
      {error && (
        <p class="mm-mixer-error" role="alert">
          {error}
        </p>
      )}
      {state && (
        <div class="mm-mixer-body" key={epoch}>
          <MasterSection state={state} apply={apply} engine={engine} />
          <DuckingSection state={state} apply={apply} />
          <BusSection state={state} apply={apply} />
          <section class="mm-mixer-section" aria-label="Tracks">
            <h3>Tracks</h3>
            <ul class="mm-mixer-tracks">
              {state.tracks.map((t) => (
                <TrackRow key={t.layer} track={t} roles={state.roles} engine={engine} apply={apply} />
              ))}
              {state.tracks.length === 0 && <li class="mm-mixer-empty">No audio layers in this composition.</li>}
            </ul>
          </section>
        </div>
      )}
    </section>
  );
};
