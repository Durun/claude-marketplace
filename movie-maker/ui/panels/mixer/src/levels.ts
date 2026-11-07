/** メーターとフェーダーの目盛りの範囲と表示の整形。 */

/** メーターの下端（dBFS）。これ以下は空にする。 */
export const METER_FLOOR_DB = -60;
/** 色が変わる位置（dBFS）。mixer.css のグラデーションの境目と揃える。 */
export const METER_WARN_DB = -18;
export const METER_HOT_DB = -6;
/** フェーダーの範囲（dB）。 */
export const FADER_MIN_DB = -60;
export const FADER_MAX_DB = 12;
export const FADER_STEP_DB = 0.1;
export const PAN_STEP = 0.01;
const PERCENT = 100;
const DECIMALS = 1;

/** dBFS をメーターの長さの割合（0..1）にする。 */
export const dbToFraction = (db: number): number => Math.min(1, Math.max(0, (db - METER_FLOOR_DB) / -METER_FLOOR_DB));

/** 範囲に収める。 */
export const clamp = (v: number, min: number, max: number): number => Math.min(max, Math.max(min, v));

/** dB の数字。測れない（null）は "--"。 */
export const formatDb = (db: number | null | undefined): string => (db === null || db === undefined ? "--" : db.toFixed(DECIMALS));

/** パンの表示。0 は C、負は L、正は R。 */
export const formatPan = (pan: number): string => {
  const n = Math.round(Math.abs(pan) * PERCENT);
  if (n === 0) return "C";
  return `${pan < 0 ? "L" : "R"}${n}`;
};
