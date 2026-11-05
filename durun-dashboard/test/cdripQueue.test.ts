import assert from 'node:assert/strict';
import { test } from 'node:test';
import { applyList, newItem, nextWaiting, parseList, recover, TransitionError, transition } from '../server/cdripQueue.ts';
import type { CdRipItem } from '../shared/types.ts';

const ids = (): (() => string) => {
  let n = 0;
  return () => `id${++n}`;
};

const item = (patch: Partial<CdRipItem> = {}): CdRipItem => ({ ...newItem({ hint: 'MEGAREX' }, 'a', 1), ...patch });

test('リストは 1 行 1 枚で、## の行を次の ## までの行のグループ名にし、空行と前後の空白は無視する', () => {
  const text = ['  MEGAREX  ', '', '## M3-2026秋', 'https://example.com/album', 'Synthion', '##', 'ほかの1枚', ''].join('\r\n');
  assert.deepEqual(parseList(text), [
    { hint: 'MEGAREX' },
    { hint: 'https://example.com/album', group: 'M3-2026秋' },
    { hint: 'Synthion', group: 'M3-2026秋' },
    { hint: 'ほかの1枚' },
  ]);
});

test('受付の前にリストを貼り直すと待機中の行だけを置き換え、同じ位置に同じ手がかりの行があれば id を残す', () => {
  const next = ids();
  const first = applyList([], parseList('A\nB'), false, 1, next);
  assert.deepEqual(first.map((i) => [i.id, i.hint]), [['id1', 'A'], ['id2', 'B']]);
  const ripping = { ...first[0]!, status: 'ripping' as const };
  const again = applyList([ripping, first[1]!], parseList('B\nC'), false, 2, next);
  // 取り込み中の A は残り、待機中の B は手がかりが同じ位置ではないので作り直す。
  assert.deepEqual(again.map((i) => [i.hint, i.status]), [['A', 'ripping'], ['B', 'waiting'], ['C', 'waiting']]);
  const same = applyList(again, parseList('B\nC'), false, 3, next);
  assert.deepEqual(same.map((i) => i.id), again.map((i) => i.id));
});

test('受付中にリストを貼ると末尾に足すだけで、すでにある行の順番を動かさない', () => {
  const next = ids();
  const items = applyList([], parseList('A\nB'), false, 1, next);
  const added = applyList(items, parseList('C'), true, 2, next);
  assert.deepEqual(added.map((i) => i.hint), ['A', 'B', 'C']);
  assert.equal(added[0], items[0]);
});

test('次に取り込む行は、リストの上から数えて最初の待機中の行', () => {
  const items = [item({ id: '1', status: 'done' }), item({ id: '2', status: 'ripping' }), item({ id: '3' }), item({ id: '4' })];
  assert.equal(nextWaiting(items)?.id, '3');
  assert.equal(nextWaiting(items.slice(0, 2)), undefined);
});

test('入れた順の割り当て: 取り込みが終わるたびに、次の待機中の行が取り込みの対象になる', () => {
  let items = applyList([], parseList('A\nB\nC'), false, 1, ids());
  const order: string[] = [];
  while (nextWaiting(items)) {
    const target = nextWaiting(items)!;
    order.push(target.hint);
    items = items.map((i) => (i.id === target.id ? transition(transition(i, { type: 'start' }, 2), { type: 'ripped', researched: true }, 3) : i));
  }
  assert.deepEqual(order, ['A', 'B', 'C']);
  assert.deepEqual(items.map((i) => i.status), ['review', 'review', 'review']);
});

test('取り込み済みの後は、研究が済んでいれば確認待ち、まだなら情報収集中になる', () => {
  const ripping = transition(item(), { type: 'start' }, 2);
  assert.equal(ripping.status, 'ripping');
  assert.equal(transition(ripping, { type: 'ripped', researched: false }, 3).status, 'researching');
  assert.equal(transition(ripping, { type: 'ripped', researched: true }, 3).status, 'review');
});

test('取り込み中に研究が終わったら状態は変えず、情報収集中なら確認待ちにして、注意があれば残す', () => {
  const ripping = transition(item(), { type: 'start' }, 2);
  const during = transition(ripping, { type: 'researched', attention: '曲数が違います' }, 3);
  assert.deepEqual([during.status, during.attention], ['ripping', '曲数が違います']);
  const after = transition(item({ status: 'researching' }), { type: 'researched' }, 3);
  assert.deepEqual([after.status, after.attention], ['review', undefined]);
});

test('書き出しの成功は done と書き出したファイルを残し、失敗は確認待ちに戻して理由を残す', () => {
  const review = item({ status: 'review', attention: '前の理由' });
  const exporting = transition(review, { type: 'export' }, 5);
  assert.deepEqual([exporting.status, exporting.attention], ['exporting', undefined]);
  assert.deepEqual(transition(exporting, { type: 'exported', files: ['a.flac'] }, 6).exported, { at: 6, files: ['a.flac'] });
  const failed = transition(exporting, { type: 'exportFailed', error: '既にあります' }, 6);
  assert.deepEqual([failed.status, failed.attention], ['review', '既にあります']);
});

test('中断と取り込みのやり直しは待機中に戻し、取り込んだ曲の情報を捨てるが、手がかりと曲情報は残す', () => {
  const ripping = item({ status: 'ripping', disc: { tracks: 8, durations: [] }, progress: { track: 2, total: 8 }, meta: { circle: 'x', album: 'y', artist: 'z', dest: 'CLUB', tracks: [] } });
  const aborted = transition(ripping, { type: 'abort' }, 9);
  assert.deepEqual([aborted.status, aborted.disc, aborted.progress], ['waiting', undefined, undefined]);
  assert.equal(aborted.meta?.album, 'y');
  assert.equal(transition(item({ status: 'failed', attention: 'x' }), { type: 'retry' }, 9).status, 'waiting');
});

test('許されない遷移は TransitionError にする', () => {
  assert.throws(() => transition(item({ status: 'done' }), { type: 'export' }, 1), TransitionError);
  assert.throws(() => transition(item({ status: 'ripping' }), { type: 'skip' }, 1), TransitionError);
  assert.throws(() => transition(item({ status: 'waiting' }), { type: 'ripped', researched: true }, 1), TransitionError);
});

test('起動時の復旧: 取り込み中だった行は、曲数どおりの FLAC と rip.log があれば先へ進め、無ければ失敗にし、書き出し中だった行は確認待ちに戻す', () => {
  const disc = { tracks: 8, durations: [] };
  const items = [
    item({ id: 'ok', status: 'ripping', disc }),
    item({ id: 'ok2', status: 'ripping', disc, research: { startedAt: 1, finishedAt: 2, sources: [] } }),
    item({ id: 'short', status: 'ripping', disc }),
    item({ id: 'nolog', status: 'ripping', disc }),
    item({ id: 'nodisc', status: 'ripping' }),
    item({ id: 'exp', status: 'exporting' }),
    item({ id: 'done', status: 'done' }),
  ];
  const probes: Record<string, { flacs: number; logged: boolean }> = { ok: { flacs: 8, logged: true }, ok2: { flacs: 8, logged: true }, short: { flacs: 5, logged: true }, nolog: { flacs: 8, logged: false }, nodisc: { flacs: 3, logged: true } };
  const out = recover(items, (i) => probes[i.id] ?? { flacs: 0, logged: false }, 10);
  assert.deepEqual(out.map((i) => i.status), ['researching', 'review', 'failed', 'failed', 'failed', 'review', 'done']);
  assert.match(out[2]?.attention ?? '', /取り込みを中断/);
  assert.match(out[5]?.attention ?? '', /書き出しを中断/);
  assert.equal(out[6], items[6]);
});
