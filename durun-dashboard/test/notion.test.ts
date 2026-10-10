import assert from 'node:assert/strict';
import { test } from 'node:test';
import { notionPageId } from '../shared/types.ts';

// notion.ts は読み込んだ時点の環境変数でトークンを決めるので、先に設定してから読み込む。
process.env.NOTION_TOKEN = 'secret';
const { blocksToMarkdown, notionPage } = await import('../server/notion.ts');

const ID = '1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d';
const text = (plain_text: string, annotations = {}, href?: string) => ({ plain_text, annotations, href });
const block = (type: string, body: object, children?: object[]) => ({ id: `${type}-id`, type, [type]: body, children }) as never;

test('notionPageId は、Notion のページの URL から ID を取り出し、ほかのサイトとページを指さない URL は拾わない', () => {
  const id = (url: string) => notionPageId(new URL(url));
  assert.equal(id(`https://www.notion.so/team/%E5%88%86%E6%9E%90-${ID}`), ID);
  assert.equal(id(`https://www.notion.so/${ID.toUpperCase()}?v=abc#block`), ID);
  assert.equal(id('https://www.notion.so/1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d'), ID);
  assert.equal(id(`https://app.notion.com/p/${ID}`), ID);
  assert.equal(id(`https://me.notion.site/Title-${ID}/`), ID);
  assert.equal(id(`https://www.notion.so/team/ffffffffffffffffffffffffffffffff?v=1&p=${ID}&pm=s`), ID);
  assert.equal(id('https://www.notion.so/help/guides'), undefined);
  assert.equal(id(`https://notion.so.example.com/${ID}`), undefined);
  assert.equal(id(`https://example.com/${ID}`), undefined);
});

test('blocksToMarkdown は、見出し、段落、装飾、リンクを Markdown にし、記号は文字のまま出るようにする', () => {
  const md = blocksToMarkdown([
    block('heading_2', { rich_text: [text('まとめ')] }),
    block('paragraph', { rich_text: [text('a*b '), text('太字 ', { bold: true }), text('x`y', { code: true }), text(' '), text('z', { code: true }), text('先', {}, '/abc')] }),
    block('divider', {}),
    block('table_of_contents', {}),
    block('code', { rich_text: [text('const a = 1;')], language: 'typescript' }),
  ]);
  assert.equal(md, '## まとめ\n\na\\*b **太字** `` x`y `` `z`[先](https://www.notion.so/abc)\n\n---\n\n```typescript\nconst a = 1;\n```');
});

test('blocksToMarkdown は、続く項目を 1 つのリストにまとめ、子は字下げして項目の中に入れる', () => {
  const md = blocksToMarkdown([
    block('bulleted_list_item', { rich_text: [text('親')] }, [block('numbered_list_item', { rich_text: [text('子')] })]),
    block('to_do', { rich_text: [text('済み')], checked: true }),
    block('paragraph', { rich_text: [text('後ろ')] }),
  ]);
  assert.equal(md, '- 親\n    1. 子\n- [x] 済み\n\n後ろ');
});

test('blocksToMarkdown は、引用、コールアウト、表、画像、子ページを Markdown にする', () => {
  const md = blocksToMarkdown([
    block('callout', { rich_text: [text('注意')], icon: { emoji: '💡' } }, [block('paragraph', { rich_text: [text('中身')] })]),
    block('table', {}, [block('table_row', { cells: [[text('名前')], [text('a|b')]] }), block('table_row', { cells: [[text('x')], [text('y')]] })]),
    block('image', { file: { url: 'https://files.example/a.png' }, caption: [] }),
    block('child_page', { title: '子' }),
  ]);
  assert.equal(md, '> 💡 注意\n>\n> 中身\n\n| 名前 | a\\|b |\n| --- | --- |\n| x | y |\n\n[画像](https://files.example/a.png)\n\n📄 [子](https://www.notion.so/child_pageid)');
});

test('notionPage は、題名と更新日時に、続きのページと子のブロックまで読んだ本文を添えて返す', async (t) => {
  const p = (id: string, s: string, has_children = false) => ({ id, type: 'paragraph', has_children, paragraph: { rich_text: [text(s)] } });
  const seen: string[] = [];
  t.mock.method(globalThis, 'fetch', async (url: string, init: RequestInit) => {
    assert.equal((init.headers as Record<string, string>).authorization, 'Bearer secret');
    const path = url.replace('https://api.notion.com/v1/', '');
    seen.push(path);
    if (path === `pages/${ID}`) return Response.json({ url: 'https://www.notion.so/x', last_edited_time: '2026-10-09T03:00:00.000Z', icon: { emoji: '📈' }, properties: { 名前: { type: 'title', title: [text('分析')] } } });
    if (path === `blocks/${ID}/children?page_size=100`) return Response.json({ results: [p('b1', '一', true)], has_more: true, next_cursor: 'c2' });
    if (path === `blocks/${ID}/children?page_size=100&start_cursor=c2`) return Response.json({ results: [p('b2', '二')] });
    if (path === 'blocks/b1/children?page_size=100') return Response.json({ results: [p('b3', '子')] });
    return Response.json({ message: 'unexpected' }, { status: 400 });
  });
  assert.deepEqual(await notionPage(ID), { id: ID, url: 'https://www.notion.so/x', title: '分析', icon: '📈', updatedAt: Date.parse('2026-10-09T03:00:00.000Z'), markdown: '一\n\n子\n\n二', truncated: false });
  assert.equal(seen.length, 4);
});

test('notionPage は、読めないページを Notion が返した理由で投げる', async (t) => {
  t.mock.method(globalThis, 'fetch', async () => Response.json({ message: 'Could not find page' }, { status: 404 }));
  await assert.rejects(notionPage(ID), /Could not find page/);
});
