import type { Rational, RenderTarget } from "@mm/sdk";
import type { DeliverOverview } from "./api.ts";
import { withRequired, type FieldSpec } from "./form.ts";

/** プリセット。大きさ、フレームレート、コーデック、拡張子だけを決め、残りのフィールドは足すときに埋める。 */
export interface Preset {
  id: string;
  label: string;
  width: number;
  height: number;
  fps: Rational;
  codec: string;
  extension: string;
  /** 目標のラウドネス（LUFS）。YouTube は -14。 */
  loudness_lufs?: number;
  /** 縦長の成果物。足すときの既定のコンポジションを、縦長のものから選ぶ。 */
  portrait?: boolean;
}

const FPS_30: Rational = { num: 30, den: 1 };
const YOUTUBE_LUFS = -14;

/** 配信先ごとのプリセット。ProRes のプロファイル（HQ）はコーデックの名前に出ないので、エンコーダーの側の既定に従う。 */
export const PRESETS: readonly Preset[] = [
  { id: "youtube-1080p", label: "YouTube 1080p", width: 1920, height: 1080, fps: FPS_30, codec: "h264_nvenc", extension: "mp4", loudness_lufs: YOUTUBE_LUFS },
  { id: "youtube-shorts", label: "YouTube Shorts 1080x1920", width: 1080, height: 1920, fps: FPS_30, codec: "h264_nvenc", extension: "mp4", loudness_lufs: YOUTUBE_LUFS, portrait: true },
  { id: "prores-422-hq", label: "ProRes 422 HQ master", width: 1920, height: 1080, fps: FPS_30, codec: "prores_ks", extension: "mov" },
];

/** 出力先の既定のフォルダー（プロジェクトのフォルダーからの相対）。 */
export const DEFAULT_OUTPUT_DIR = "exports";

const FIRST_SUFFIX = 2;

/** `base` が `taken` に無ければそのまま、あれば `base-2`、`base-3` と数える。 */
export const uniqueId = (base: string, taken: readonly string[]): string => {
  const from = (n: number): string => (taken.includes(`${base}-${n}`) ? from(n + 1) : `${base}-${n}`);
  return taken.includes(base) ? from(FIRST_SUFFIX) : base;
};

/** プリセットからレンダーターゲットを作る。必須のフィールドが増えていても、スキーマの既定値で埋める。 */
export const targetFromPreset = (
  preset: Preset,
  context: { composition: string; takenIds: readonly string[] },
  specs: FieldSpec[],
): RenderTarget => {
  const id = uniqueId(preset.id, context.takenIds);
  return withRequired(
    {
      id,
      composition: context.composition,
      width: preset.width,
      height: preset.height,
      fps: preset.fps,
      codec: preset.codec,
      output_path: `${DEFAULT_OUTPUT_DIR}/${id}.${preset.extension}`,
      loudness_lufs: preset.loudness_lufs,
    },
    specs,
  );
};

/** 「Custom」の最初の値。いちばん使う YouTube 1080p から始める。 */
export const customPreset: Preset = { ...(PRESETS[0] as Preset), id: "target", label: "Custom" };

/**
 * プリセットを足すときの既定のコンポジション。縦長のプリセットは、いま開いているのが縦長ならそれ、
 * 無ければ縦長のうち先頭（本編の 16:9 を縦長で書き出さない）。縦長が 1 つも無ければ開いているもの。
 */
export const defaultComposition = (preset: Preset, overview: Pick<DeliverOverview, "compositions" | "open_composition">): string => {
  const open = overview.compositions.find((c) => c.id === overview.open_composition);
  const fallback = open?.id ?? overview.compositions[0]?.id ?? "";
  if (!preset.portrait) return fallback;
  const isPortrait = (c: { width: number; height: number }): boolean => c.height > c.width;
  return (open && isPortrait(open) ? open : overview.compositions.find(isPortrait))?.id ?? fallback;
};
