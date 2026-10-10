import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Session } from '../shared/types.ts';
import { arrangeSessions, moveTo } from '../client/order.ts';

const session = (id: string, status: Session['status'], updatedAt: number) => ({ id, status, updatedAt }) as Session;

test('arrangeSessions は並べ替えた順に従い、並べ替えた後に現れたセッションを前に、状態と更新の順で置く', () => {
  const list = [session('a', 'idle', 1), session('b', 'busy', 2), session('c', 'idle', 3), session('d', 'ended', 4)];
  assert.deepEqual(arrangeSessions(list, []).map((s) => s.id), ['b', 'c', 'a', 'd']);
  assert.deepEqual(arrangeSessions(list, ['d', 'a']).map((s) => s.id), ['b', 'c', 'd', 'a']);
});

test('moveTo は、下へ動かすと target の後ろに、上へ動かすと target の前に置く', () => {
  assert.deepEqual(moveTo(['a', 'b', 'c'], 'a', 'b'), ['b', 'a', 'c']);
  assert.deepEqual(moveTo(['a', 'b', 'c'], 'c', 'a'), ['c', 'a', 'b']);
});
