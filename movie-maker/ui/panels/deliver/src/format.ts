import type { Rational } from "@mm/sdk";

const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const BYTES_PER_UNIT = 1024;
const UNITS = ["B", "KB", "MB", "GB"] as const;
export const PERCENT = 100;
const PAD_WIDTH = 2;
const FPS_DIGITS = 2;

const pad2 = (n: number): string => String(n).padStart(PAD_WIDTH, "0");

/** 秒を `m:ss`（1 時間以上は `h:mm:ss`）にする。 */
export const formatDuration = (seconds: number): string => {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / SECONDS_PER_HOUR);
  const m = Math.floor((total % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
  const s = total % SECONDS_PER_MINUTE;
  return h > 0 ? `${h}:${pad2(m)}:${pad2(s)}` : `${m}:${pad2(s)}`;
};

/** バイト数を KB、MB、GB のうち読みやすい単位にする。 */
export const formatBytes = (bytes: number): string => {
  let value = bytes;
  let unit = 0;
  while (value >= BYTES_PER_UNIT && unit < UNITS.length - 1) {
    value /= BYTES_PER_UNIT;
    unit += 1;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${UNITS[unit]}`;
};

/** 有理数を `30`、`30000/1001` の形にする。 */
export const formatRational = (r: Rational): string => (r.den === 1 ? `${r.num}` : `${r.num}/${r.den}`);

/** fps の表示。`29.97` のように小数 2 桁までで、割り切れるときは整数。 */
export const formatFps = (r: Rational): string => `${Number((r.num / r.den).toFixed(FPS_DIGITS))} fps`;

/** 進み具合の割合（0 から 100 の整数）。総数が分からない間は 0。 */
export const percent = (done: number, total: number): number =>
  total > 0 ? Math.min(PERCENT, Math.floor((done * PERCENT) / total)) : 0;
