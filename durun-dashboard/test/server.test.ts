import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { summarizeLog } from '../server/claude.ts';
import { formatReview } from '../server/deliver.ts';
import { countChecks, gitDiff, parseDiff, repoFile, resolveRange } from '../server/integrations.ts';
import { toMemoryStatus } from '../server/memory.ts';
import { type BranchView, WORKTREE } from '../shared/types.ts';
import { renameTheme, stoppedStatus } from '../server/snapshot.ts';

test('summarizeLog は、会話ログに載った skills の一覧を、後から届いた分も含めて名前ごとに集め、説明の書き出しを添える', () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-skills-')), 'log.jsonl');
  const listing = (names: string[], content: string): string => JSON.stringify({ type: 'attachment', attachment: { type: 'skill_listing', names, content } });
  fs.writeFileSync(file, `${listing(['code-review:comment-review', 'code-review', 'pdf'], '- code-review:comment-review: コメントを見る\n- code-review: 差分を見る\n  続きの行\n- pdf')}\n${listing(['extra'], '- extra: 増えた分')}\n`);
  assert.deepEqual([...summarizeLog(file).skills], [
    ['code-review:comment-review', 'コメントを見る'],
    ['code-review', '差分を見る'],
    ['pdf', ''],
    ['extra', '増えた分'],
  ]);
});

test('summarizeLog は、claude.ai に公開した artifact の題名と元のファイルを URL ごとに集め、公開し直したら新しいほうを残す', () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-artifacts-')), 'log.jsonl');
  const link = (title: string, p: string): string => JSON.stringify({ type: 'frame-link', frameUrl: 'https://claude.ai/artifact/abc', title, path: p });
  fs.writeFileSync(file, `${link('旧題', '/tmp/a.html')}\n${link('新題', '/tmp/b.html')}\n`);
  assert.deepEqual([...summarizeLog(file).artifacts.values()], [{ url: 'https://claude.ai/artifact/abc', title: '新題', path: '/tmp/b.html' }]);
});

test('summarizeLog は、居たブランチを最後に居た順に並べ、戻ってきたブランチは末尾へ移す', () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-branches-')), 'log.jsonl');
  const on = (gitBranch: string): string => `${JSON.stringify({ type: 'user', gitBranch, message: { content: '' } })}\n`;
  fs.writeFileSync(file, ['a', 'b', 'b', 'a', 'c', 'b'].map(on).join(''));
  const log = summarizeLog(file);
  assert.deepEqual([...log.branches], ['a', 'c', 'b']);
  assert.equal(log.branch, 'b');
});

test('summarizeLog は、バックグラウンドの shell と Monitor を、ツールの結果でタスクの id と結びつけ、終わった通知で状態を更新する', () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-bg-')), 'log.jsonl');
  const call = (id: string, name: string, input: object) => ({ type: 'assistant', message: { content: [{ type: 'tool_use', id, name, input }] } });
  const result = (id: string, text: string, toolUseResult: object) => ({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: id, content: text }] }, toolUseResult });
  const notify = (body: string) => ({ type: 'user', message: { content: `<task-notification>\n${body}\n</task-notification>` } });
  fs.writeFileSync(file, `${[
    call('u1', 'Bash', { command: 'git push', description: 'push する', run_in_background: true }),
    call('u2', 'Bash', { command: 'ls' }),
    call('u3', 'Monitor', { command: 'tail -f log', description: 'ログを見る' }),
    result('u1', 'Command running in background with ID: b1. Output is being written to: /tmp/x/b1.output. You will be notified', { backgroundTaskId: 'b1' }),
    result('u3', 'Monitor started (task m1)', { taskId: 'm1' }),
    notify('<task-id>m1</task-id>\n<summary>Monitor event</summary>\n<event>line</event>'),
    notify('<task-id>b1</task-id>\n<status>failed</status>\n<summary>Background command "push する" failed</summary>'),
  ].map((e) => JSON.stringify(e)).join('\n')}\n`);
  assert.deepEqual([...summarizeLog(file).bgTasks.values()].map(({ id, kind, description, status, outputFile }) => ({ id, kind, description, status, outputFile })), [
    { id: 'b1', kind: 'shell', description: 'push する', status: 'failed', outputFile: '/tmp/x/b1.output' },
    { id: 'm1', kind: 'monitor', description: 'ログを見る', status: 'running', outputFile: undefined },
  ]);
});

test('parseDiff は行番号と変更の種類を取り出す', () => {
  const diff = [
    'diff --git a/src/a.ts b/src/a.ts',
    'index 1111111..2222222 100644',
    '--- a/src/a.ts',
    '+++ b/src/a.ts',
    '@@ -10,3 +10,3 @@ fn',
    ' keep',
    '-old',
    '+new',
    '--- not a header',
    ' tail',
    'diff --git a/gone.txt b/gone.txt',
    'deleted file mode 100644',
    '--- a/gone.txt',
    '+++ /dev/null',
    '@@ -1 +0,0 @@',
    '-bye',
    'diff --git a/old name.md b/new name.md',
    'similarity index 90%',
    'rename from old name.md',
    'rename to new name.md',
    'diff --git a/img.png b/img.png',
    'Binary files a/img.png and b/img.png differ',
    '',
  ].join('\n');
  const [a, gone, renamed, img] = parseDiff(diff);
  assert.equal(a?.path, 'src/a.ts');
  assert.deepEqual(
    a?.hunks[0]?.lines.map((l) => [l.kind, l.oldNo, l.newNo]),
    [
      ['ctx', 10, 10],
      ['del', 11, undefined],
      ['add', undefined, 11],
      ['del', 12, undefined],
      ['ctx', 13, 12],
    ],
  );
  assert.equal(a?.additions, 1);
  assert.equal(a?.deletions, 2);
  assert.equal(gone?.status, 'deleted');
  assert.equal(gone?.path, 'gone.txt');
  assert.deepEqual([renamed?.status, renamed?.oldPath, renamed?.path], ['renamed', 'old name.md', 'new name.md']);
  assert.equal(img?.binary, true);
});

test('gitDiff はコミットの範囲の差分を返し、repoFile は差分に無いファイルを git が無視しないものに限って読む。ブランチを渡されたら、そのブランチから出す', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-git-'));
  const git = (...args: string[]): string => execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
  const commit = (files: Record<string, string>, subject: string): string => {
    for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), text);
    git('add', '-A');
    git('-c', 'user.name=t', '-c', 'user.email=t@example.com', '-c', 'commit.gpgsign=false', 'commit', '-q', '--no-verify', '-m', subject);
    return git('rev-parse', 'HEAD');
  };
  git('init', '-q');
  const c1 = commit({ 'a.ts': 'one\n', '.gitignore': '.env\n' }, '一つ目');
  const c2 = commit({ 'a.ts': 'two\n', 'b.ts': 'b\n' }, '二つ目');
  const c3 = commit({ 'a.ts': 'three\n' }, '三つ目');
  git('checkout', '-q', '-b', 'feat', c1);
  const f1 = commit({ 'c.ts': 'c1\n' }, 'ブランチの一つ目');
  const f2 = commit({ 'c.ts': 'c2\n' }, 'ブランチの二つ目');
  git('checkout', '-q', '-');
  fs.writeFileSync(path.join(dir, 'a.ts'), 'four\n');
  fs.writeFileSync(path.join(dir, '.env'), 'SECRET=1\n');
  const diff = (want = {}, view?: BranchView) => gitDiff(dir, undefined, false, new Set(), want, view);
  const texts = (files: Awaited<ReturnType<typeof diff>>['files']) => files.map((f) => [f.path, f.hunks.flatMap((h) => h.lines.map((l) => `${l.kind}:${l.text}`))]);

  // 分岐点の無いリポジトリでは、範囲を渡されなければ未コミットの変更だけを出す。
  const plain = await diff();
  assert.deepEqual(plain.range, { from: WORKTREE, to: WORKTREE });
  assert.deepEqual(plain.commits, [{ sha: c3, subject: '三つ目' }, { sha: c2, subject: '二つ目' }, { sha: c1, subject: '一つ目' }]);
  assert.deepEqual(texts(plain.files), [['a.ts', ['del:three', 'add:four']]]);
  // from と to のコミットを含める。未コミットの変更は、to が WORKTREE のときに限る。
  assert.deepEqual(texts((await diff({ from: c2, to: c3 })).files), [['a.ts', ['del:one', 'add:three']], ['b.ts', ['add:b']]]);
  assert.deepEqual(texts((await diff({ from: c3, to: WORKTREE })).files), [['a.ts', ['del:two', 'add:four']]]);
  // 最初のコミットからなら、何も無いところからの差分になる。
  assert.deepEqual(texts((await diff({ from: c1, to: c1 })).files), [['.gitignore', ['add:.env']], ['a.ts', ['add:one']]]);
  await assert.rejects(diff({ from: '--output=x', to: c3 }), /コミットが見つかりません/);

  assert.deepEqual(await resolveRange(dir, 'HEAD~1', 'HEAD'), { from: c2, to: c3 });
  await assert.rejects(resolveRange(dir, 'HEAD', 'HEAD~1'), /古いコミット/);

  const whole = await repoFile(dir, `${path.join(dir, 'b.ts')}:1`);
  assert.deepEqual([whole?.status, texts(whole ? [whole] : [])], ['unchanged', [['b.ts', ['ctx:b']]]]);
  assert.equal((await repoFile(dir, 'a.ts'))?.hunks[0]?.lines[0]?.text, 'four');
  assert.deepEqual(await Promise.all(['.env', '../a.ts', 'missing.ts'].map((ref) => repoFile(dir, ref))), [undefined, undefined, undefined]);

  // ブランチは、比べる相手との分岐点から先端までを出す。範囲の端の WORKTREE は、ブランチの先端に読み替える。
  const branch = await diff({}, { branch: 'feat', base: c3 });
  assert.deepEqual(branch.range, { from: f1, to: f2 });
  assert.deepEqual(branch.commits.map((c) => c.sha), [f2, f1, c1]);
  assert.deepEqual(texts(branch.files), [['c.ts', ['add:c2']]]);
  assert.deepEqual((await diff({ from: f1, to: WORKTREE }, { branch: 'feat' })).range, { from: f1, to: f2 });
  // 比べる相手が無く、origin の既定のブランチも無ければ、先端のコミットだけにする。
  assert.deepEqual(texts((await diff({}, { branch: 'feat' })).files), [['c.ts', ['del:c1', 'add:c2']]]);
  await assert.rejects(diff({}, { branch: 'missing' }), /コミットが見つかりません/);
  // ブランチのファイルは、作業ツリーに無くても、作業ツリーと中身が違っても、そのブランチから読む。
  assert.deepEqual(texts(await Promise.all(['c.ts', 'a.ts'].map((ref) => repoFile(dir, ref, 'feat'))).then((files) => files.flatMap((f) => f ?? []))), [['c.ts', ['ctx:c2']], ['a.ts', ['ctx:one']]]);
  assert.deepEqual(await Promise.all(['../a.ts', 'missing.ts'].map((ref) => repoFile(dir, ref, 'feat'))), [undefined, undefined]);
  fs.rmSync(dir, { recursive: true });
});

test('countChecks は CheckRun と StatusContext をまとめて数える', () => {
  const checks = countChecks([
    { status: 'COMPLETED', conclusion: 'SUCCESS' },
    { status: 'COMPLETED', conclusion: 'SKIPPED' },
    { status: 'COMPLETED', conclusion: 'FAILURE' },
    { status: 'IN_PROGRESS', conclusion: '' },
    { state: 'PENDING' },
    { state: 'ERROR' },
  ]);
  assert.deepEqual(checks, { passed: 2, failed: 2, pending: 2 });
});

test('summarizeLog は同じ応答の usage を二重に数えず、書きかけの行は次回に読む', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-test-'));
  const file = path.join(dir, 's.jsonl');
  const usage = { input_tokens: 10, output_tokens: 5, cache_read_input_tokens: 100, cache_creation_input_tokens: 20 };
  const entry = (o: object): string => `${JSON.stringify(o)}\n`;
  const assistant = (id: string, content: object[], ts: string): string =>
    entry({ type: 'assistant', timestamp: ts, gitBranch: 'feature/x', message: { id, model: 'claude-opus-5-5', usage, content } });
  fs.writeFileSync(
    file,
    entry({ type: 'ai-title', aiTitle: 'テスト' }) +
      entry({ type: 'user', origin: { kind: 'human' }, message: { content: '直して' } }) +
      entry({ type: 'last-prompt', lastPrompt: null }) +
      entry({ type: 'user', origin: { kind: 'task-notification' }, message: { content: '<task-notification>' } }) +
      assistant('m1', [{ type: 'thinking' }], '2026-10-06T00:00:00Z') +
      assistant('m1', [{ type: 'tool_use', name: 'Edit', input: { file_path: '/repo/a.ts' } }], '2026-10-06T00:00:01Z') +
      entry({ type: 'assistant', isSidechain: true, message: { id: 'side', usage, content: [{ type: 'text', text: 'サブ' }] } }) +
      '{"type":"assistant","message":{"id":"m2"',
  );
  const first = summarizeLog(file);
  assert.equal(first.title, 'テスト');
  assert.equal(first.lastPrompt, '直して');
  assert.equal(first.branch, 'feature/x');
  assert.deepEqual(first.totals, { input: 10, output: 5, cacheRead: 100, cacheCreation: 20 });
  assert.equal(first.contextTokens, 130);
  assert.equal(first.lastToolUse, 'Edit');
  assert.deepEqual([...first.editedFiles], ['/repo/a.ts']);
  assert.equal(first.timeline.length, 1);

  fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace(/\{"type":"assistant","message":\{"id":"m2"$/, ''));
  fs.appendFileSync(file, assistant('m2', [{ type: 'text', text: '完了しました' }], '2026-10-06T00:01:00Z'));
  const second = summarizeLog(file);
  assert.equal(second.totals.output, 10);
  assert.equal(second.lastMessage, '完了しました');
  assert.equal(second.lastToolUse, undefined);
  assert.equal(second.timeline.length, 2);
  fs.rmSync(dir, { recursive: true });
});

test('summarizeLog は、呼んだツールを名前と入力の要点の 1 行にして会話に入れ、入力の全体を detail に持つ', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-test-'));
  const file = path.join(dir, 's.jsonl');
  const content = [{ type: 'text', text: '調べます' }, { type: 'tool_use', name: 'Bash', input: { command: 'ls\npwd' } }, { type: 'tool_use', name: 'TodoWrite', input: { todos: [] } }];
  fs.writeFileSync(file, `${JSON.stringify({ type: 'assistant', message: { content } })}\n`);
  assert.deepEqual(summarizeLog(file).texts.map((t) => [t.kind, t.text, t.detail]), [
    ['message', '調べます', undefined],
    ['tool', 'Bash ls', '{\n  "command": "ls\\npwd"\n}'],
    ['tool', 'TodoWrite', '{\n  "todos": []\n}'],
  ]);
  fs.rmSync(dir, { recursive: true });
});

test('summarizeLog は、人が打った依頼と claude -p で渡した依頼とダッシュボードから届いた依頼を会話に入れ、前置きと閉じタグと注意書きを外す', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-test-'));
  const file = path.join(dir, 's.jsonl');
  const user = (o: object): string => `${JSON.stringify({ type: 'user', ...o })}\n`;
  fs.writeFileSync(
    file,
    user({ origin: { kind: 'human' }, message: { content: '直して' } }) +
      user({ promptSource: 'sdk', message: { content: '-p で渡した' } }) +
      user({ promptSource: 'sdk', message: { content: '[durun-dashboard] 人間から:\n前置きつきで -p で渡した' } }) +
      user({ isMeta: true, message: { content: 'Stop hook feedback:\n[durun-dashboard] 改善要望です' } }) +
      user({ origin: { kind: 'peer' }, isMeta: true, message: { content: 'Another Claude session sent a message: <agent-message from="a">[durun-dashboard] 送りました\n---\n続けて</agent-message>\n返信は不要' } }) +
      user({ origin: { kind: 'peer' }, isMeta: true, message: { content: 'Another Claude session sent a message: <agent-message from="b">[Subagent hand-back] 報告</agent-message>' } }) +
      user({ origin: { kind: 'peer' }, isMeta: true, message: { content: 'Another Claude session sent a message:\n[durun-dashboard] 人間から:\nもう一度\n\nThis came from another Claude session — not typed by your user.' } }) +
      user({ isMeta: true, message: { content: [{ type: 'text', text: 'Base directory for this skill' }] } }) +
      user({ message: { content: [{ type: 'tool_result', content: '[durun-dashboard] を含む出力' }] } }),
  );
  const log = summarizeLog(file);
  assert.deepEqual(log.texts.map((t) => t.text), ['直して', '-p で渡した', '前置きつきで -p で渡した', '[durun-dashboard] 改善要望です', '[durun-dashboard] 送りました\n---\n続けて', 'もう一度']);
  assert.equal(log.lastPrompt, 'もう一度');
  fs.rmSync(dir, { recursive: true });
});

test('summarizeLog は、巻き戻して送った依頼が来たら、捨てられた分岐の発言を会話から外し、依頼に巻き戻す先を持たせる', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-test-'));
  const file = path.join(dir, 's.jsonl');
  const entry = (o: object): string => `${JSON.stringify(o)}\n`;
  const ask = (uuid: string, parentUuid: string, content: string): string => entry({ type: 'user', uuid, parentUuid, origin: { kind: 'human' }, message: { content } });
  const say = (uuid: string, parentUuid: string, text: string): string => entry({ type: 'assistant', uuid, parentUuid, message: { content: [{ type: 'text', text }] } });
  fs.writeFileSync(
    file,
    entry({ type: 'attachment', uuid: 'a0', parentUuid: null }) +
      ask('u1', 'a0', 'A') + say('m1', 'u1', 'a') +
      ask('u2', 'm1', 'B') + say('m2', 'u2', 'b') +
      ask('u3', 'm2', 'C') + say('m3', 'u3', 'c') +
      // 巻き戻して送る依頼は -p で渡すので、origin を持たない。
      entry({ type: 'user', uuid: 'u4', parentUuid: 'm1', promptSource: 'sdk', message: { content: 'B2' } }) + say('m4', 'u4', 'b2'),
  );
  const log = summarizeLog(file);
  assert.deepEqual(log.texts.map((t) => t.text), ['A', 'a', 'B2', 'b2']);
  assert.deepEqual(log.texts.map((t) => t.rewindTo), ['a0', undefined, 'm1', undefined]);
  fs.rmSync(dir, { recursive: true });
});

test('formatReview は指摘を場所と該当行つきで並べる', () => {
  const text = formatReview([{ path: 'a.ts', line: 3, side: 'new', lineText: '  const x = 1;', body: '名前を具体的に' }], '全体は良い');
  assert.match(text, /全体は良い/);
  assert.match(text, /1\. a\.ts:3\n {3}> const x = 1;\n {3}名前を具体的に/);
  assert.equal(formatReview([], ''), '指摘はありません。');
  // ブランチに付けたコメントは、作業ツリーのファイルと取り違えないよう、どのブランチの行かを添える。
  assert.match(formatReview([{ path: 'a.ts', line: 3, side: 'new', lineText: 'x', body: 'y' }], '', 'origin/feat'), /^ダッシュボードで origin\/feat の差分に付けたコメントです。行番号は origin\/feat のファイルのものです。/);
});

test('止まったセッションは、応答を返し終えたかとアーカイブで状態が分かれる', () => {
  assert.equal(stoppedStatus(false, { lastMessage: '済みました' }), 'done');
  assert.equal(stoppedStatus(false, { lastMessage: '直します', lastToolUse: 'Edit' }), 'interrupted');
  assert.equal(stoppedStatus(false, undefined), 'interrupted');
  assert.equal(stoppedStatus(true, { lastMessage: '済みました' }), 'ended');
});

test('renameTheme は、リポジトリ名がテーマになっているセッションも含めて、画面で同じテーマにまとまるセッションを新しい名前へ移す', () => {
  const meta = (cwd: string, theme?: string) => ({ id: cwd, cwd, theme, jira: [], archived: false, firstSeen: 0, replySeenAt: 0 });
  const sessions = [meta('/w/some-repository'), meta('/w/some-repository/.worktrees/x'), meta('/w/other', ' some-repository '), meta('/w/some-repository', 'engage'), meta('/w/other')];
  renameTheme(sessions, 'some-repository', 'サンプル');
  assert.deepEqual(sessions.map((s) => s.theme), ['サンプル', 'サンプル', 'サンプル', 'engage', undefined]);
});

test('toMemoryStatus は、sysctl と vm_stat の出力から、メモリプレッシャーの段階と、アクティビティモニタと同じ内訳を求める', () => {
  const sysctl = '2\n35\n25769803776\ntotal = 2048.00M  used = 476.12M  free = 1571.88M  (encrypted)\n';
  const vmStat = [
    'Mach Virtual Memory Statistics: (page size of 16384 bytes)',
    'Pages free:                                     5620.',
    'Pages wired down:                             198444.',
    'Pages purgeable:                               22385.',
    '"Translation faults":                     1036478404.',
    'File-backed pages:                            254559.',
    'Anonymous pages:                              545064.',
    'Pages occupied by compressor:                 531761.',
    '',
  ].join('\n');
  const page = 16384;
  assert.deepEqual(toMemoryStatus(sysctl, vmStat), {
    kind: 'pressure',
    level: 'warn',
    percent: 65,
    total: 25769803776,
    app: (545064 - 22385) * page,
    wired: 198444 * page,
    compressed: 531761 * page,
    cached: (254559 + 22385) * page,
    swapUsed: 476.12 * 1024 * 1024,
  });
});
