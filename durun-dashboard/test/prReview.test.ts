import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { skipFakeClaude } from './fakeClaude.ts';
import type { PrInfo, PrThread } from '../shared/types.ts';

// 実際の状態に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-pr-review-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
const { toPrDetail, toThreads } = await import('../server/integrations.ts');
const { notesFor } = await import('../server/prReview.ts');

// 標準入力のプロンプトを書き出して呼ばれた回数を数え、プロンプトに載ったスレッドのうち「黙る」を含まないものに答える偽の claude を PATH の先頭に置く。
const promptFile = path.join(tmp, 'prompt');
const callsFile = path.join(tmp, 'calls');
fs.mkdirSync(path.join(tmp, 'bin'));
fs.writeFileSync(
  path.join(tmp, 'bin', 'claude'),
  `#!/bin/sh
cat > '${promptFile}'
echo >> '${callsFile}'
node -e '
const prompt = require("fs").readFileSync(process.argv[1], "utf8");
const ids = [...prompt.matchAll(/<thread id="([^"]+)">([^]*?)<\\/thread>/g)].filter((m) => !m[2].includes("黙る")).map((m) => m[1]);
const threads = ids.map((id) => ({ id, explanation: " " + id + " の説明 ", factCheck: "正しい", proposal: "直す" }));
console.log(JSON.stringify({ structured_output: { threads } }));
' '${promptFile}'
`,
  { mode: 0o755 },
);
process.env.PATH = `${path.join(tmp, 'bin')}${path.delimiter}${process.env.PATH}`;
const calls = (): number => (fs.existsSync(callsFile) ? fs.readFileSync(callsFile, 'utf8').length : 0);
const lastPrompt = (): string => fs.readFileSync(promptFile, 'utf8');

const tree = { dir: tmp, onBranch: true };
const pr: PrInfo = { url: 'https://github.com/o/r/pull/1', number: 1, title: '題名', state: 'OPEN', isDraft: false, review: '', checks: { passed: 0, failed: 0, pending: 0 } };

const thread = (id: string, ...bodies: string[]): PrThread => ({
  id,
  path: 'src/a.ts',
  line: 3,
  side: 'new',
  outdated: false,
  diffHunk: '@@ -1 +1 @@\n+x',
  comments: bodies.map((body, i) => ({ id: `${id}-c${i}`, author: 'reviewer', body, url: `${pr.url}#discussion_r${i}`, at: 0 })),
});

test('toThreads は、未解決のスレッドだけを、先頭のコメントの差分と、差分の側と、退会したユーザーを ghost にしたコメントつきで返す', () => {
  const comment = { id: 'c1', author: null, body: '直して', url: 'https://github.com/o/r/pull/1#discussion_r1', createdAt: '2026-10-08T05:20:10Z', diffHunk: '@@ -1 +1 @@' };
  const threads = toThreads([
    { id: 't1', isResolved: false, isOutdated: true, path: 'src/a.ts', line: null, diffSide: 'LEFT', comments: { nodes: [comment, { ...comment, id: 'c2', author: { login: 'me' }, diffHunk: '別の差分' }] } },
    { id: 't2', isResolved: true, isOutdated: false, path: 'src/b.ts', line: 4, comments: { nodes: [comment] } },
  ]);
  assert.deepEqual(threads, [
    {
      id: 't1',
      path: 'src/a.ts',
      line: undefined,
      side: 'old',
      outdated: true,
      diffHunk: '@@ -1 +1 @@',
      comments: [
        { id: 'c1', author: 'ghost', body: '直して', url: comment.url, at: Date.parse('2026-10-08T05:20:10Z') },
        { id: 'c2', author: 'me', body: '直して', url: comment.url, at: Date.parse('2026-10-08T05:20:10Z') },
      ],
    },
  ]);
});

test('notesFor は、分析していないスレッドだけを 1 回でまとめて分析し、返信が付いたスレッドは作り直す', { skip: skipFakeClaude }, async () => {
  const a = thread('a', '直して');
  const b = thread('b', 'ここも');
  assert.deepEqual(await notesFor(tree, pr, [a, b]), {
    notes: { a: { explanation: 'a の説明', factCheck: '正しい', proposal: '直す' }, b: { explanation: 'b の説明', factCheck: '正しい', proposal: '直す' } },
    failed: undefined,
  });
  assert.equal(calls(), 1);
  assert.match(lastPrompt(), /<thread id="a">\n場所: src\/a\.ts:3\n<diff_hunk>\n@@ -1 \+1 @@\n\+x\n<\/diff_hunk>\n<comment author="reviewer">\n直して\n<\/comment>\n<\/thread>/);

  // 同じスレッドは作り直さず、返信が付いた b だけを分析する。
  const replied = thread('b', 'ここも', '直しました');
  assert.equal(Object.keys((await notesFor(tree, pr, [a, replied])).notes).length, 2);
  assert.equal(calls(), 2);
  assert.doesNotMatch(lastPrompt(), /<thread id="a">/);
});

test('notesFor は、分析が返らなかったスレッドの理由を返し、次に頼まれたときに作り直す', { skip: skipFakeClaude }, async () => {
  const silent = thread('c', '黙る');
  const before = calls();
  const { notes, failed } = await notesFor(tree, pr, [silent]);
  assert.deepEqual(notes, {});
  assert.match(failed ?? '', /返りませんでした/);
  await notesFor(tree, pr, [silent]);
  assert.equal(calls() - before, 2);
});

test('toPrDetail は、会話のコメントとレビューを古い順に並べ、行へのコメントだけの空のレビューを外す', () => {
  const detail = toPrDetail({
    url: 'https://github.com/o/r/pull/1', number: 1, title: '直す', state: 'OPEN', isDraft: false, author: { login: 'me' }, body: '本文',
    headRefName: 'feature/x', baseRefName: 'main', additions: 3, deletions: 1, files: [{ path: 'a.ts', additions: 3, deletions: 1 }], updatedAt: '2026-10-09T00:00:03Z',
    comments: [{ author: null, body: '2 番目', createdAt: '2026-10-09T00:00:02Z' }],
    reviews: [
      { author: { login: 'rv' }, body: '', state: 'COMMENTED', submittedAt: '2026-10-09T00:00:00Z' },
      { author: { login: 'rv' }, body: '', state: 'APPROVED', submittedAt: '2026-10-09T00:00:01Z' },
    ],
  });
  assert.deepEqual(detail.posts, [
    { author: 'rv', body: '', at: Date.parse('2026-10-09T00:00:01Z'), state: 'APPROVED' },
    { author: 'ghost', body: '2 番目', at: Date.parse('2026-10-09T00:00:02Z') },
  ]);
});
