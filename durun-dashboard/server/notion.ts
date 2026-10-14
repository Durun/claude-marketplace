// Notion のページとデータベースを、Notion の API で読んで Markdown にする。
import type { NotionPage } from '../shared/types.ts';
import { cached } from './cache.ts';

const TOKEN = process.env.NOTION_TOKEN;
const API = 'https://api.notion.com/v1';
const API_VERSION = '2022-06-28';
// 子を持つブロックは 1 つずつ読みに行くので、入れ子の多いページで待たせすぎないよう、読みに行く回数を抑える。
const MAX_REQUESTS = 60;
const MAX_RETRY_AFTER_SEC = 5;

/** トークンを設定してあるか。無ければ、画面は Notion へのリンクをブラウザで開く。 */
export const notionEnabled = (): boolean => !!TOKEN;

type RichText = {
  plain_text: string;
  href?: string | null;
  annotations?: { bold?: boolean; italic?: boolean; strikethrough?: boolean; code?: boolean };
  equation?: { expression: string };
};

type FileRef = { url?: string; external?: { url: string }; file?: { url: string }; name?: string; caption?: RichText[] };

/** ブロックの中身。種類ごとに持つものが違うので、使うものだけを並べる。 */
type BlockBody = FileRef & {
  rich_text?: RichText[];
  checked?: boolean;
  language?: string;
  title?: string;
  expression?: string;
  cells?: RichText[][];
  icon?: { emoji?: string } | null;
};

/** children は、has_children のブロックの子を読んで足したもの。 */
export type NotionBlock = { id: string; type: string; has_children?: boolean; children?: NotionBlock[] } & { [type: string]: BlockBody | unknown };

const ESCAPED = /[\\`*_[\]<>~|]/g;

// 強調の記号は、空白と隣り合うと強調として読まれないので、前後の空白の内側に置く。
const wrap = (text: string, mark: string): string => text.replace(/^(\s*)([\s\S]*?)(\s*)$/, (_, head: string, body: string, tail: string) => (body ? `${head}${mark}${body}${mark}${tail}` : text));

const pageUrl = (id: string): string => `https://www.notion.so/${id.replaceAll('-', '')}`;

// 中に ` があるコードは、`` で囲む。
const codeSpan = (code: string): string => (code.includes('`') ? `\`\` ${code} \`\`` : `\`${code}\``);

const inline = (t: RichText): string => {
  const a = t.annotations ?? {};
  let text = t.equation ? `\`${t.equation.expression}\`` : a.code ? codeSpan(t.plain_text) : t.plain_text.replace(ESCAPED, '\\$&');
  if (a.bold) text = wrap(text, '**');
  if (a.italic) text = wrap(text, '*');
  if (a.strikethrough) text = wrap(text, '~~');
  // ページへのリンクは、Notion が / で始まるパスで返すことがある。
  return t.href ? `[${text}](${new URL(t.href, 'https://www.notion.so').href})` : text;
};

const rich = (texts: RichText[] | undefined): string => (texts ?? []).map(inline).join('');
const plain = (texts: RichText[] | undefined): string => (texts ?? []).map((t) => t.plain_text).join('');

const indent = (text: string, by: string): string => text.split('\n').map((l) => (l ? by + l : l)).join('\n');
const quote = (text: string): string => text.split('\n').map((l) => (l ? `> ${l}` : '>')).join('\n');

const LIST_MARK: Record<string, (b: BlockBody) => string> = {
  bulleted_list_item: () => '- ',
  numbered_list_item: () => '1. ',
  to_do: (b) => `- [${b.checked ? 'x' : ' '}] `,
  toggle: () => '- ',
};

const HEADING: Record<string, string> = { heading_1: '# ', heading_2: '## ', heading_3: '### ' };
const LINKED = new Set(['image', 'video', 'file', 'pdf', 'audio', 'bookmark', 'embed', 'link_preview']);
// 中身を持たず、子をそのまま並べるブロック。
const CONTAINERS = new Set(['column_list', 'column', 'synced_block']);

const tableRow = (cells: RichText[][]): string => `| ${cells.map((c) => rich(c).replaceAll('\n', ' ')).join(' | ')} |`;

// GFM の表は見出しの行が要るので、見出しの無い表でも 1 行目を見出しにする。
const table = (rows: NotionBlock[]): string => {
  const [head, ...rest] = rows.map((r) => (r.table_row as BlockBody | undefined)?.cells ?? []);
  if (!head) return '';
  return [tableRow(head), `|${' --- |'.repeat(head.length)}`, ...rest.map(tableRow)].join('\n');
};

const fence = (code: string, language = ''): string => {
  const ticks = code.includes('```') ? '````' : '```';
  return `${ticks}${language === 'plain text' ? '' : language}\n${code}\n${ticks}`;
};

const blockText = (b: NotionBlock): string => {
  const body = (b[b.type] ?? {}) as BlockBody;
  const children = b.children?.length ? blocksToMarkdown(b.children) : '';
  const below = children && `\n\n${children}`;
  const mark = LIST_MARK[b.type]?.(body);
  if (mark) return `${mark}${rich(body.rich_text)}${children && `\n${indent(children, '    ')}`}`;
  if (HEADING[b.type]) return `${HEADING[b.type]}${rich(body.rich_text).replaceAll('\n', ' ')}${below}`;
  if (CONTAINERS.has(b.type)) return children;
  if (LINKED.has(b.type)) {
    const url = body.url ?? body.external?.url ?? body.file?.url;
    const label = rich(body.caption) || body.name?.replace(ESCAPED, '\\$&') || (b.type === 'image' ? '画像' : url);
    return url ? `[${label}](${url})` : '';
  }
  switch (b.type) {
    case 'paragraph': return `${rich(body.rich_text)}${below}`;
    case 'quote': return quote(`${rich(body.rich_text)}${below}`);
    case 'callout': return quote(`${body.icon?.emoji ? `${body.icon.emoji} ` : ''}${rich(body.rich_text)}${below}`);
    case 'code': return fence(plain(body.rich_text), body.language);
    case 'equation': return fence(body.expression ?? '');
    case 'divider': return '---';
    case 'table': return table(b.children ?? []);
    case 'child_page': return `📄 [${(body.title ?? '').replace(ESCAPED, '\\$&') || '無題'}](${pageUrl(b.id)})`;
    case 'child_database': return `🗃️ [${(body.title ?? '').replace(ESCAPED, '\\$&') || '無題'}](${pageUrl(b.id)})`;
    default: return children;
  }
};

/** ブロックを Markdown にする。続くリストの項目は 1 つのリストにまとめ、ほかは空行で区切る。目次など、文字にできないブロックは出さない。 */
export const blocksToMarkdown = (blocks: NotionBlock[]): string => {
  let out = '';
  let prevListed = false;
  for (const b of blocks) {
    const text = blockText(b);
    if (!text) continue;
    const listed = b.type in LIST_MARK;
    out += out ? (listed && prevListed ? '\n' : '\n\n') : '';
    out += text;
    prevListed = listed;
  }
  return out;
};

/** post を渡すと、それを本文にして POST する。 */
const notionFetch = async <T,>(apiPath: string, post?: object, retry = true): Promise<T> => {
  if (!TOKEN) throw new Error('~/.durun-dashboard/.env.local に NOTION_TOKEN を設定してください');
  const headers = { authorization: `Bearer ${TOKEN}`, 'notion-version': API_VERSION, accept: 'application/json' };
  const res = await fetch(`${API}/${apiPath}`, post ? { method: 'POST', headers: { ...headers, 'content-type': 'application/json' }, body: JSON.stringify(post) } : { headers });
  // 子のブロックをまとめて読みに行くと、Notion が間隔を空けるよう求めることがある。求められた秒数だけ待って、1 回だけやり直す。
  if (res.status === 429 && retry) {
    await new Promise((r) => setTimeout(r, Math.min(Number(res.headers.get('retry-after')) || 1, MAX_RETRY_AFTER_SEC) * 1000));
    return notionFetch(apiPath, post, false);
  }
  const body = (await res.json().catch(() => ({}))) as T & { message?: string };
  if (!res.ok) throw new Error(body.message || `HTTP ${res.status}`);
  return body;
};

type BlockList = { results: NotionBlock[]; has_more?: boolean; next_cursor?: string | null };

/** 読みに行ける残りの回数。使い切ったら、そこから先のブロックは読まない。 */
type Budget = { left: number; truncated: boolean };

const childrenOf = async (id: string, budget: Budget): Promise<NotionBlock[]> => {
  const blocks: NotionBlock[] = [];
  let cursor: string | undefined;
  do {
    if (budget.left <= 0) {
      budget.truncated = true;
      return blocks;
    }
    budget.left--;
    const page = await notionFetch<BlockList>(`blocks/${id}/children?${new URLSearchParams({ page_size: '100', ...(cursor ? { start_cursor: cursor } : {}) })}`);
    blocks.push(...page.results);
    cursor = (page.has_more && page.next_cursor) || undefined;
  } while (cursor);
  // 子ページの中身は、そのページを開いたときに読む。
  await Promise.all(blocks.filter((b) => b.has_children && b.type !== 'child_page' && b.type !== 'child_database').map(async (b) => (b.children = await childrenOf(b.id, budget))));
  return blocks;
};

type DateRange = { start: string; end?: string | null };

/** ページのプロパティの値。種類ごとに持つものが違うので、表に出すものだけを並べる。 */
type PropertyValue = {
  type?: string;
  title?: RichText[];
  rich_text?: RichText[];
  number?: number | null;
  select?: { name: string } | null;
  status?: { name: string } | null;
  multi_select?: { name: string }[];
  date?: DateRange | null;
  checkbox?: boolean;
  url?: string | null;
  email?: string | null;
  phone_number?: string | null;
  people?: { name?: string }[];
  files?: { name: string }[];
  created_time?: string;
  last_edited_time?: string;
  formula?: { string?: string | null; number?: number | null; boolean?: boolean | null; date?: DateRange | null };
  unique_id?: { prefix?: string | null; number: number };
};

type PageBody = {
  id?: string;
  url?: string;
  last_edited_time?: string;
  icon?: { emoji?: string } | null;
  properties?: Record<string, PropertyValue>;
};

type DatabaseBody = Omit<PageBody, 'properties'> & { title?: RichText[]; properties?: Record<string, { type?: string }> };

const escaped = (text: string | null | undefined): string => (text ?? '').replace(ESCAPED, '\\$&');
const dateRange = (d: DateRange | null | undefined): string => (d ? `${d.start}${d.end ? ` 〜 ${d.end}` : ''}` : '');
const names = (items: { name?: string }[] | undefined): string => (items ?? []).map((i) => escaped(i.name)).filter(Boolean).join('、');

// リレーションやロールアップなど、ほかのページを読まないと文字にできないものは空にする。
const cell = (p: PropertyValue | undefined): string => {
  switch (p?.type) {
    case 'title': return rich(p.title);
    case 'rich_text': return rich(p.rich_text);
    case 'number': return String(p.number ?? '');
    case 'select': return escaped(p.select?.name);
    case 'status': return escaped(p.status?.name);
    case 'multi_select': return names(p.multi_select);
    case 'date': return dateRange(p.date);
    case 'checkbox': return p.checkbox ? '✓' : '';
    case 'url': return p.url ?? '';
    case 'email': return escaped(p.email);
    case 'phone_number': return escaped(p.phone_number);
    case 'people': return names(p.people);
    case 'files': return names(p.files);
    case 'created_time': return p.created_time?.slice(0, 10) ?? '';
    case 'last_edited_time': return p.last_edited_time?.slice(0, 10) ?? '';
    case 'formula': return p.formula?.date ? dateRange(p.formula.date) : p.formula?.boolean ? '✓' : escaped(p.formula?.string) || String(p.formula?.number ?? '');
    case 'unique_id': return p.unique_id ? `${p.unique_id.prefix ? `${p.unique_id.prefix}-` : ''}${p.unique_id.number}` : '';
    default: return '';
  }
};

/**
 * データベースの行を Markdown の表にする。columns はプロパティの名前で、先頭を題名の列にする。
 * 題名は行のページへのリンクにし、どの行でも空の列は出さない。
 */
export const databaseToMarkdown = (columns: string[], rows: PageBody[]): string => {
  if (!rows.length) return '';
  const [titleColumn = '', ...rest] = columns;
  const shown = rest.filter((c) => rows.some((r) => cell(r.properties?.[c])));
  const line = (cells: string[]): string => `| ${cells.map((c) => c.replaceAll('\n', ' ')).join(' | ')} |`;
  return [
    line([titleColumn, ...shown].map(escaped)),
    `|${' --- |'.repeat(shown.length + 1)}`,
    ...rows.map((r) => line([`[${cell(r.properties?.[titleColumn]) || '無題'}](${r.url ?? pageUrl(r.id ?? '')})`, ...shown.map((c) => cell(r.properties?.[c]))])),
  ].join('\n');
};

const titleOf = (page: PageBody): string => plain(Object.values(page.properties ?? {}).find((p) => p.type === 'title')?.title) || '無題';

const databaseTitle = (db: DatabaseBody): string => plain(db.title) || '無題';

// データベースの ID でページを読むと、Notion はデータベースだと断る。
const IS_DATABASE = /is a database, not a page/;

/** ページとして読めなかった理由がデータベースだったときは read で読み直し、ほかの理由はそのまま投げる。 */
const orDatabase = <T,>(read: () => Promise<T>) => (e: unknown): Promise<T> => {
  if (e instanceof Error && IS_DATABASE.test(e.message)) return read();
  throw e;
};

// 題名は資料の一覧が同じページを何度も尋ねるうえ、めったに変わらないので、ページの中身より長く覚える。
const TITLE_MAX_AGE = 60 * 60_000;
const PAGE_MAX_AGE = 60_000;

/** ページかデータベースの題名だけを読む。読めなかったページは、次に尋ねられたら読み直す。 */
export const notionTitle = (id: string): Promise<string> =>
  cached(`notion-title:${id}`, { maxAge: TITLE_MAX_AGE }, () =>
    notionFetch<PageBody>(`pages/${id}`).then(titleOf, orDatabase(async () => databaseTitle(await notionFetch<DatabaseBody>(`databases/${id}`)))),
  );

const pageOf = async (id: string): Promise<NotionPage> => {
  const budget: Budget = { left: MAX_REQUESTS, truncated: false };
  const blocks = childrenOf(id, budget);
  // 読めない理由はページのほうから取るので、ブロックのほうの失敗は、ページを読めてから受ける。
  blocks.catch(() => {});
  const page = await notionFetch<PageBody>(`pages/${id}`);
  return {
    id,
    url: page.url ?? pageUrl(id),
    title: titleOf(page),
    icon: page.icon?.emoji,
    updatedAt: page.last_edited_time ? Date.parse(page.last_edited_time) : undefined,
    markdown: blocksToMarkdown(await blocks),
    truncated: budget.truncated,
  };
};

// ponytail: 行は Notion が 1 回で返す 100 件までを、Notion が返す順に出す。ビューの絞り込みと並び順は API から読めない。
const databaseOf = async (id: string): Promise<NotionPage> => {
  const [db, rows] = await Promise.all([
    notionFetch<DatabaseBody>(`databases/${id}`),
    notionFetch<{ results: PageBody[]; has_more?: boolean }>(`databases/${id}/query`, { page_size: 100 }),
  ]);
  const columns = Object.entries(db.properties ?? {}).sort(([, a], [, b]) => Number(b.type === 'title') - Number(a.type === 'title')).map(([name]) => name);
  return {
    id,
    url: db.url ?? pageUrl(id),
    title: databaseTitle(db),
    icon: db.icon?.emoji,
    updatedAt: db.last_edited_time ? Date.parse(db.last_edited_time) : undefined,
    markdown: databaseToMarkdown(columns, rows.results),
    truncated: !!rows.has_more,
    database: true,
  };
};

/** ページの題名と本文。id は、ハイフンの無い 32 桁のページの ID。データベースの ID なら、行を表にして本文にする。 */
export const notionPage = (id: string, fresh = false): Promise<NotionPage> =>
  cached(`notion:${id}`, { maxAge: PAGE_MAX_AGE, fresh }, () => pageOf(id).catch(orDatabase(() => databaseOf(id))));
