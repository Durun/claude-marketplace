// Mixer パネルをモックの API で動かすページ。スクリーンショット用で、アプリには載せない。
// `?scene=meters`（既定）はダッキングなしでメーターが振れる。`?scene=ducking` はダッキング有効で、ミュートとソロがある。
import { render } from "preact";
import type { LayerAudio } from "@mm/sdk";
import type { MixerApi, MixerMeters, MixerState, MixerTrack } from "../src/api.ts";
import { followTheme } from "../../../shell/src/followTheme.ts";
import { MixerPanel } from "../src/index.ts";
import "../../../shell/src/shell.css";

const num = (value: number) => ({ value: { type: "number" as const, value } });
const audio = (role: string, gain: number, pan = 0, over: Partial<LayerAudio> = {}): LayerAudio => ({
  role,
  gain_db: num(gain),
  pan: num(pan),
  ...over,
});

const scene = new URLSearchParams(location.search).get("scene") === "ducking" ? "ducking" : "meters";

const tracks = (): MixerTrack[] => [
  { layer: "l-voice", kind: "footage", label: "narration.wav", audio: audio("voice", -2.5, 0, scene === "ducking" ? { solo: true } : {}), animated: false },
  { layer: "l-bgm", kind: "footage", label: "bgm-loop.mp3", audio: audio("bgm", -9), animated: false },
  { layer: "l-click", kind: "footage", label: "click.wav", audio: audio("sfx", -4, -0.35, scene === "ducking" ? { mute: true } : {}), animated: false },
  { layer: "l-intro", kind: "composition", label: "intro-title", audio: audio("voice", -6), animated: true },
  { layer: "l-broll", kind: "footage", label: "b-roll.mp4", audio: null, animated: false },
];

const state: MixerState = {
  composition: "main",
  mixer:
    scene === "ducking"
      ? {
          master_db: -1.5,
          buses: { bgm: { gain_db: -3 } },
          ducking: { trigger: "voice", targets: ["bgm"], reduction_db: -12, threshold_db: -40, attack_ms: 20, release_ms: 300 },
        }
      : null,
  tracks: tracks(),
  roles: ["voice", "bgm", "sfx"],
};

const apply = (op: Parameters<MixerApi["applyOps"]>[0][number]): void => {
  if (op.op === "setLayerAudio") {
    const t = state.tracks.find((x) => x.layer === op.layer);
    if (t) t.audio = op.audio ?? null;
  }
  if (op.op === "setMixer") state.mixer = op.mixer ?? null;
};

const listeners = new Set<(m: MixerMeters) => void>();
const api: MixerApi = {
  state: async () => structuredClone(state),
  applyOps: async (ops) => ops.forEach(apply),
  onMeters: async (handler) => {
    listeners.add(handler);
    return () => void listeners.delete(handler);
  },
};

// 適当な値を 30 Hz で流す。トラックごとに位相と振幅を変えて、棒の長さと色が揃わないようにする。
const wave = (t: number, phase: number, base: number, swing: number): number => base + swing * Math.sin(t * 3 + phase) * Math.sin(t * 0.7 + phase);
let clock = 0;
setInterval(() => {
  clock += 1 / 30;
  const peaks = state.tracks.map((tr, i) => ({ layer: tr.layer, peak_db: tr.animated ? null : wave(clock, i * 1.3, -20, 16) }));
  const meters: MixerMeters = {
    t: clock,
    playing: true,
    tracks: peaks,
    master: {
      peak_db: [wave(clock, 0.2, -10, 9), wave(clock, 0.9, -11, 9)],
      true_peak_db: -5.4,
      short_term_lufs: -15.8,
      integrated_lufs: -16.3,
    },
  };
  listeners.forEach((l) => l(meters));
}, 33);

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) {
  root.style.boxSizing = "border-box";
  root.style.flexDirection = "column";
  root.style.padding = "10px";
  root.style.width = "300px";
  root.style.height = "100vh";
  render(<MixerPanel api={api} refreshKey={0} />, root);
}
