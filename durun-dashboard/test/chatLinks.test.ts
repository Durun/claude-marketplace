import assert from 'node:assert/strict';
import { test } from 'node:test';
import { chatLinks, chatMarkdownFiles, mergeLinks } from '../client/chatLinks.ts';

test('人間と Claude の発言に出たリンクを、題名を付けて新しい順に 1 件ずつ拾い、ダッシュボード自身とコードの中の URL は除く', () => {
  const links = chatLinks(
    [
      { kind: 'message', at: 1, text: '手順書: http://docs.example.jp/manual/バックアップ手順.html\n`https://in.code/x`\n```\ncurl https://in.block/x\n```' },
      { kind: 'prompt', at: 2, text: 'これを読んで https://from.human/doc' },
      {
        kind: 'message',
        at: 3,
        text: [
          '[CDN キャッシュの計画](https://claude.ai/code/artifact/ca1) を書きました。',
          '- https://example.atlassian.net/wiki/spaces/DOC/pages/1/4.9.+CloudFlare',
          '- http://docs.example.jp/manual/index.html',
          '- https://github.com/o/r/pull/1 と https://example.atlassian.net/browse/PROJ-1149',
          '- https://github.com/o/r/pull/1#discussion_r9 と https://github.com/o/r/actions/runs/5/job/6',
          '- http://localhost:3000/ と http://127.0.0.1:7337/#/doc/abc と a@b.com',
        ].join('\n'),
      },
      { kind: 'message', at: 4, text: 'もう一度 https://claude.ai/code/artifact/ca1 を見てください。' },
    ],
    '7337',
  );
  assert.deepEqual(
    links.map((l) => [l.title, l.host, l.at]),
    [
      ['CDN キャッシュの計画', 'claude.ai', 4],
      ['4.9. CloudFlare', 'example.atlassian.net', 3],
      ['manual', 'docs.example.jp', 3],
      ['o/r#1', 'github.com', 3],
      ['PROJ-1149', 'example.atlassian.net', 3],
      ['o/r#1 のコメント', 'github.com', 3],
      ['o/r の Actions 5', 'github.com', 3],
      ['localhost:3000', 'localhost:3000', 3],
      ['doc', 'from.human', 2],
      ['バックアップ手順', 'docs.example.jp', 1],
    ],
  );
});

test('URL の後ろに空白を置かずに文を続けたときは、閉じていない ) と、日本語の句読点や括弧の手前までを URL とする', () => {
  const text = '提案(https://x.example/a?b=1)は、作ること。（https://x.example/c）を見て、https://x.example/wiki/A_(B)。';
  assert.deepEqual(
    chatLinks([{ kind: 'prompt', at: 1, text }], '7337').map((l) => l.url),
    ['https://x.example/a?b=1', 'https://x.example/c', 'https://x.example/wiki/A_(B)'],
  );
});

test('題名とサイトが同じリンクは 1 件にまとめて最後に出たものを開き、まとめたリンクをすべて持つ', () => {
  const jira = (q: string, at: number) => ({ url: `https://j.example/browse/A-1${q}`, title: 'A-1', host: 'j.example', at });
  assert.deepEqual(
    mergeLinks([jira('?focusedCommentId=1', 1), jira('?focusedCommentId=3', 3), { url: 'https://k.example/browse/A-1', title: 'A-1', host: 'k.example', at: 2 }, jira('', 2)]).map((l) => [l.url, l.urls]),
    [
      ['https://j.example/browse/A-1?focusedCommentId=3', ['https://j.example/browse/A-1?focusedCommentId=3', 'https://j.example/browse/A-1', 'https://j.example/browse/A-1?focusedCommentId=1']],
      ['https://k.example/browse/A-1', ['https://k.example/browse/A-1']],
    ],
  );
});

test('発言のインラインのコードに出た Markdown ファイルを、行の指定を外して新しい順に 1 件ずつ拾う', () => {
  const files = chatMarkdownFiles([
    { kind: 'message', at: 1, text: '`docs/a.md` と `~/b.md:3-5` を書きました。`main.ts` と `x y.md` は除く\n```\ncat c.md\n```' },
    { kind: 'tool', at: 2, text: '`docs/c.md`' },
    { kind: 'prompt', at: 3, text: '`docs/a.md:10` を直して' },
  ]);
  assert.deepEqual(files, [{ ref: 'docs/a.md', at: 3 }, { ref: '~/b.md', at: 1 }]);
});
