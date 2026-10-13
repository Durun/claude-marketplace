// Internet Explorer のウィンドウに出す Wikipedia を、2004〜2010 年ごろの既定の見た目の MonoBook で描く。
// MonoBook は ?useskin=monobook で選べるが、ページの中のリンクはこの指定を引き継がないので、サーバーが取ってきて記事へのリンクを中継に向け直す。
import { isWikipedia, wikipediaFrame } from '../shared/retro.ts';

const TIMEOUT_MS = 10_000;
// Wikimedia は User-Agent の無い取得を断る。
const USER_AGENT = 'durun-dashboard';

const attr = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('"', '&quot;');

/**
 * 記事へのリンクを中継に向け、スクリプトを除く。here はこのページを開いた中継の URL で、self はダッシュボードのオリジン。
 * 画像や CSS は base で Wikipedia から読む。ページ内の #見出し へのリンクは、base に引かれて Wikipedia へ移らないよう here に付け直す。
 */
export const rewriteWikipedia = (html: string, page: URL, here: string, self: string): string =>
  html
    .replace(/<script\b[\s\S]*?<\/script>/gi, '')
    .replace(/href="((?:https:)?\/\/[a-z0-9-]+\.wikipedia\.org)?(\/wiki\/[^"]*)"/g, (_, origin = '', path: string) => {
      const to = new URL((origin + path).replaceAll('&amp;', '&'), page).href;
      return `href="${attr(self + wikipediaFrame(to))}"`;
    })
    .replace(/href="#/g, `href="${attr(here)}#`)
    .replace(/<head\b[^>]*>/i, `$&<base href="${attr(page.href)}">`);

/** url は Wikipedia のページ。here と self は rewriteWikipedia と同じ。 */
export const fetchWikipedia = async (url: string, lang: string | undefined, here: string, self: string): Promise<string> => {
  const page = URL.parse(url);
  if (!page || !isWikipedia(page)) throw new Error('Wikipedia の URL ではありません');
  const skinned = new URL(page);
  skinned.searchParams.set('useskin', 'monobook');
  const res = await fetch(skinned, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Wikipedia が ${res.status} を返しました`);
  return rewriteWikipedia(await res.text(), page, here, self);
};
