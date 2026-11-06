import type { Rational, RenderTarget } from "@mm/sdk";
import { withRequired, type FieldSpec } from "./form.ts";

/** プリセット。大きさ、フレームレート、コーデック、拡張子だけを決め、残りの欄は足すときに埋める。 */
export interface Preset {
  id: string;
  label: string;
  width: number;
  height: number;
  fps: Rational;
  codec: string;
  extension: string;
}

const FPS_30: Rational = { num: 30, den: 1 };

/** 書き出し先ごとの定番。ProRes のプロファイル（HQ）はコーデックの名前に出ないので、エンコーダーの側の既定に従う。 */
export const PRESETS: readonly Preset[] = [
  { id: "youtube-1080p", label: "YouTube 1080p", width: 1920, height: 1080, fps: FPS_30, codec: "h264", extension: "mp4" },
  { id: "youtube-shorts", label: "YouTube Shorts 1080x1920", width: 1080, height: 1920, fps: FPS_30, codec: "h264", extension: "mp4" },
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

/** プリセットからレンダーターゲットを作る。必須の欄が増えていても、スキーマの既定値で埋める。 */
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
    },
    specs,
  );
};

/** 「Custom」の最初の値。いちばん使う YouTube 1080p から始める。 */
export const customPreset: Preset = { ...(PRESETS[0] as Preset), id: "target", label: "Custom" };
