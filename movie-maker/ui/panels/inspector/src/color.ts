import type { Rgba } from "@mm/sdk";

const BYTE_MAX = 255;
const HEX_RADIX = 16;
const HEX_CHANNEL_LEN = 2;
/** Program と書き出し（`bt709`）の表示の式のべき。黒のオフセットは持たない。 */
const DISPLAY_GAMMA = 2.4;
const HEX_RE = /^#[0-9a-f]{6}$/i;
/** `#rrggbb` の中の、赤・緑・青の始まりの位置。 */
const RED_AT = 1;
const CHANNEL_STARTS = [RED_AT, RED_AT + HEX_CHANNEL_LEN, RED_AT + HEX_CHANNEL_LEN + HEX_CHANNEL_LEN];

const toDisplay = (c: number): number => Math.max(0, c) ** (1 / DISPLAY_GAMMA);

const fromDisplay = (c: number): number => c ** DISPLAY_GAMMA;

const clamp01 = (c: number): number => Math.min(1, Math.max(0, c));

const byteHex = (c: number): string =>
  Math.round(clamp01(toDisplay(c)) * BYTE_MAX)
    .toString(HEX_RADIX)
    .padStart(HEX_CHANNEL_LEN, "0");

/** 線形・premultiplied の色を、色選択の `#rrggbb`（表示の値、straight）にする。表示の式は Program と同じ Rec.1886（08-ui.md の「色の表示の式」）。 */
export const rgbaToHex = (c: Rgba): string => {
  const un = c.a > 0 ? 1 / c.a : 0;
  return `#${byteHex(c.r * un)}${byteHex(c.g * un)}${byteHex(c.b * un)}`;
};

/** `#rrggbb` と透明度から、線形・premultiplied の色を作る。 */
export const hexToRgba = (hex: string, a: number): Rgba | undefined => {
  if (!HEX_RE.test(hex)) return undefined;
  const [r, g, b] = CHANNEL_STARTS.map(
    (i) => fromDisplay(parseInt(hex.slice(i, i + HEX_CHANNEL_LEN), HEX_RADIX) / BYTE_MAX) * a,
  );
  return { r: r ?? 0, g: g ?? 0, b: b ?? 0, a };
};
