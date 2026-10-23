import type { GlossaryTerm, TokenTotals } from '../shared/types.ts';

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

/** 時間の長さを、日と時間か、時間か、分で書く。1 分に満たなければ 1 分とする。 */
export const fmtSpan = (ms: number): string => {
  const hours = Math.floor(ms / HOUR);
  if (hours >= 24) return `${Math.floor(hours / 24)} 日${hours % 24 ? ` ${hours % 24} 時間` : ''}`;
  return hours ? `${hours} 時間` : `${Math.max(1, Math.floor(ms / MINUTE))} 分`;
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

/** 分けた質問への回答を、どの質問への答えかが分かる形で 1 つにまとめる。答えていない質問は省き、自由記述は最後に添える。 */
export const askAnswer = (questions: { question: string }[], picked: string[][], note: string): string => {
  const lines = questions.flatMap((q, i) => {
    const answers = picked[i] ?? [];
    return answers.length ? [`「${q.question}」への回答: ${answers.join('、')}`] : [];
  });
  return [lines.join('\n'), note.trim()].filter(Boolean).join('\n\n');
};

/** Markdown の先頭から、max 文字に収まるところまでの行を返す。行の途中で切ると書式が崩れるので、1 行目が収まらなければ 1 行目を丸ごと返す。 */
export const headLines = (text: string, max: number): string => {
  if (text.length <= max) return text;
  const end = text.lastIndexOf('\n', max);
  const cut = end > 0 ? end : text.indexOf('\n');
  return cut > 0 ? text.slice(0, cut) : text;
};

export const escapeRegExp = (s: string): string => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** 文の切れ端。term があれば、用語集の語。 */
export type TermPiece = { text: string; term?: GlossaryTerm };

/**
 * 文を、用語集の語とそれ以外に分ける関数を作る。同じ語は何度出てもすべて拾う。大文字と小文字は区別せず、長い語から当てる。
 * 英数字で始まるか終わる語は、前後に英数字が続くところでは当てない。RAPID の中の API のような、別の語の一部を拾わないため。
 */
export const termSplitter = (terms: GlossaryTerm[]): ((text: string) => TermPiece[]) => {
  // 語が無いと区切りが空の文字列に当たり、1 文字ずつに分かれてしまう。
  if (terms.length === 0) return (text) => [{ text }];
  const byKey = new Map(terms.map((t) => [t.term.toLowerCase(), t]));
  const words = [...byKey.keys()]
    .sort((a, b) => b.length - a.length)
    .map((k) => `${/^\w/.test(k) ? '(?<!\\w)' : ''}${escapeRegExp(k)}${/\w$/.test(k) ? '(?!\\w)' : ''}`);
  const re = new RegExp(`(${words.join('|')})`, 'i');
  // 区切りをキャプチャすると、split の結果は当たらない部分と当たった部分が交互に並ぶ。
  return (text) =>
    text.split(re).flatMap((part, i) => {
      const term = i % 2 ? byKey.get(part.toLowerCase()) : undefined;
      return !part ? [] : term ? [{ text: part, term }] : [{ text: part }];
    });
};
