import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// integrations.ts は読み込んだ時点の環境変数で JIRA の接続先を決めるので、先に設定してから読み込む。
// 読んだ課題を覚えるファイルは、実際の状態に触れないよう一時ディレクトリに置く。
process.env.DURUN_DASHBOARD_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-jira-'));
process.env.JIRA_BASE_URL = 'https://jira.example';
process.env.JIRA_EMAIL = 'me@example.com';
process.env.JIRA_API_TOKEN = 'token';
const { adfText, badgeOf, jiraDetail, jiraKeyOfUrl, jiraRefs, postJiraComment, searchJira, transitionJira } = await import('../server/integrations.ts');
const { pageTitleOf } = await import('../server/pageTitle.ts');

test('searchJira は、JQL で探した課題を、題名、状態、担当、更新日時、優先度、親、ラベル、報告者、期限つきで返す', async (t) => {
  t.mock.method(globalThis, 'fetch', async (url: string) => {
    assert.match(url, /^https:\/\/jira\.example\/rest\/api\/3\/search\/jql\?jql=project\+%3D\+PROJ&maxResults=5&/);
    return Response.json({
      issues: [
        { key: 'PROJ-1', fields: { summary: '直す', status: { name: '進行中', statusCategory: { key: 'indeterminate' } }, assignee: { displayName: '山田' }, updated: '2026-10-07T12:57:27.228+0900', priority: { name: 'High' }, parent: { key: 'PROJ-9', fields: { summary: '親の課題' } }, labels: ['proj'], reporter: { displayName: '佐藤' }, duedate: '2026-10-31' } },
        { key: 'PROJ-2', fields: { summary: '誰も持っていない', status: { name: '未着手', statusCategory: { key: 'new' } }, assignee: null } },
      ],
    });
  });
  assert.deepEqual(await searchJira('project = PROJ', 5), [
    { key: 'PROJ-1', url: 'https://jira.example/browse/PROJ-1', summary: '直す', status: '進行中', statusCategory: 'indeterminate', assignee: '山田', updatedAt: Date.parse('2026-10-07T03:57:27.228Z'), priority: 'High', parent: { key: 'PROJ-9', summary: '親の課題' }, labels: ['proj'], reporter: '佐藤', dueDate: '2026-10-31' },
    { key: 'PROJ-2', url: 'https://jira.example/browse/PROJ-2', summary: '誰も持っていない', status: '未着手', statusCategory: 'new', assignee: undefined, updatedAt: undefined, priority: undefined, parent: undefined, labels: undefined, reporter: undefined, dueDate: undefined },
  ]);
});

test('searchJira は、JQL の誤りを JIRA が返した理由で投げる', async (t) => {
  t.mock.method(globalThis, 'fetch', async () => Response.json({ errorMessages: ['JQL クエリのエラー'] }, { status: 400 }));
  await assert.rejects(searchJira('foo = =', 5), /JQL クエリのエラー/);
});

test('jiraRefs は、自分が担当者か起票者の課題があるスペースごとに、自分の課題、ほかの課題の順に、それぞれ更新が新しい順で並べる', async (t) => {
  const mine = 'assignee = currentUser() OR reporter = currentUser()';
  const found: Record<string, string[]> = {
    [mine]: ['CATS-3', 'LPO-1', 'CATS-12'],
    [`project = "CATS" AND (${mine}) ORDER BY updated DESC`]: ['CATS-12', 'CATS-3'],
    'project = "CATS" ORDER BY updated DESC': ['CATS-120', 'CATS-12'],
    [`project = "LPO" AND (${mine}) ORDER BY updated DESC`]: ['LPO-1'],
    'project = "LPO" ORDER BY updated DESC': ['LPO-2', 'LPO-1'],
  };
  t.mock.method(globalThis, 'fetch', async (url: string) => {
    const q = new URL(url).searchParams;
    const keys = found[q.get('jql') ?? ''];
    assert.ok(keys, q.get('jql') ?? '');
    // スペースを数えるために読む自分の課題は、キーだけを 1 回で返せる上限まで読む。
    assert.deepEqual([q.get('fields'), q.get('maxResults')], q.get('jql') === mine ? ['key', '5000'] : ['summary,status', '100']);
    return Response.json({ issues: keys.map((key) => ({ key, fields: { summary: key } })) });
  });
  assert.deepEqual((await jiraRefs()).map((j) => j.key), ['CATS-12', 'CATS-3', 'CATS-120', 'LPO-1', 'LPO-2']);
});

test('adfText は、JIRA の本文の段落、改行、リスト、メンション、リンクを、Markdown として読める文字にする', () => {
  const text = (t: string) => ({ type: 'text', text: t });
  const doc = {
    type: 'doc',
    content: [
      { type: 'paragraph', content: [text('一行目'), { type: 'hardBreak' }, text('二行目 '), { type: 'mention', attrs: { text: '@山田' } }] },
      { type: 'bulletList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [text('項目')] }] }] },
      { type: 'paragraph', content: [{ type: 'inlineCard', attrs: { url: 'https://example.com' } }] },
    ],
  };
  assert.equal(adfText(doc).trim(), '一行目\n二行目 @山田\n\n- 項目\n\n\nhttps://example.com');
});

test('adfText は、見出し、表、タスクリスト、コードとリンクの装飾を Markdown の書き方にし、太字は文字だけ残す', () => {
  const text = (t: string, mark?: object) => ({ type: 'text', text: t, marks: mark && [mark] });
  const p = (...content: object[]) => ({ type: 'paragraph', content });
  const row = (type: string, ...cells: object[][]) => ({ type: 'tableRow', content: cells.map((content) => ({ type, content })) });
  const task = (t: string, state: string) => ({ type: 'taskItem', attrs: { state }, content: [text(t)] });
  const doc = {
    type: 'doc',
    content: [
      { type: 'heading', attrs: { level: 2 }, content: [text('凡例')] },
      { type: 'table', content: [row('tableHeader', [p(text('記号'))], [p(text('意味'))]), row('tableCell', [p(text('A'))], [p(text('a|b')), p(text('続き'))])] },
      { type: 'taskList', content: [task('未', 'TODO'), task('済', 'DONE')] },
      p(text('global', { type: 'code' }), text(' と '), text('PR', { type: 'link', attrs: { href: 'https://example.com/pr' } }), text(' は'), text('大事。', { type: 'strong' })),
    ],
  };
  assert.equal(adfText(doc).trim(), '## 凡例\n\n| 記号 | 意味 |\n| --- | --- |\n| A | a\\|b 続き |\n\n- [ ] 未\n- [x] 済\n\n\n`global` と [PR](https://example.com/pr) は大事。');
});

test('jiraDetail は、課題の属性に本文とコメントを古い順に添え、子の課題と、今のステータスへのものを除いた移せる先も返す', async (t) => {
  t.mock.method(globalThis, 'fetch', async (url: string) => {
    if (url.includes('/search/jql?')) {
      assert.match(url, /jql=parent\+%3D\+PROJ-1\+ORDER\+BY\+created\+ASC/);
      return Response.json({ issues: [{ key: 'PROJ-2', fields: { summary: '子' } }] });
    }
    assert.match(url, /^https:\/\/jira\.example\/rest\/api\/3\/issue\/PROJ-1\?fields=.*&expand=transitions$/);
    const p = (s: string) => ({ type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: s }] }] });
    const to = (name: string, key: string) => ({ name, statusCategory: { key } });
    return Response.json({
      fields: {
        summary: '直す', status: { name: '進行中', statusCategory: { key: 'indeterminate' } }, assignee: null, description: p('説明'),
        comment: { comments: [{ author: { displayName: '佐藤' }, body: p('見ました'), created: '2026-10-07T12:00:00.000+0900' }] },
      },
      transitions: [
        { id: '11', name: '未着手', to: to('未着手', 'new') },
        { id: '21', name: '進行中', to: to('進行中', 'indeterminate') },
        { id: '31', name: 'レビューに出す', to: to('レビュー中', 'indeterminate') },
      ],
    });
  });
  const issue = await jiraDetail('PROJ-1');
  assert.equal(issue.url, 'https://jira.example/browse/PROJ-1');
  assert.equal(issue.description, '説明');
  assert.deepEqual(issue.comments, [{ author: '佐藤', body: '見ました', at: Date.parse('2026-10-07T03:00:00.000Z') }]);
  assert.deepEqual(issue.children.map((c) => [c.key, c.summary]), [['PROJ-2', '子']]);
  assert.deepEqual(issue.transitions, [
    { id: '11', name: '未着手', status: '未着手', statusCategory: 'new' },
    { id: '31', name: 'レビューに出す', status: 'レビュー中', statusCategory: 'indeterminate' },
  ]);
});

test('transitionJira は、選んだトランジションで課題を移し、失敗は JIRA が項目ごとに返した理由でも投げる', async (t) => {
  const sent: unknown[] = [];
  t.mock.method(globalThis, 'fetch', async (url: string, init: RequestInit) => {
    assert.equal(url, 'https://jira.example/rest/api/3/issue/PROJ-1/transitions');
    sent.push(JSON.parse(String(init.body)));
    return sent.length === 1 ? new Response(null, { status: 204 }) : Response.json({ errorMessages: [], errors: { resolution: '解決状況を選んでください' } }, { status: 400 });
  });
  await transitionJira('PROJ-1', '31');
  assert.deepEqual(sent[0], { transition: { id: '31' } });
  await assert.rejects(transitionJira('PROJ-1', '41'), /解決状況を選んでください/);
});

test('postJiraComment は、行ごとに段落にしたコメントを投稿し、失敗は JIRA が返した理由で投げる', async (t) => {
  const sent: unknown[] = [];
  t.mock.method(globalThis, 'fetch', async (url: string, init: RequestInit) => {
    assert.equal(url, 'https://jira.example/rest/api/3/issue/PROJ-1/comment');
    sent.push(JSON.parse(String(init.body)));
    return sent.length === 1 ? Response.json({}) : Response.json({ errorMessages: ['権限がありません'] }, { status: 403 });
  });
  await postJiraComment('PROJ-1', 'a\n\nb');
  assert.deepEqual(sent[0], {
    body: { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'a' }] }, { type: 'paragraph', content: [] }, { type: 'paragraph', content: [{ type: 'text', text: 'b' }] }] },
  });
  await assert.rejects(postJiraComment('PROJ-1', 'c'), /権限がありません/);
});

test('pageTitleOf は、資料に足した JIRA の課題の画面に、API から読んだ課題のキーと題名を付け、読めなければ画面の題名も読まない', async (t) => {
  const called: string[] = [];
  t.mock.method(globalThis, 'fetch', async (url: string) => {
    called.push(url);
    return url.endsWith('/issue/PROJ-1?fields=summary') ? Response.json({ fields: { summary: '直す' } }) : Response.json({ errorMessages: ['権限がありません'] }, { status: 403 });
  });
  assert.equal(await pageTitleOf(new URL('https://jira.example/browse/PROJ-1?focusedCommentId=10')), 'PROJ-1 直す');
  assert.equal(await pageTitleOf(new URL('https://jira.example/browse/PROJ-2')), undefined);
  assert.deepEqual(called, ['https://jira.example/rest/api/3/issue/PROJ-1?fields=summary', 'https://jira.example/rest/api/3/issue/PROJ-2?fields=summary']);
  assert.equal(jiraKeyOfUrl(new URL('https://other.example/browse/PROJ-1')), undefined);
  assert.equal(jiraKeyOfUrl(new URL('https://jira.example/browse/')), undefined);
});

test('badgeOf は、JIRA の課題、GitHub の PR と Issue の URL と課題のキーを、そのものを指す URL にそろえ、ほかの URL は受けない', () => {
  assert.deepEqual(badgeOf('https://jira.example/browse/PROJ-1?focusedCommentId=1'), { kind: 'jira', url: 'https://jira.example/browse/PROJ-1' });
  assert.deepEqual(badgeOf('PROJ-12'), { kind: 'jira', url: 'https://jira.example/browse/PROJ-12' });
  assert.deepEqual(badgeOf('proj-12'), { kind: 'jira', url: 'https://jira.example/browse/PROJ-12' });
  assert.equal(badgeOf('#12'), undefined);
  assert.deepEqual(badgeOf('https://github.com/o/r/pull/12/files#diff-1'), { kind: 'pr', url: 'https://github.com/o/r/pull/12' });
  assert.deepEqual(badgeOf('https://github.com/o/r/issues/3?q=1'), { kind: 'issue', url: 'https://github.com/o/r/issues/3' });
  assert.equal(badgeOf('https://github.com/o/r/pulls'), undefined);
  assert.equal(badgeOf('http://github.com/o/r/pull/12'), undefined);
  assert.equal(badgeOf('https://other.example/browse/PROJ-1'), undefined);
});
