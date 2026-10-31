// 使用制限。statusline から読み、今の値が statusline に無いときは、ログインしているアカウントの値を claude -p の get_usage で読んで補う。
// statusline の値は、描いたセッションが最後に API を呼んだときのもので、バックグラウンドのセッションは使用制限を載せない。
import type { RateLimit, RateLimits } from '../shared/types.ts';
import { peekCached } from './cache.ts';
import { readRateLimits, withPoints } from './claude.ts';
import { askRateLimits } from './haiku.ts';
import { type RateWindow, state, touch } from './store.ts';

const KEY = 'rate-limits';
// これより前に描いた statusline の値は、ほかのセッションが使った分を含まないことがあるので、get_usage で読む。get_usage の値も、これより古ければ読み直す。
const FRESH_MS = 60_000;
// get_usage を読めなくなったときに前に読んだ値を出し続けないよう、これより古い値は使わず、statusline に残る値に戻る。
const USAGE_MAX_AGE_MS = 10 * 60_000;

/**
 * 今の枠の使用率の推移を、新しい値で進める。使用率が最後の点と変わったときだけ点を足し、変わらなければ prev をそのまま返す。
 * 枠は始まりの使用率が 0 なので、点が無いときは 0 から変わったかを見る。resetsAt が変わったら新しい枠として、前の点を捨てる。
 */
export const advanceRateWindow = (prev: RateWindow | undefined, limit: RateLimit | undefined, now: number): RateWindow | undefined => {
  // リセットの時刻が無い値は、どの枠のものか分からない。
  if (limit?.resetsAt === undefined) return prev;
  const current = prev?.resetsAt === limit.resetsAt ? prev : { resetsAt: limit.resetsAt, points: [] };
  const last = current.points.at(-1)?.[1] ?? 0;
  return limit.usedPercent === last ? current : { ...current, points: [...current.points, [now, limit.usedPercent]] };
};

/** 使用率を、今の枠の推移に記録する。変わったときだけ保存して、画面へ配る。 */
const record = (limits: RateLimits | undefined, at: number): void => {
  let updated = false;
  for (const key of ['fiveHour', 'sevenDay'] as const) {
    const next = advanceRateWindow(state.rateWindows[key], limits?.[key], at);
    if (next === state.rateWindows[key]) continue;
    state.rateWindows[key] = next;
    updated = true;
  }
  if (updated) touch();
};

// get_usage を読むのは、statusline に今の値が無いときだけなので、読んだ値をそのまま推移に記録する。
const loadUsage = async (): Promise<RateLimits> => {
  const limits = await askRateLimits();
  record(limits, limits.updatedAt);
  return limits;
};

/** get_usage で読んだ値。古くなっていれば裏で読み直す。読んだ後にリセットを過ぎた枠は出さない。 */
const peekUsage = (now: number): RateLimits | undefined => {
  const limits = peekCached(KEY, { maxAge: FRESH_MS }, loadUsage);
  if (!limits || now - limits.updatedAt > USAGE_MAX_AGE_MS) return undefined;
  const current = (limit: RateLimit | undefined): RateLimit | undefined => (limit && (limit.resetsAt ?? Infinity) > now ? limit : undefined);
  const fiveHour = current(limits.fiveHour);
  const sevenDay = current(limits.sevenDay);
  return fiveHour || sevenDay
    ? { fiveHour: withPoints(fiveHour, state.rateWindows.fiveHour), sevenDay: withPoints(sevenDay, state.rateWindows.sevenDay), updatedAt: limits.updatedAt }
    : undefined;
};

/** 今の使用制限。1 分以内に描いた statusline に今の 5 時間の枠があれば、その値を出す。無ければ get_usage で読んだ値を、それも無ければ statusline に残る値を出す。 */
export const currentRateLimits = (now: number): RateLimits | undefined => {
  const fresh = readRateLimits(FRESH_MS);
  return fresh?.fiveHour ? fresh : (peekUsage(now) ?? readRateLimits());
};

/** statusline が変わったときに、今の使用率を推移に記録する。 */
export const recordRateLimits = (): void => {
  const now = Date.now();
  record(currentRateLimits(now), now);
};
