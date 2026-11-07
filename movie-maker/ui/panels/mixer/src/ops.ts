import type { CompMixer, Ducking, LayerAudio, Op, Param } from "@mm/sdk";
import type { MixerState, MixerTrack } from "./api.ts";

/**
 * 送る直前の state から操作を作る関数。操作は変えた後の全体を置くので、押した時点の state で作ると、
 * 前の操作の応答を待つ間に続けて押した変更（ミュートの直後のソロなど）が前の変更を上書きして消す。
 */
export type BuildOps = (state: MixerState) => Op[];

/** 送る直前の state のトラック `layer` から操作を作る。トラックが消えていれば何も送らない。 */
export const forTrack =
  (layer: string, build: (track: MixerTrack) => Op): BuildOps =>
  (state) => {
    const track = state.tracks.find((t) => t.layer === layer);
    return track ? [build(track)] : [];
  };

/** 送る直前の state のダッキングに `patch` を重ねる。無効になっていれば何も送らない。 */
export const duckingOps =
  (patch: (ducking: Ducking) => Partial<Ducking>): BuildOps =>
  (state) => {
    const ducking = state.mixer?.ducking;
    return ducking ? [mixerOp(state, { ducking: { ...ducking, ...patch(ducking) } })] : [];
  };

/** 音の設定が無いトラックの既定の送り先。コアの既定（役割 voice、0 dB、中央）と同じ。 */
export const DEFAULT_ROLE = "voice";

/** ダッキングを有効にしたときの既定値。 */
export const DEFAULT_DUCKING: Ducking = {
  trigger: "voice",
  targets: ["bgm"],
  reduction_db: -12,
  threshold_db: -40,
  attack_ms: 20,
  release_ms: 300,
};

export const numberParam = (value: number, base?: Param): Param => ({ ...base, value: { type: "number", value } });

/** Param の数値。数値でないときは `fallback`。 */
export const paramNumber = (p: Param | undefined, fallback = 0): number => (p?.value.type === "number" ? p.value.value : fallback);

/** トラックの音の設定。無ければ既定。 */
export const audioOf = (track: MixerTrack): LayerAudio =>
  track.audio ?? { role: DEFAULT_ROLE, gain_db: numberParam(0), pan: numberParam(0) };

/** 変えた後の全体を `setLayerAudio` で置く。 */
export const trackOp = (track: MixerTrack, patch: Partial<LayerAudio>): Op => ({
  op: "setLayerAudio",
  layer: track.layer,
  audio: { ...audioOf(track), ...patch },
});

export const trackGainOp = (track: MixerTrack, db: number): Op => trackOp(track, { gain_db: numberParam(db, audioOf(track).gain_db) });

export const trackPanOp = (track: MixerTrack, pan: number): Op => trackOp(track, { pan: numberParam(pan, audioOf(track).pan) });

/** 変えた後のコンポジションのミキサー全体を `setMixer` で置く。 */
export const mixerOp = (state: MixerState, patch: Partial<CompMixer>): Op => ({
  op: "setMixer",
  composition: state.composition,
  mixer: { ...state.mixer, ...patch },
});

export const busGain = (state: MixerState, role: string): number => state.mixer?.buses?.[role]?.gain_db ?? 0;

export const busGainOp = (state: MixerState, role: string, db: number): Op =>
  mixerOp(state, { buses: { ...state.mixer?.buses, [role]: { gain_db: db } } });
