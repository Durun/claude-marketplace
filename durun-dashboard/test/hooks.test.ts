import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
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

  // 質問が複数あるときと複数選べるときは、質問をタブに分けて出す。本文には、端末のセッションの画面のために、質問ごとに選択肢を並べる。
  fs.appendFileSync(transcript, toolUse('AskUserQuestion', { questions: [question, { question: 'どれを直しますか？', header: '直す画面', multiSelect: true, options: [{ label: 'A' }, { label: 'B' }] }] }));
  handleHook({ ...base, notification_type: 'permission_prompt', message: 'Claude needs your permission' }, 0);
  assert.deepEqual(dialogs(), [{ kind: 'question', title: '質問', body: '今すぐ実装してよいですか？\n- 今すぐ実装する\n- 時期を確認してから\n\nどれを直しますか？\n- A\n- B', options: undefined }]);
  assert.deepEqual(state.attention[0]?.questions, [
    { question: '今すぐ実装してよいですか？', header: undefined, options: ['今すぐ実装する', '時期を確認してから'], multiSelect: false },
    { question: 'どれを直しますか？', header: '直す画面', options: ['A', 'B'], multiSelect: true },
  ]);

  handleHook({ ...base, hook_event_name: 'PostToolUse', tool_name: 'AskUserQuestion' }, 0);
  fs.appendFileSync(transcript, toolUse('Bash', { command: 'rm -rf build' }));
  handleHook({ ...base, notification_type: 'permission_prompt', message: 'Claude needs your permission to use Bash' }, 0);
  assert.deepEqual(dialogs(), [{ kind: 'permission', title: '権限の確認', body: 'Claude needs your permission to use Bash', options: undefined }]);
});

test('AskUserQuestion の質問は、会話ログにまだ書かれていなくても、ツールを呼ぶ前のフックから記録し、続く権限の確認の通知やサブエージェントのフックでは消さない', () => {
  state.attention = [];
  const transcript = path.join(tmp, 'late.jsonl');
  fs.writeFileSync(transcript, toolUse('Bash', { command: 'ls' }));
  const base = { session_id: 'late', cwd: tmp, transcript_path: transcript };
  const questions = [{ question: 'どの単位で 1 本にしますか？', header: '切り方', multiSelect: false, options: [{ label: 'A', description: '出だしだけ' }, { label: 'B' }] }];
  const asked = [{ kind: 'question', title: '質問', body: 'どの単位で 1 本にしますか？', options: ['A', 'B'] }];

  handleHook({ ...base, hook_event_name: 'PreToolUse', tool_name: 'AskUserQuestion', tool_input: { questions } }, 0);
  assert.deepEqual(dialogs(), asked);
  handleHook({ ...base, hook_event_name: 'Notification', notification_type: 'permission_prompt', message: 'Claude needs your permission' }, 0);
  assert.deepEqual(dialogs(), asked);

  // 裏で動くサブエージェントが進んでも、権限の確認で止まっても、本体の質問は待ったまま。
  handleHook({ ...base, hook_event_name: 'PostToolUse', tool_name: 'Bash', agent_id: 'a1' }, 0);
  assert.deepEqual(dialogs(), asked);
  handleHook({ ...base, hook_event_name: 'Notification', notification_type: 'worker_permission_prompt', message: 'worker needs your permission' }, 0);
  assert.deepEqual(dialogs(), [...asked, { kind: 'permission', title: '権限の確認（サブエージェント）', body: 'worker needs your permission', options: undefined }]);
  handleHook({ ...base, hook_event_name: 'PostToolUse', tool_name: 'Bash', agent_id: 'a1' }, 0);
  assert.deepEqual(dialogs(), asked);

  handleHook({ ...base, hook_event_name: 'PostToolUse', tool_name: 'AskUserQuestion' }, 0);
  assert.deepEqual(dialogs(), []);
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

  // 会話ログに書かれる前に通知が届いて権限の確認になっていたら、質問に置き換える。
  state.attention = [{ id: 'p1', kind: 'permission', sessionId: 'bg1', title: '権限の確認', body: 'Claude needs your permission', createdAt: 0 }];
  buildSnapshot();
  assert.deepEqual(dialogs(), [{ kind: 'question', title: '質問', body: 'どこで撮りますか？', options: ['stg', 'preview'] }]);

  handleHook({ session_id: 'bg1', cwd: tmp, hook_event_name: 'UserPromptSubmit' }, 0);
  assert.deepEqual(dialogs(), []);
  fs.rmSync(path.join(sessionsDir, `${process.pid}.json`));
});

test('worktree に入って会話ログが別のディレクトリへ移ったら、移った先を探し、移る前の場所を渡すフックが来ても戻さない', async (t) => {
  const { buildSnapshot } = await import('../server/snapshot.ts');
  const live = path.join(tmp, 'claude', 'sessions', 'moved.json');
  fs.mkdirSync(path.dirname(live), { recursive: true });
  fs.writeFileSync(live, JSON.stringify({ pid: process.pid, sessionId: 'moved', cwd: tmp, kind: 'bg', status: 'busy' }));
  t.after(() => fs.rmSync(live));
  const hook = { session_id: 'moved', cwd: tmp, transcript_path: path.join(tmp, 'claude', 'projects', '-repo', 'moved.jsonl'), hook_event_name: 'PostToolUse', tool_name: 'Edit' };
  handleHook(hook, 0);

  const moved = path.join(tmp, 'claude', 'projects', '-repo--claude-worktrees-a', 'moved.jsonl');
  fs.mkdirSync(path.dirname(moved), { recursive: true });
  fs.writeFileSync(moved, '');
  buildSnapshot();
  assert.equal(state.sessions.moved?.transcriptPath, moved);
  handleHook(hook, 0);
  assert.equal(state.sessions.moved?.transcriptPath, moved);
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

test('EnterWorktree は、ほかのセッションがいない worktree なら許可し、いるか入ると決めたばかりなら断り、入る先を決められなければ Claude Code に任せる', (t) => {
  // git が返す場所と比べるので、macOS で /var を指す /private/var の別名を解いておく。
  const repo = path.join(fs.realpathSync(tmp), 'repo');
  execFileSync('git', ['init', '-q', repo]);
  const sub = path.join(repo, 'apps');
  fs.mkdirSync(sub);
  const sessionsDir = path.join(tmp, 'claude', 'sessions');
  fs.mkdirSync(sessionsDir, { recursive: true });
  const live = { 'wt-a': path.join(repo, '.worktrees', 'a', 'src'), 'wt-b': repo, 'wt-c': repo };
  for (const [id, cwd] of Object.entries(live)) fs.writeFileSync(path.join(sessionsDir, `${id}.json`), JSON.stringify({ pid: process.pid, sessionId: id, cwd, status: 'busy' }));
  t.after(() => Object.keys(live).forEach((id) => fs.rmSync(path.join(sessionsDir, `${id}.json`))));
  const enter = (id: string, cwd: string, toolInput: object) =>
    handleHook({ session_id: id, cwd, hook_event_name: 'PreToolUse', tool_name: 'EnterWorktree', tool_input: toolInput }, 0)?.hookSpecificOutput;

  assert.equal(enter('wt-b', repo, { path: '.worktrees/a' })?.permissionDecision, 'deny');
  assert.match(enter('wt-b', repo, { name: 'a' })?.permissionDecisionReason ?? '', /別のセッション「wt-a」/);
  // name の場所は、リポジトリの中のどこから入っても、元の作業ツリーの下に決まる。
  assert.equal(enter('wt-b', sub, { name: 'b' })?.permissionDecision, 'allow');
  // 許可した先は、セッションの cwd が移る前でも、ほかのセッションを入れない。許可されたセッション自身は入り直せる。
  assert.equal(enter('wt-c', repo, { path: path.join(repo, '.claude', 'worktrees', 'b') })?.permissionDecision, 'deny');
  assert.equal(enter('wt-b', repo, { name: 'b' })?.permissionDecision, 'allow');
  assert.equal(enter('wt-c', repo, {})?.permissionDecision, 'allow');
  assert.equal(enter('wt-c', tmp, { name: 'x' }), undefined);
});

test('ブックマークした発言のあるセッションは、アーカイブして会話ログが止まってから日がたっても、ブックマークから開けるよう一覧から外さない', async () => {
  const { buildSnapshot } = await import('../server/snapshot.ts');
  const transcript = path.join(tmp, 'old.jsonl');
  fs.writeFileSync(transcript, '');
  // 一覧から外れる 14 日より前に止まったことにする。
  const longAgo = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000);
  fs.utimesSync(transcript, longAgo, longAgo);
  const meta = Object.assign(ensureSession('old', tmp), { kind: 'bg', archived: true, transcriptPath: transcript });
  const listed = () => buildSnapshot().sessions.find((s) => s.id === 'old');
  assert.equal(listed(), undefined);
  meta.bookmarks = [{ kind: 'message', at: 1, text: '直しました。', createdAt: 2 }];
  assert.deepEqual(listed()?.bookmarks, meta.bookmarks);
});
