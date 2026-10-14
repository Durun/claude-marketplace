// 資料に足したリンクの題名。一般に公開しているページの <title> を読む。
import { notionPageId } from '../shared/types.ts';
import { jiraKeyOfUrl, jiraTitle } from './integrations.ts';

const TIMEOUT_MS = 5_000;
// Notion などは、既定の User-Agent をボットとみなして断る。
const HEADERS = { 'user-agent': 'Mozilla/5.0' };
const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };

/** 最初の <title> の中身。文字参照を戻し、空白をまとめる。 */
export const findTitle = (html: string): string | undefined => {
  const raw = /<title\b[^>]*>([^<]*)<\/title>/i.exec(html)?.[1];
  const title = raw
    ?.replace(/&#x([\da-f]+);/gi, (_, n: string) => String.fromCodePoint(Number.parseInt(n, 16)))
    .replace(/&#(\d+);/g, (_, n: string) => String.fromCodePoint(Number(n)))
    .replace(/&(\w+);/g, (m, e: string) => ENTITIES[e.toLowerCase()] ?? m)
    .replace(/\s+/g, ' ')
    .trim();
  return title || undefined;
};

/**
 * ページの題名。読めないページとエラーのページは undefined にする。
 * Notion のページと claude.ai の artifact は、画面が API と公開したときの記録から題名を付けるので読まない。
 * JIRA の課題の画面は、ログインしないと <title> が「Jira」になるので、API から課題のキーと題名を読む。
 */
export const pageTitleOf = async (url: URL): Promise<string | undefined> => {
  if (notionPageId(url) || url.hostname === 'claude.ai') return undefined;
  const jira = jiraKeyOfUrl(url);
  if (jira) return jiraTitle(jira).catch(() => undefined);
  try {
    const res = await fetch(url, { headers: HEADERS, signal: AbortSignal.timeout(TIMEOUT_MS) });
    if (!res.ok || !res.headers.get('content-type')?.includes('text/html')) return undefined;
    return findTitle(await res.text());
  } catch {
    return undefined;
  }
};
