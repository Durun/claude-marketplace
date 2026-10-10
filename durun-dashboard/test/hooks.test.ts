import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態に触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-durun-dashboard-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { handleHook } = await import('../server/hooks.ts');
const { ensureSession, onChange, state } = await import('../server/store.ts');

const toolUse = (name: string, input: object): string => `${JSON.stringify({ type: 'assistant', message: { content: [{ type: 'tool_use', name, input }] } })}\n`;
const dialogs = () => state.attention.map(({ kind, title, body, options }) => ({ kind, title, body, options }));

test('権限の確認の通知は、AskUserQuestion で止まったときは質問として記録し、別のセッションの入力待ちの知らせは記録しない', () => {
  const transcript = path.join(tmp, 's1.jsonl');
  const base = { session_id: 's1', cwd: tmp, transcript_path: transcript, hook_event_name: 'Notification' };
  const question = { question: '今すぐ実装してよいですか？', multiSelect: false, options: [{ label: '今すぐ実装する' }, { label: '時期を確認してから' }] };
  fs.writeFileSync(transcript, toolUse('AskUserQuestion', { questions: [question] }));

  handleHook({ ...base, notification_type: 'agent_needs_input', message: 'other needs your input: answer: ...' }, 0);
  assert.deepEqual(dialogs(), []);
  handleHook({ ...base, notification_type: 'permission_prompt', message: 'Claude needs your permission' }, 0);
  assert.deepEqual(dialogs(), [{ kind: 'question', title: '質問', body: '今すぐ実装してよいですか？', options: ['今すぐ実装する', '時期を確認してから'] }]);

  // Claude Code の画面で答えると、ツールの実行を終えたフックで片付ける。
  handleHook({ ...base, hook_event_name: 'PostToolUse', tool_name: 'AskUserQuestion' }, 0);
  assert.deepEqual(dialogs(), []);

  // 質問が複数あるときと複数選べるときは、ボタンにせず、質問ごとに選択肢を本文に並べる。
  fs.appendFileSync(transcript, toolUse('AskUserQuestion', { questions: [question, { question: 'どれを直しますか？', multiSelect: true, options: [{ label: 'A' }, { label: 'B' }] }] }));
  handleHook({ ...base, notification_type: 'permission_prompt', message: 'Claude needs your permission' }, 0);
  assert.deepEqual(dialogs(), [{ kind: 'question', title: '質問', body: '今すぐ実装してよいですか？\n- 今すぐ実装する\n- 時期を確認してから\n\nどれを直しますか？\n- A\n- B', options: undefined }]);

  fs.appendFileSync(transcript, toolUse('Bash', { command: 'rm -rf build' }));
  handleHook({ ...base, notification_type: 'permission_prompt', message: 'Claude needs your permission to use Bash' }, 0);
  assert.deepEqual(dialogs(), [{ kind: 'permission', title: '権限の確認', body: 'Claude needs your permission to use Bash', options: undefined }]);
});

test('バックグラウンドのセッションが AskUserQuestion で入力待ちになったら、通知のフックが来なくても会話ログから質問を記録し、次のフックで片付ける', async () => {
  const { buildSnapshot } = await import('../server/snapshot.ts');
  state.attention = [];
  const transcript = path.join(tmp, 'bg.jsonl');
  const question = { question: 'どこで撮りますか？', multiSelect: false, options: [{ label: 'stg' }, { label: 'preview' }] };
  fs.writeFileSync(transcript, toolUse('AskUserQuestion', { questions: [question] }));
  const sessionsDir = path.join(tmp, 'claude', 'sessions');
  fs.mkdirSync(sessionsDir, { recursive: true });
  fs.writeFileSync(path.join(sessionsDir, `${process.pid}.json`), JSON.stringify({ pid: process.pid, sessionId: 'bg1', cwd: tmp, kind: 'bg', status: 'waiting' }));
  ensureSession('bg1', tmp).transcriptPath = transcript;

  buildSnapshot();
  buildSnapshot();
  assert.deepEqual(dialogs(), [{ kind: 'question', title: '質問', body: 'どこで撮りますか？', options: ['stg', 'preview'] }]);

  handleHook({ session_id: 'bg1', cwd: tmp, hook_event_name: 'UserPromptSubmit' }, 0);
  assert.deepEqual(dialogs(), []);
  fs.rmSync(path.join(sessionsDir, `${process.pid}.json`));
});

test('名前の無いセッションには、Claude の最初の応答が終わったときに haiku が名前を付け、人間が付けた名前は変えない', async () => {
  // 偽の claude は、概要と名前のどちらを頼まれても同じ答えを返す。
  const answer = JSON.stringify({ structured_output: { title: 'ログインの修正', doing: 'ログインを直す', now: '直し終えた' } });
  useFakeClaude(tmp, `stdin(); console.log(${js(answer)});`);
  const transcript = path.join(tmp, 's2.jsonl');
  fs.writeFileSync(transcript, `${JSON.stringify({ type: 'user', origin: { kind: 'human' }, message: { content: 'ログインを直して' } })}\n`);
  const started = { kind: 'bg', entrypoint: 'cli', startedAt: Date.now() };
  const named = Object.assign(ensureSession('s3', tmp), started, { title: '手で付けた名前', transcriptPath: transcript });
  const meta = Object.assign(ensureSession('s2', tmp), started);
  const stop = { session_id: 's2', cwd: tmp, transcript_path: transcript, hook_event_name: 'Stop', last_assistant_message: '直しました。' };

  // 応答の途中では、始まって 5 分経つまで付けない。
  handleHook({ ...stop, hook_event_name: 'PostToolUse' }, 0);
  handleHook({ ...stop, session_id: 's3' }, 0);
  assert.equal(meta.title, undefined);
  handleHook(stop, 0);
  await new Promise<void>((resolve) => onChange(() => meta.title && resolve()));
  assert.equal(meta.title, 'ログインの修正');
  assert.equal(named.title, '手で付けた名前');
});
