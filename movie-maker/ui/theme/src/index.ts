import themeJson from "../theme.json" with { type: "json" };

/** theme.json の 1 つの配色（ライトかダーク）。 */
export interface Mode {
  color: Record<string, string>;
  gradient: Record<string, { direction: "horizontal" | "vertical"; from: string; to: string }>;
}

/** theme.json の全体。wgpu の面も同じ形で読む。 */
export interface Theme {
  version: number;
  font: { family: string[]; mono: string[]; size: Record<string, number> };
  shape: {
    radius: Record<string, number>;
    borderWidth: number;
    space: Record<string, number>;
    /** 部品の寸法（矢印、箇条書きの点、左の列の幅など）。 */
    size: Record<string, number>;
    /** 線の太さ（矢印、フォーカスの枠）。`borderWidth` は面の境界。 */
    stroke: Record<string, number>;
  };
  modes: Record<ModeName, Mode>;
}

export type ModeName = "light" | "dark";

export const MODE_NAMES: readonly ModeName[] = ["light", "dark"];

/** CSS 変数の名前の頭。パネル側は var(--mm-...) だけを使う。 */
const PREFIX = "--mm";

const HEX_RE = /^#[0-9a-f]{6}$/i;
const HEX_RADIX = 16;
const BYTE_MAX = 255;
const SRGB_KNEE = 0.04045;
const SRGB_LOW_DIVISOR = 12.92;
const SRGB_OFFSET = 0.055;
const SRGB_SCALE = 1.055;
const SRGB_GAMMA = 2.4;
const CHANNEL_COUNT = 3;
const HEX_CHANNEL_LEN = 2;

/** 同梱の theme.json。 */
export const theme: Theme = themeJson as Theme;

/**
 * sRGB の 16 進（#rrggbb）を線形 RGB（0〜1）にする。
 * wgpu はブレンドを線形で行うので、同じ JSON から線形の値も引けるようにしておく。
 */
export const hexToLinear = (hex: string): [number, number, number] => {
  if (!HEX_RE.test(hex)) {
    throw new Error(`色は #rrggbb で書く: ${hex}`);
  }
  const [r, g, b] = Array.from({ length: CHANNEL_COUNT }, (_, i) => {
    const start = 1 + i * HEX_CHANNEL_LEN;
    const srgb = parseInt(hex.slice(start, start + HEX_CHANNEL_LEN), HEX_RADIX) / BYTE_MAX;
    return srgb <= SRGB_KNEE
      ? srgb / SRGB_LOW_DIVISOR
      : ((srgb + SRGB_OFFSET) / SRGB_SCALE) ** SRGB_GAMMA;
  });
  return [r ?? 0, g ?? 0, b ?? 0];
};

const px = (n: number): string => `${n}px`;

const GENERIC_FAMILIES = new Set(["sans-serif", "serif", "monospace"]);

const fontStack = (names: string[]): string =>
  names.map((n) => (GENERIC_FAMILIES.has(n) ? n : `"${n}"`)).join(", ");

const gradientCss = (g: Mode["gradient"][string]): string =>
  `linear-gradient(${g.direction === "horizontal" ? "to right" : "to bottom"}, ${g.from}, ${g.to})`;

const modeVars = (mode: Mode): string[] => [
  ...Object.entries(mode.color).map(([k, v]) => `${PREFIX}-color-${k}: ${v};`),
  ...Object.entries(mode.gradient).map(([k, v]) => `${PREFIX}-gradient-${k}: ${gradientCss(v)};`),
];

const sharedVars = (t: Theme): string[] => [
  `${PREFIX}-font-family: ${fontStack(t.font.family)};`,
  `${PREFIX}-font-mono: ${fontStack(t.font.mono)};`,
  ...Object.entries(t.font.size).map(([k, v]) => `${PREFIX}-font-size-${k}: ${px(v)};`),
  ...Object.entries(t.shape.radius).map(([k, v]) => `${PREFIX}-radius-${k}: ${px(v)};`),
  `${PREFIX}-border-width: ${px(t.shape.borderWidth)};`,
  ...Object.entries(t.shape.space).map(([k, v]) => `${PREFIX}-space-${k}: ${px(v)};`),
  ...Object.entries(t.shape.size).map(([k, v]) => `${PREFIX}-size-${k}: ${px(v)};`),
  ...Object.entries(t.shape.stroke).map(([k, v]) => `${PREFIX}-stroke-${k}: ${px(v)};`),
];

const block = (selector: string, lines: string[]): string =>
  `${selector} {\n${lines.map((l) => `  ${l}`).join("\n")}\n}\n`;

/**
 * theme.json から CSS 変数の定義を作る。
 * data-theme 属性でライトとダークを切り替え、形の値（角丸、字、余白）は両方で共通にする。
 */
export const generateCss = (t: Theme = theme): string =>
  [
    "/* theme.json から生成。直接は書き換えない */",
    block(":root", sharedVars(t)),
    block(':root, :root[data-theme="light"]', ["color-scheme: light;", ...modeVars(t.modes.light)]),
    block(':root[data-theme="dark"]', ["color-scheme: dark;", ...modeVars(t.modes.dark)]),
  ].join("\n");
