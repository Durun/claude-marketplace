// ダッシュボード自身で起きた想定外のエラーと、見込みより時間のかかった処理を集め、改善要望として起票する。
// 大量の記録で改善が埋もれないよう、同じものは 1 件にまとめて数え、未対応の要望はエラーと遅い処理のそれぞれで少数に抑える。
import path from 'node:path';
import { pendingItems, saveFeedback } from './improve.ts';
import { persist, state, type ErrorRecord } from './store.ts';

const MAX_PENDING = 3;
// 直しても再発し続けるエラーで、同じ要望が繰り返し起票されないようにする。
const MAX_FILINGS = 2;
const MAX_RECORDS = 100;
const MAX_MESSAGE_CHARS = 500;
const STACK_LINES = 12;

const SOURCE_LABEL: Record<ErrorRecord['source'], string> = { server: 'サーバー', client: '画面' };

const ERROR_REQUEST = 'ダッシュボードで想定外のエラーが起きています。原因を調べて直してください。直さなくてよいエラーなら、理由を書いて止まってください。';
// 終えるまでに時間のかかる処理はあっても、そのために画面を固めたり人間を待たせたりはしない。
const SLOW_REQUEST =
  'ダッシュボードで、見込みより時間のかかった処理があります。時間のかかっているところを調べて速くしてください。外部を待つなどで速くできないなら、画面を固めたり人間を待たせたりしないよう、裏で進めてください。見込みが合っていなければ見込みを直し、直さなくてよいなら、理由を書いて止まってください。';

/**
 * 同じエラーをまとめる鍵。id や数字は発生ごとに変わるので、UUID と数字を含む語を伏せる。
 * 場所は最初の関数名で見る。行番号はコードを直すたびにずれ、画面では作り直すたびにずれるため。
 */
const keyOf = (source: string, message: string, stack: string | undefined): string => {
  const frame = stack?.split('\n').find((l) => /^\s*at /.test(l))?.trim().replace(/ \(.*\)$/, '').replace(/:\d+:\d+$/, '');
  return [source, message.replace(/[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}|\w*\d\w*/gi, '#'), frame ?? ''].join('|');
};

const textOf = (r: ErrorRecord): string => {
  const slowest = r.slowestMs === undefined ? '' : `最長 ${(r.slowestMs / 1000).toFixed(1)} 秒、`;
  return [
    r.slowestMs === undefined ? ERROR_REQUEST : SLOW_REQUEST,
    `- 起きた場所: ${SOURCE_LABEL[r.source]}${r.where ? `（${r.where}）` : ''}`,
    `- ${r.slowestMs === undefined ? 'エラー' : '遅い処理'}: ${r.message}`,
    `- 回数: ${r.count} 回（${slowest}最初 ${new Date(r.firstAt).toLocaleString('ja-JP')}、最後 ${new Date(r.lastAt).toLocaleString('ja-JP')}）`,
    ...(r.filed > 0 ? [`- 前回の要望（${r.item}）に対応した後も起きています。`] : []),
    ...(r.stack ? ['', r.stack.split('\n').slice(0, STACK_LINES).join('\n')] : []),
  ].join('\n');
};

const prune = (): void => {
  const keys = Object.keys(state.errors).sort((a, b) => (state.errors[b]?.lastAt ?? 0) - (state.errors[a]?.lastAt ?? 0));
  for (const key of keys.slice(MAX_RECORDS)) delete state.errors[key];
};

/** 記録して、まだ起票していないか、前の要望の対応が済んだ後に再発したものを起票する。遅い処理は、ms にかかった時間を渡す。 */
const record = (source: ErrorRecord['source'], message: string, stack: string | undefined, where: string | undefined, ms?: number): void => {
  // 記録の失敗でエラーが連鎖しないよう、ここで止める。
  try {
    const now = Date.now();
    const key = keyOf(source, message, stack);
    const r = (state.errors[key] ??= { source, message: message.slice(0, MAX_MESSAGE_CHARS), count: 0, firstAt: now, lastAt: now, filed: 0 });
    Object.assign(r, { stack: stack ?? r.stack, where: where ?? r.where, count: r.count + 1, lastAt: now });
    if (ms !== undefined) r.slowestMs = Math.max(r.slowestMs ?? 0, ms);
    const pending = new Set(pendingItems());
    // 遅い処理の要望がたまっていてもエラーを起票できるよう、別々に数える。
    const open = Object.values(state.errors).filter((e) => e.item && pending.has(e.item) && (e.slowestMs === undefined) === (ms === undefined)).length;
    if (!(r.item && pending.has(r.item)) && r.filed < MAX_FILINGS && open < MAX_PENDING) {
      r.item = path.basename(saveFeedback(textOf(r), [], { error: r }));
      r.filed += 1;
    }
    prune();
    persist();
  } catch (e) {
    console.error('記録できません', e);
  }
};

export const recordError = (source: ErrorRecord['source'], message: string, stack?: string, where?: string): void => record(source, message, stack, where);

/** 見込みより時間のかかった処理。同じ処理は message でまとめる。detail には、どこに時間がかかったかの内訳を渡す。 */
export const recordSlow = (source: ErrorRecord['source'], message: string, ms: number, where?: string, detail?: string): void => record(source, message, detail, where, ms);
