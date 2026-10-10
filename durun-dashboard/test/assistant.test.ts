import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import type { Session, Snapshot } from '../shared/types.ts';
import { askDolphin } from '../server/assistant.ts';

test('イルカは説明と状態と会話を渡して答えさせ、ダッシュボードの外へのリンクはボタンにしない', async () => {
  // 標準入力のプロンプトを書き出し、外へのリンクを混ぜた答えを返す偽の claude を置く。
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-assistant-'));
  const promptFile = path.join(tmp, 'prompt');
  const links = [{ label: '開く', href: '#/s/s1' }, { label: '外', href: 'https://example.com' }, { label: '', href: '#/' }];
  useFakeClaude(tmp, `fs.writeFileSync(${js(promptFile)}, stdin()); console.log(${js(JSON.stringify({ structured_output: { answer: ' 入力待ちです ', links } }))});`);
  const session = {
    id: 's1', name: '検索画面', theme: 'dash', repo: 'dash', cwd: tmp, status: 'idle', updatedAt: 0, tasks: [], subagents: [], background: [], jira: [],
    usage: { contextTokens: 0, contextWindow: 1, contextPercent: 12.3, exact: false, totals: { input: 0, output: 0, cacheRead: 0, cacheCreation: 0 }, timeline: [] },
    archived: false, pinned: false, links: [], docOrder: [], hiddenDocs: [], docTitles: {}, pendingMessages: 0, awaitingReview: false, lastMessage: '終わりました',
  } satisfies Session;
  const snap = { now: 0, sessions: [session], attention: [], docs: [], outbox: [], themes: [], sessionOrder: [], improve: { enabled: false, running: false, pending: 0 }, jiraRefreshedAt: 0, assets: { js: 0, css: 0 } } satisfies Snapshot;

  const reply = await askDolphin(snap, [{ role: 'human', text: '何か待ってる？' }, { role: 'dolphin', text: 'はい' }, { role: 'human', text: 'どれ？' }]);
  assert.deepEqual(reply, { answer: '入力待ちです', links: [{ label: '開く', href: '#/s/s1' }] });
  const prompt = fs.readFileSync(promptFile, 'utf8');
  assert.match(prompt, /<guide>\n# durun-dashboard/);
  assert.match(prompt, /"name":"検索画面".*"lastMessage":"終わりました"/);
  assert.match(prompt, /<conversation>\n人間: 何か待ってる？\n\nイルカ: はい\n\n人間: どれ？\n<\/conversation>$/);
});
