// TypeSafe の判定モデル Jev に、二値の問い（Noul）と段階の問い（Score）をまとめて投げる。数百ミリ秒で、問いごとの確率を返す。
// キーは ~/.durun-dashboard/.env.local の TYPESAFE_AI_API_KEY から、起動時に環境変数として読む。無いときは呼び出し側が haiku で代える。
import { type OnUsage, type RawUsage, UpstreamError, tokensOf } from './haiku.ts';

// テストでは、偽の Jev のアドレスに差し替える。
const endpoint = (): string => process.env.DURUN_DASHBOARD_FAKE_JEV ?? 'https://api.typesafe.ai/v1/systemone';
const TIMEOUT_MS = 60_000;

export type NoulQuestion = { instructions: string; criteria: { true: string; false: string } };
/** criteria は段階の説明で、低い段階から並べる。 */
export type ScoreQuestion = { instructions: string; criteria: string[] };
export type JevQuestion = ({ type: 'noul' } & NoulQuestion) | ({ type: 'score' } & ScoreQuestion);

export const hasJev = (): boolean => !!process.env.TYPESAFE_AI_API_KEY || !!process.env.DURUN_DASHBOARD_FAKE_JEV;

/** state を前提に、問いごとの確率を問いと同じ順で返す。Noul は「はい」の確率を 1 つ、Score は段階ごとの確率を低い段階から並べる。 */
export const askJev = async (state: Record<string, unknown>, questions: JevQuestion[], onUsage?: OnUsage): Promise<number[][]> => {
  const res = await fetch(endpoint(), {
    method: 'POST',
    headers: { authorization: `Bearer ${process.env.TYPESAFE_AI_API_KEY}`, 'content-type': 'application/json' },
    body: JSON.stringify({ state, model: 'jev-latest', questions: Object.fromEntries(questions.map((q, n) => [`q${n}`, q])) }),
    signal: AbortSignal.timeout(TIMEOUT_MS),
  }).catch((e: Error) => {
    throw new UpstreamError(`Jev に届きませんでした（${e.message}）`);
  });
  if (!res.ok) throw new UpstreamError(`Jev が ${res.status} を返しました：${(await res.text()).slice(0, 200)}`);
  const { answers, usage } = (await res.json()) as { answers?: Record<string, { noul?: unknown; probabilities?: Record<string, unknown> }>; usage?: RawUsage };
  if (usage) onUsage?.(tokensOf(usage));
  return questions.map((q, n) => {
    const a = answers?.[`q${n}`];
    const ps = q.type === 'noul' ? [a?.noul] : q.criteria.map((_, k) => a?.probabilities?.[k]);
    if (!ps.every((p) => typeof p === 'number')) throw new UpstreamError('Jev の答えが欠けています');
    return ps as number[];
  });
};

/** state を前提に、問いごとの「はい」の確率を、問いと同じ順で返す。 */
export const askNoul = async (state: Record<string, unknown>, questions: NoulQuestion[], onUsage?: OnUsage): Promise<number[]> =>
  (await askJev(state, questions.map((q) => ({ type: 'noul', ...q })), onUsage)).map(([p]) => p!);
