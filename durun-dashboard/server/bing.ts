// Internet Explorer のウィンドウに出す Bing の検索結果。Bing は結果のリンクに必ず target="_blank" を付けるので、iframe にそのまま出すと押したリンクが別のタブで開く。
// サーバーが取ってきてスクリプトを除き、リンクを Bing の計測用のリダイレクトから行き先そのものへ書き換えて、ウィンドウの中で開けるようにする。
export const BING_PATH = '/api/bing/search';
const BING = 'https://www.bing.com';
const TIMEOUT_MS = 10_000;
// Bing は既定の User-Agent には結果の少ない簡易なページを返す。
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36';

const attr = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('"', '&quot;');

/** /ck/a?...&u=a1<base64url> の u から、行き先の URL を取り出す。http(s) でなければ書き換えない。 */
const destinationOf = (href: string): string | undefined => {
  const u = new URLSearchParams(href.replaceAll('&amp;', '&').split('?')[1]).get('u');
  if (!u?.startsWith('a1')) return undefined;
  const url = URL.parse(Buffer.from(u.slice(2), 'base64url').toString());
  return url?.protocol === 'https:' || url?.protocol === 'http:' ? url.href : undefined;
};

/**
 * 結果のページを、ウィンドウの中で辿れる形にする。self はダッシュボードのオリジン。
 * 画像などは base で Bing から読み、検索欄と次のページは、この中継を通す。
 */
export const rewriteBing = (html: string, self: string): string =>
  html
    .replace(/<script\b[\s\S]*?<\/script>/gi, '')
    .replace(/\starget="_blank"/gi, '')
    .replace(/href="((?:https:\/\/www\.bing\.com)?\/ck\/a\?[^"]*)"/g, (all, href: string) => {
      const to = destinationOf(href);
      return to ? `href="${attr(to)}"` : all;
    })
    .replace(/(href|action)="(?:https:\/\/www\.bing\.com)?\/search\b/g, `$1="${attr(self + BING_PATH)}`)
    .replace(/<head\b[^>]*>/i, `$&<base href="${BING}/">`);

/** search は、Bing の /search に付ける ? からの問い合わせ。 */
export const fetchBing = async (search: string, lang: string | undefined, self: string): Promise<string> => {
  const res = await fetch(`${BING}/search${search}`, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Bing が ${res.status} を返しました`);
  return rewriteBing(await res.text(), self);
};
