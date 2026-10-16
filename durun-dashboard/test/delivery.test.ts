import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { ChatEntry, OutboxMessage } from '../shared/types.ts';
import { failureKeys, newFailures, unreadMessages } from '../client/delivery.ts';

const message = (id: string, error?: string): OutboxMessage => ({ id, sessionId: 's1', text: id, createdAt: 0, error });

test('届けられなかったメッセージは、前に見た失敗を除いて拾い、同じメッセージが別の理由でまた失敗したら拾い直す', () => {
  const before = [message('a', 'Workspace not trusted.'), message('b')];
  const known = failureKeys(before);
  assert.deepEqual(newFailures(known, before), []);

  const after = [message('a', 'Workspace not trusted.'), message('b', '再開できません'), message('c')];
  assert.deepEqual(newFailures(known, after).map((m) => m.id), ['b']);

  const retried = [message('a', '受信ソケットに認証を拒まれました')];
  assert.deepEqual(newFailures(known, retried).map((m) => m.id), ['a']);
});

test('Claude がまだ読んでいないメッセージは、会話に現れた最後のメッセージより後に送ったものに限る', () => {
  const sent = (text: string, createdAt: number, sessionId = 's1'): OutboxMessage => ({ id: text, sessionId, text, createdAt });
  // a は巻き戻して捨てた分岐に宛てたもの。c は送る前の同じ文の依頼と、Claude の発言にしか現れていない。
  const outbox = [sent('a', 1), sent('b', 2), sent('c', 3), sent('x', 4, 's2'), sent('d', 5)];
  const chat: ChatEntry[] = [{ kind: 'prompt', text: 'c', at: 0 }, { kind: 'prompt', text: 'b', at: 2 }, { kind: 'message', text: 'c', at: 4 }];
  assert.deepEqual(unreadMessages(outbox, 's1', chat).map((m) => m.id), ['c', 'd']);
  assert.deepEqual(unreadMessages(outbox, 's1', []).map((m) => m.id), ['a', 'b', 'c', 'd']);
});
