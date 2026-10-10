// Notion のページを、Notion の API で読んで Markdown にする。
import type { NotionPage } from '../shared/types.ts';

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

const notionFetch = async <T,>(apiPath: string, retry = true): Promise<T> => {
  if (!TOKEN) throw new Error('~/.durun-dashboard/.env.local に NOTION_TOKEN を設定してください');
  const res = await fetch(`${API}/${apiPath}`, { headers: { authorization: `Bearer ${TOKEN}`, 'notion-version': API_VERSION, accept: 'application/json' } });
  // 子のブロックをまとめて読みに行くと、Notion が間隔を空けるよう求めることがある。求められた秒数だけ待って、1 回だけやり直す。
  if (res.status === 429 && retry) {
    await new Promise((r) => setTimeout(r, Math.min(Number(res.headers.get('retry-after')) || 1, MAX_RETRY_AFTER_SEC) * 1000));
    return notionFetch(apiPath, false);
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

type PageBody = {
  url?: string;
  last_edited_time?: string;
  icon?: { emoji?: string } | null;
  properties?: Record<string, { type?: string; title?: RichText[] }>;
};

/** ページの題名と本文。id は、ハイフンの無い 32 桁のページの ID。 */
export const notionPage = async (id: string): Promise<NotionPage> => {
  const budget: Budget = { left: MAX_REQUESTS, truncated: false };
  const [page, blocks] = await Promise.all([notionFetch<PageBody>(`pages/${id}`), childrenOf(id, budget)]);
  const title = Object.values(page.properties ?? {}).find((p) => p.type === 'title')?.title;
  return {
    id,
    url: page.url ?? pageUrl(id),
    title: plain(title) || '無題',
    icon: page.icon?.emoji,
    updatedAt: page.last_edited_time ? Date.parse(page.last_edited_time) : undefined,
    markdown: blocksToMarkdown(blocks),
    truncated: budget.truncated,
  };
};
