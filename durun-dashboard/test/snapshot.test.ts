import assert from 'node:assert/strict';
import path from 'node:path';
import { test } from 'node:test';
import type { PrInfo, Session } from '../shared/types.ts';
import { tempDir } from './tmp.ts';

// 実際の状態やセッションに触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = tempDir('durun-dashboard-snapshot-');
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { deriveAttention } = await import('../server/snapshot.ts');
const { state } = await import('../server/store.ts');

const url = 'https://github.com/o/r/pull/1';
const ciFailed = `pr:${url}:ci-failed`;
const failing: PrInfo = { url, number: 1, title: 't', state: 'OPEN', isDraft: false, review: '', checks: { passed: 1, failed: 2, pending: 0 } };
const passing: PrInfo = { ...failing, checks: { passed: 3, failed: 0, pending: 0 } };
const session = (id: string, pr: PrInfo, archived = false): Session => ({ id, pr, archived, updatedAt: 0 }) as Session;
const ids = (sessions: Session[]): string[] => deriveAttention(sessions).map((a) => a.id);

test('同じ PR をアーカイブしたセッションも持つとき、既読にした CI の失敗を出し直さない', () => {
  state.dismissed = [ciFailed];
  assert.deepEqual(ids([session('archived', failing, true), session('open', failing)]), []);
  assert.deepEqual(state.dismissed, [ciFailed]);
});

test('CI が通ったら既読印を消し、次に失敗したときにまた知らせる', () => {
  state.dismissed = [ciFailed];
  assert.deepEqual(ids([session('open', passing)]), []);
  assert.deepEqual(state.dismissed, []);
  assert.deepEqual(ids([session('open', failing)]), [ciFailed]);
});
