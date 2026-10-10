import type { TokenTotals } from '../shared/types.ts';

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const JUST_NOW_MS = 10 * SECOND;
const RELATIVE_LIMIT_DAYS = 7;
/** この値未満の epoch は秒とみなす。ミリ秒の epoch は 1973 年以降すべてこの値を超える。 */
const EPOCH_SECONDS_LIMIT = 1e11;

const pad = (n: number): string => String(n).padStart(2, '0');

/** 型に単位の記載がない epoch（レート制限のリセット時刻）を、秒でもミリ秒でもミリ秒にそろえる。 */
export const toMs = (epoch: number): number => (epoch < EPOCH_SECONDS_LIMIT ? epoch * SECOND : epoch);

export const clock = (ms: number): string => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

export const dateTime = (ms: number): string => {
  const d = new Date(ms);
  return `${d.getMonth() + 1}/${d.getDate()} ${clock(ms)}`;
};

export const dateTimeSeconds = (ms: number): string => `${dateTime(ms)}:${pad(new Date(ms).getSeconds())}`;

/** 今日なら時刻だけにし、ほかの日なら日付も付ける。 */
export const shortDateTime = (ms: number, now: number): string =>
  new Date(ms).toDateString() === new Date(now).toDateString() ? clock(ms) : dateTime(ms);

export const relativeTime = (at: number, now: number): string => {
  const diff = Math.max(0, now - at);
  if (diff < JUST_NOW_MS) return 'たった今';
  if (diff < MINUTE) return `${Math.floor(diff / SECOND)}秒前`;
  if (diff < HOUR) return `${Math.floor(diff / MINUTE)}分前`;
  if (diff < DAY) return `${Math.floor(diff / HOUR)}時間前`;
  if (diff < RELATIVE_LIMIT_DAYS * DAY) return `${Math.floor(diff / DAY)}日前`;
  const d = new Date(at);
  return `${d.getMonth() + 1}/${d.getDate()}`;
};

const KILO = 1e3;
const MEGA = 1e6;
const SIGNIFICANT_DIGITS = 3;

/** 有効数字を 3 桁にそろえる。桁数の違う 12.3k と 123k が並んでも読み取りやすい。 */
const significant = (v: number): string =>
  String(Number(v.toFixed(Math.max(0, SIGNIFICANT_DIGITS - 1 - Math.floor(Math.log10(v))))));

export const fmtTokens = (n: number): string => {
  if (n < KILO) return String(Math.round(n));
  const kilo = significant(n / KILO);
  // 999,999 が 1000k と出ないよう、k の表記が 1000 に達したら M に切り上げる。
  return n >= MEGA || Number(kilo) >= KILO ? `${significant(n / MEGA)}M` : `${kilo}k`;
};

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** ファイルの大きさ。1024 ごとに単位を上げ、KB から上は小数 1 桁までにする。 */
export const fmtBytes = (n: number): string => {
  const i = Math.min(BYTE_UNITS.length - 1, n > 0 ? Math.floor(Math.log2(n) / 10) : 0);
  return `${i === 0 ? n : (n / 1024 ** i).toFixed(1).replace(/.0$/, '')} ${BYTE_UNITS[i]}`;
};

export const fmtInt = (n: number): string => Math.round(n).toLocaleString('ja-JP');

export const totalTokens = (t: TokenTotals): number => t.input + t.output + t.cacheRead + t.cacheCreation;

export const truncate = (text: string, max: number): string =>
  text.length > max ? `${text.slice(0, max)}…` : text;

/** Markdown の先頭から、max 文字に収まるところまでの行を返す。行の途中で切ると書式が崩れるので、1 行目が収まらなければ 1 行目を丸ごと返す。 */
export const headLines = (text: string, max: number): string => {
  if (text.length <= max) return text;
  const end = text.lastIndexOf('\n', max);
  const cut = end > 0 ? end : text.indexOf('\n');
  return cut > 0 ? text.slice(0, cut) : text;
};
