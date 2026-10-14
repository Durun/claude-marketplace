import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Session } from '../shared/types.ts';
import { arrangeDocs, arrangeSessions, moveTo } from '../client/order.ts';

const session = (id: string, status: Session['status'], updatedAt: number) => ({ id, status, updatedAt }) as Session;

test('arrangeSessions は並べ替えた順に従い、並べ替えた後に現れたセッションを前に、状態と更新の順で置く', () => {
  const list = [session('a', 'idle', 1), session('b', 'busy', 2), session('c', 'idle', 3), session('d', 'ended', 4)];
  assert.deepEqual(arrangeSessions(list, []).map((s) => s.id), ['b', 'c', 'a', 'd']);
  assert.deepEqual(arrangeSessions(list, ['d', 'a']).map((s) => s.id), ['b', 'c', 'd', 'a']);
});

test('arrangeDocs は、ピン留めした資料を上にまとめ、どちらの中でも並べ替えた順に従い、並べ替えた後に出た資料を前に新しい順で置く', () => {
  const doc = (key: string, at: number, pinned = false) => ({ key, keys: [key], at, pinned });
  const docs = [doc('a', 1), doc('b', 2, true), doc('c', 3), doc('d', 4, true), doc('e', 5)];
  assert.deepEqual(arrangeDocs(docs, []).map((d) => d.key), ['d', 'b', 'e', 'c', 'a']);
  assert.deepEqual(arrangeDocs(docs, ['a', 'b', 'c', 'd']).map((d) => d.key), ['b', 'd', 'e', 'a', 'c']);
});

test('moveTo は、下へ動かすと target の後ろに、上へ動かすと target の前に置く', () => {
  assert.deepEqual(moveTo(['a', 'b', 'c'], 'a', 'b'), ['b', 'a', 'c']);
  assert.deepEqual(moveTo(['a', 'b', 'c'], 'c', 'a'), ['c', 'a', 'b']);
});
