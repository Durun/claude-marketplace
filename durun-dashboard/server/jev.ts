// TypeSafe の判定モデル Jev に、二値の問い（Noul）をまとめて投げる。数百ミリ秒で、問いごとの「はい」の確率を返す。
// キーは環境変数 TYPESAFE_AI_API_KEY から読む。無いときは呼び出し側が Claude で代える。
import { UpstreamError } from './haiku.ts';

// テストでは、偽の Jev のアドレスに差し替える。
const endpoint = (): string => process.env.DURUN_DASHBOARD_FAKE_JEV ?? 'https://api.typesafe.ai/v1/systemone';
const TIMEOUT_MS = 60_000;

export type NoulQuestion = { instructions: string; criteria: { true: string; false: string } };

export const hasJev = (): boolean => !!process.env.TYPESAFE_AI_API_KEY || !!process.env.DURUN_DASHBOARD_FAKE_JEV;

/** state を前提に、問いごとの「はい」の確率を、問いと同じ順で返す。 */
export const askNoul = async (state: Record<string, unknown>, questions: NoulQuestion[]): Promise<number[]> => {
  const res = await fetch(endpoint(), {
    method: 'POST',
    headers: { authorization: `Bearer ${process.env.TYPESAFE_AI_API_KEY}`, 'content-type': 'application/json' },
    body: JSON.stringify({ state, model: 'jev-latest', questions: Object.fromEntries(questions.map((q, n) => [`q${n}`, { type: 'noul', ...q }])) }),
    signal: AbortSignal.timeout(TIMEOUT_MS),
  }).catch((e: Error) => {
    throw new UpstreamError(`Jev に届きませんでした（${e.message}）`);
  });
  if (!res.ok) throw new UpstreamError(`Jev が ${res.status} を返しました：${(await res.text()).slice(0, 200)}`);
  const { answers } = (await res.json()) as { answers?: Record<string, { noul?: unknown }> };
  return questions.map((_, n) => {
    const p = answers?.[`q${n}`]?.noul;
    if (typeof p !== 'number') throw new UpstreamError('Jev の答えが欠けています');
    return p;
  });
};
