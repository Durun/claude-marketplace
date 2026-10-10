import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Attention, Session, SessionStatus, Snapshot } from '../shared/types.ts';
import { sessionEvents, sessionsSeen } from '../client/sessionEvents.ts';

const session = (id: string, status: SessionStatus, archived = false) => ({ id, name: id, status, archived }) as Session;
const ask = (id: string, sessionId: string, kind: Attention['kind'] = 'ask'): Attention => ({ id, kind, sessionId, title: 'どちらにしますか', createdAt: 0 });
const snapshot = (sessions: Session[], attention: Attention[] = []) => ({ sessions, attention }) as Snapshot;

test('作業中から止まったセッションを完了として知らせ、前から止まっていたものと、新しく現れたものは知らせない', () => {
  const seen = sessionsSeen(snapshot([session('a', 'busy'), session('b', 'idle'), session('c', 'busy'), session('d', 'busy')]));
  const events = sessionEvents(seen, snapshot([session('a', 'idle'), session('b', 'idle'), session('c', 'done'), session('d', 'busy'), session('e', 'idle')]));
  assert.deepEqual(events.map((e) => [e.kind, e.session.id]), [['done', 'a'], ['done', 'c']]);
  assert.equal(events[0]?.text, 'a が完了しました');
});

test('答えを待ち始めたセッションは、止まっても待っていることだけを知らせ、前から待っていた質問は知らせ直さない', () => {
  const seen = sessionsSeen(snapshot([session('a', 'busy'), session('b', 'busy')], [ask('q1', 'b')]));
  const events = sessionEvents(seen, snapshot([session('a', 'idle'), session('b', 'busy')], [ask('q1', 'b'), ask('q2', 'a', 'permission'), ask('r1', 'a', 'reply')]));
  assert.deepEqual(events.map((e) => [e.kind, e.session.id, e.attention?.id]), [['ask', 'a', 'q2']]);
  assert.equal(events[0]?.text, 'a が権限の確認を待っています: どちらにしますか');
});

test('アーカイブしたセッションは知らせない', () => {
  const seen = sessionsSeen(snapshot([session('a', 'busy')]));
  assert.deepEqual(sessionEvents(seen, snapshot([session('a', 'done', true)], [ask('q1', 'a')])), []);
});
