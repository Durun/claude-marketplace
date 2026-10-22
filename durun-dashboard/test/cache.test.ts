import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 覚えた値を書き出すファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
process.env.DURUN_DASHBOARD_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-cache-'));
const { cacheVersion, cached, forgetCached, peekCached, saveCache } = await import('../server/cache.ts');

/** 裏で進む読み込みの続きを、先に動かし終える。 */
const settle = () => new Promise((resolve) => setImmediate(resolve));

/** 呼ばれた順に、後から値を決められる読み込み。 */
const deferred = () => {
  const resolvers: ((value: string) => void)[] = [];
  const load = () => new Promise<string>((resolve) => resolvers.push(resolve));
  return { load, resolvers };
};

test('cached は、覚えた値が無ければ読み終えるのを待ち、古くなるまでは読み直さずに返す', async () => {
  let n = 0;
  const load = async () => `v${++n}`;
  assert.equal(await cached('new', { maxAge: 60_000 }, load), 'v1');
  assert.equal(await cached('new', { maxAge: 60_000 }, load), 'v1');
  assert.equal(n, 1);
});

test('cached は、古くなった値をすぐ返して裏で読み直し、値が変わったときだけ版を上げる', async (t) => {
  t.mock.timers.enable({ apis: ['Date'], now: 1_000_000 });
  const { load, resolvers } = deferred();
  const first = cached('old', { maxAge: 1000 }, load);
  resolvers[0]?.('a');
  assert.equal(await first, 'a');
  const version = cacheVersion();

  t.mock.timers.tick(2000);
  assert.equal(await cached('old', { maxAge: 1000 }, load), 'a');
  resolvers[1]?.('a');
  await settle();
  assert.equal(cacheVersion(), version);

  t.mock.timers.tick(2000);
  assert.equal(await cached('old', { maxAge: 1000 }, load), 'a');
  resolvers[2]?.('b');
  await settle();
  assert.equal(cacheVersion(), version + 1);
  assert.equal(await cached('old', { maxAge: 1000 }, load), 'b');
});

test('cached は、材料が変わると前の値を返して読み直し、wait なら読み終えるのを待つ', async () => {
  assert.equal(await cached('input', { input: 'x' }, async () => 'X'), 'X');
  assert.equal(await cached('input', { input: 'y' }, async () => 'Y'), 'X');
  assert.equal(await cached('input', { input: 'z', wait: true }, async () => 'Z'), 'Z');
});

test('fresh で読み直すと、前に始めた読み込みが後から終わっても、後に始めたほうの値を覚える', async () => {
  const { load, resolvers } = deferred();
  const before = cached('fresh', {}, load);
  const after = cached('fresh', { fresh: true }, load);
  resolvers[1]?.('new');
  assert.equal(await after, 'new');
  resolvers[0]?.('old');
  assert.equal(await before, 'old');
  assert.equal(await cached('fresh', {}, load), 'new');
});

test('forgetCached は値を忘れ、忘れる前に始めた読み込みの値を覚えない', async () => {
  const { load, resolvers } = deferred();
  const before = cached('forget', {}, load);
  forgetCached(/^forg/);
  resolvers[0]?.('before');
  assert.equal(await before, 'before');
  const after = cached('forget', {}, load);
  resolvers[1]?.('after');
  assert.equal(await after, 'after');
});

test('peekCached は待たずに返し、裏で読めなかったら maxAge が過ぎるまで読み直さない', async (t) => {
  t.mock.timers.enable({ apis: ['Date'], now: 1_000_000 });
  t.mock.method(console, 'error', () => {});
  let n = 0;
  const failing = async (): Promise<string> => {
    n++;
    throw new Error('down');
  };
  assert.equal(peekCached('peek', { maxAge: 1000 }, failing), undefined);
  await settle();
  assert.equal(peekCached('peek', { maxAge: 1000 }, failing), undefined);
  assert.equal(n, 1);

  t.mock.timers.tick(2000);
  assert.equal(peekCached('peek', { maxAge: 1000 }, async () => 'ok'), undefined);
  await settle();
  assert.equal(peekCached('peek', { maxAge: 1000 }, failing), 'ok');
});

test('peekCached が待たずに返した値は、裏で読み終えたときも、読めなかったときも、版を上げて画面に取り直させる', async (t) => {
  t.mock.method(console, 'error', () => {});
  const version = cacheVersion();
  assert.equal(peekCached('arrived', {}, async () => 'v'), undefined);
  await settle();
  assert.equal(cacheVersion(), version + 1);
  assert.equal(peekCached('arrived', {}, async () => 'w'), 'v');
  assert.equal(
    peekCached('failed', {}, async () => {
      throw new Error('down');
    }),
    undefined,
  );
  await settle();
  assert.equal(cacheVersion(), version + 2);
});

test('keep なら、覚えた値があれば材料が変わっても読み直さず、覚えた値が無いときだけ読む', async () => {
  let n = 0;
  const load = async () => `v${++n}`;
  assert.equal(peekCached('keep', { input: 'a', keep: true }, load), undefined);
  await settle();
  assert.equal(peekCached('keep', { input: 'b', keep: true }, load), 'v1');
  await settle();
  assert.equal(n, 1);
  assert.equal(peekCached('keep', { input: 'b' }, load), 'v1');
  await settle();
  assert.equal(peekCached('keep', { input: 'b' }, load), 'v2');
});

test('覚えた値はファイルに残り、サーバーを起動し直した後も読み直さずに返す', async () => {
  await cached('saved', {}, async () => ({ a: 1 }));
  saveCache();
  // 別の URL で読み込むと、起動し直したときと同じく、ファイルから読み直したモジュールになる。
  const restarted = await import(new URL('../server/cache.ts?restarted', import.meta.url).href);
  assert.deepEqual(await restarted.cached('saved', {}, async () => assert.fail('読み直しました')), { a: 1 });
});
