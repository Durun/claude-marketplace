import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 実際の会話ログや状態に触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-search-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { searchSessions } = await import('../server/search.ts');

const writeLog = (id: string, entries: object[]): void => {
  const dir = path.join(tmp, 'claude', 'projects', '-work-repo');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, `${id}.jsonl`), entries.map((e) => `${JSON.stringify(e)}\n`).join(''));
};
const prompt = (text: string, timestamp: string) => ({ type: 'user', origin: { kind: 'human' }, cwd: '/work/repo', timestamp, message: { content: text } });
const reply = (text: string, timestamp: string) => ({ type: 'assistant', cwd: '/work/repo', timestamp, message: { content: [{ type: 'text', text }] } });

test('すべての語を含む題名、依頼、発言を探し、同じ文はまとめて、新しいセッションから並べる', async () => {
  writeLog('old', [{ type: 'ai-title', aiTitle: 'Socket の調査' }, prompt('ソケットで配信して', '2026-10-01T00:00:00Z'), prompt('ソケットで配信して', '2026-10-01T00:01:00Z')]);
  writeLog('new', [prompt('配信の経路を見直す', '2026-10-05T00:00:00Z'), reply('ソケットへ送ります。'.padStart(300, '前置き'), '2026-10-05T00:02:00Z')]);

  const both = await searchSessions('ソケット 配信');
  assert.deepEqual(both.results.map((r) => [r.id, r.repo, r.hitCount, r.hits.map((h) => h.kind)]), [['old', 'repo', 1, ['prompt']]]);

  const title = await searchSessions('SOCKET');
  assert.deepEqual(title.results.map((r) => [r.id, r.title, r.hits.map((h) => h.kind)]), [['old', 'Socket の調査', ['title']]]);

  const either = await searchSessions('ソケット');
  assert.deepEqual(either.results.map((r) => r.id), ['new', 'old']);
  assert.match(either.results[0]?.hits[0]?.text ?? '', /^….*ソケットへ送ります。$/);
});
