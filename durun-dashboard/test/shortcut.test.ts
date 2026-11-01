import assert from 'node:assert/strict';
import { test } from 'node:test';
import { onDoubleTap } from '../client/shortcut.ts';

test('修飾キーを素早く 2 回叩いたときだけ呼び、押し続けたとき、間にほかのキーを挟んだとき、間が空いたときは呼ばない', () => {
  const handlers = new Map<string, (e: object) => void>();
  Object.assign(globalThis, { addEventListener: (type: string, fn: (e: object) => void) => handlers.set(type, fn), removeEventListener: () => undefined });
  let calls = 0;
  onDoubleTap(() => calls++);
  // テストは Node で動き、Mac の UA ではないので、Ctrl を叩く。
  const send = (type: string, key: string, timeStamp: number) => handlers.get(type)?.({ key, timeStamp, repeat: false });
  const tap = (at: number, hold = 50) => {
    send('keydown', 'Control', at);
    send('keyup', 'Control', at + hold);
  };
  // 時刻はページを開いてからの経過で、0 にはならない。
  tap(1000);
  tap(1150);
  assert.equal(calls, 1);
  tap(2000, 400);
  tap(2500, 400);
  assert.equal(calls, 1);
  tap(4000);
  send('keydown', 'c', 4060);
  tap(4100);
  assert.equal(calls, 1);
  tap(6000);
  tap(6600);
  assert.equal(calls, 1);
});
