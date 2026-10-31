import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import type { Session, Snapshot } from '../shared/types.ts';
import { askDolphin } from '../server/assistant.ts';

test('イルカは説明と状態と会話を渡して答えさせ、ダッシュボードの外へのリンクと無いセッションへの指示は返さない', async () => {
  // 標準入力のプロンプトを書き出し、外へのリンクを混ぜた答えを返す偽の claude を置く。
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-assistant-'));
  const promptFile = path.join(tmp, 'prompt');
  const links = [
    { label: '開く', href: '#/s/s1', open: true },
    { label: '外', href: 'https://example.com', open: true },
    { label: '', href: '#/', open: false },
    { label: 'PR', href: 'https://github.com/o/r/pull/12', open: false },
    { label: '課題', href: 'https://jira.example.com/browse/WEB-3', open: 'yes' },
    { label: '別の JIRA', href: 'https://other.example.com/browse/WEB-3', open: false },
  ];
  const sends = [{ sessionId: 's1', text: ' テストを足して ' }, { sessionId: 'nope', text: '止めて' }, { sessionId: 's1', text: ' ' }];
  useFakeClaude(tmp, `fs.writeFileSync(${js(promptFile)}, stdin()); console.log(${js(JSON.stringify({ structured_output: { answer: ' 入力待ちです ', links, sends } }))});`);
  const session = {
    id: 's1', name: '検索画面', theme: 'dash', repo: 'dash', cwd: tmp, status: 'idle', updatedAt: 0, tasks: [], subagents: [], background: [], prs: [], jira: [],
    usage: { contextTokens: 0, contextWindow: 1, contextPercent: 12.3, exact: false, totals: { input: 0, output: 0, cacheRead: 0, cacheCreation: 0 }, timeline: [] },
    archived: false, pinned: false, links: [], files: [], docOrder: [], hiddenDocs: [], docTitles: {}, pinnedDocs: [], artifacts: [], pendingMessages: 0, awaitingReview: false, lastMessage: '終わりました',
  } satisfies Session;
  const snap = { now: 0, sessions: [session], attention: [], docs: [], outbox: [], themes: [], sessionOrder: [], improve: { enabled: false, running: false, pending: 0 }, jiraRefreshedAt: 0, jiraBase: 'https://jira.example.com', assets: { js: 0, css: 0 } } satisfies Snapshot;

  const reply = await askDolphin(snap, [{ role: 'human', text: '何か待ってる？' }, { role: 'dolphin', text: 'はい' }, { role: 'human', text: 'どれ？' }]);
  assert.deepEqual(reply, {
    answer: '入力待ちです',
    links: [
      { label: '開く', href: '#/s/s1', open: true },
      { label: 'PR', href: 'https://github.com/o/r/pull/12', open: false },
      { label: '課題', href: 'https://jira.example.com/browse/WEB-3', open: false },
    ],
    sends: [{ sessionId: 's1', name: '検索画面', text: 'テストを足して' }],
  });
  const prompt = fs.readFileSync(promptFile, 'utf8');
  assert.match(prompt, /<guide>\n# durun-dashboard/);
  assert.match(prompt, /"name":"検索画面".*"lastMessage":"終わりました"/);
  assert.match(prompt, /<conversation>\n人間: 何か待ってる？\n\nイルカ: はい\n\n人間: どれ？\n<\/conversation>$/);
  assert.doesNotMatch(prompt, /<focus>\n/);

  // 開いているセッションがあれば、その概要と、道具の呼び出しを除いた直近の会話を添える。
  const chat = [{ kind: 'prompt', text: '調べて' }, { kind: 'tool', text: 'Bash' }, { kind: 'message', text: '質問です' }] as const;
  await askDolphin(snap, [{ role: 'human', text: 'この質問は？' }], { name: '検索画面', summary: 'いま: 回答待ち', chat: [...chat] });
  assert.match(fs.readFileSync(promptFile, 'utf8'), /<focus>\nセッション: 検索画面\nいま: 回答待ち\n直近の会話:\n人間: 調べて\n\nClaude: 質問です\n<\/focus>/);
});
