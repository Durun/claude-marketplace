// Internet Explorer のウィンドウに出す、2004 年ごろの Google のトップページと検索結果のページ。
// 検索結果は Google の Custom Search JSON API から取る。API キーが無ければ、Bing の結果を代わりに出す。
// Bing は結果のリンクに target="_blank" を付けるので、そのまま出すと押したリンクが別のタブで開く。描き直したページのリンクはウィンドウの中で開く。
import { GOOGLE_SEARCH_PATH, frameOf } from '../shared/retro.ts';

const BING = 'https://www.bing.com';
const GOOGLE_API = 'https://www.googleapis.com/customsearch/v1';
const GOOGLE_API_KEY = process.env.GOOGLE_SEARCH_API_KEY;
const GOOGLE_ENGINE_ID = process.env.GOOGLE_SEARCH_ENGINE_ID;
const TIMEOUT_MS = 10_000;
const PAGE_SIZE = 10;
// Bing は既定の User-Agent には結果の少ない簡易なページを返す。
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36';
// 当時のロゴの文字ごとの色。
const LOGO = [...'Google'].map((c, i) => `<span style="color:${['#1851ce', '#c41200', '#f3c518', '#1851ce', '#30a72f', '#c41200'][i]}">${c}</span>`).join('');

const STYLE = `body{margin:8px;font-family:arial,sans-serif;font-size:13px;background:#fff;color:#000}
a:link{color:#0000cc}a:visited{color:#551a8b}
.logo{font:bold 32px "Book Antiqua",Georgia,"Times New Roman",serif;letter-spacing:-1px;text-shadow:2px 2px 1px #bbb;margin-right:12px}`;

const esc = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

/** /ck/a?...&u=a1<base64url> は Bing の計測用のリダイレクトなので、u から行き先の URL を取り出す。 */
const destinationOf = (href: string): string => {
  const u = new URLSearchParams(href.replaceAll('&amp;', '&').split('?')[1]).get('u');
  return u?.startsWith('a1') ? Buffer.from(u.slice(2), 'base64url').toString() : href;
};

/** 結果の行き先を、http(s) に限る。昔の見た目で描けるサイトは中継に向ける。 */
const linkOf = (href: string): string | undefined => {
  const url = URL.parse(destinationOf(href));
  if (url?.protocol !== 'https:' && url?.protocol !== 'http:') return undefined;
  return frameOf(url.href);
};

const displayOf = (url: string): string => {
  const display = url.replace(/^https?:\/\//, '');
  return display.length > 60 ? `${display.slice(0, 60)}...` : display;
};

/** 検索エンジンが返した HTML の一部を、強調の太字だけ残した文にする。文字は HTML として書かれたものなので、実体参照はそのまま使える。 */
export const inline = (html: string): string => html.replace(/<(\/?)strong>/g, '<$1b>').replace(/<(?!\/?b>)[^>]*>/g, '');

export type Result = { href: string; display: string; title: string; snippet: string };

/** Bing の結果のページから、結果を取り出す。 */
export const bingResultsOf = (html: string): Result[] =>
  html.split('<li class="b_algo"').slice(1).flatMap((item) => {
    const a = /<h2\b[^>]*>\s*<a\b[^>]*?href="([^"]*)"[^>]*>([\s\S]*?)<\/a>/.exec(item);
    const href = a && linkOf(a[1] ?? '');
    if (!a || !href) return [];
    return [{ href, display: displayOf(destinationOf(a[1] ?? '')), title: inline(a[2] ?? ''), snippet: inline(/<p\b[^>]*>([\s\S]*?)<\/p>/.exec(item)?.[1] ?? '') }];
  });

type GoogleItem = { link?: string; htmlTitle?: string; htmlSnippet?: string };

/** Custom Search JSON API の items を、結果にする。 */
export const googleResultsOf = (items: GoogleItem[]): Result[] =>
  items.flatMap((item) => {
    const href = linkOf(item.link ?? '');
    return href ? [{ href, display: displayOf(item.link ?? ''), title: inline(item.htmlTitle ?? ''), snippet: inline(item.htmlSnippet ?? '') }] : [];
  });

/** 検索の結果。count は「約 n 件」の n で、notice は結果の出どころが Google でないときの断り書き。 */
type Found = { results: Result[]; count?: string; notice?: string };

/** start は、当時の Google と同じく 0 から数えた何件目からか。 */
const pageLink = (q: string, page: number, label: string): string => `<a href="${esc(`${GOOGLE_SEARCH_PATH}?${new URLSearchParams({ q, start: String((page - 1) * PAGE_SIZE) })}`)}">${label}</a>`;

/** 検索結果を、当時の Google の結果のページとして書く。search は Google の /search に付ける ? からの問い合わせ。 */
export const oldGoogle = ({ results, count, notice }: Found, search: string): string => {
  const params = new URLSearchParams(search);
  const q = params.get('q') ?? '';
  const page = Math.floor((Number(params.get('start')) || 0) / PAGE_SIZE) + 1;
  const from = (page - 1) * PAGE_SIZE + 1;
  const pages = Array.from({ length: PAGE_SIZE }, (_, i) => (i + 1 === page ? `<b style="color:#a90a08">${i + 1}</b>` : pageLink(q, i + 1, String(i + 1)))).join(' ');
  return `<!doctype html><html><head><meta charset="utf-8"><title>Google 検索: ${esc(q)}</title><style>
${STYLE}
.bar{display:flex;justify-content:space-between;margin:8px 0 12px;padding:2px 4px;border-top:1px solid #3366cc;background:#e5ecf9}
.g{margin:0 0 14px;max-width:42em}.g>a{font-size:16px}.url{color:#008000}
.notice{color:#666;font-size:11px}
.nav{margin:24px 0;text-align:center}
</style></head><body>
<form action="${GOOGLE_SEARCH_PATH}" style="display:flex;align-items:center"><span class="logo">${LOGO}</span><input name="q" size="41" value="${esc(q)}"> <input type="submit" value="Google 検索"></form>
<div class="bar"><b>ウェブ</b><span>${results.length ? `${count ? `約 <b>${esc(count)}</b> 件中 ` : ''}<b>${from}</b> - <b>${from + results.length - 1}</b> 件目` : ''} (<b>${esc(q)}</b>)</span></div>
${notice ? `<p class="notice">${esc(notice)}</p>` : ''}
${results.length ? results.map((r) => `<p class="g"><a href="${esc(r.href)}">${r.title}</a><br>${r.snippet ? `${r.snippet}<br>` : ''}<span class="url">${esc(r.display)}</span></p>`).join('\n') : `<p><b>${esc(q)}</b> に該当するページが見つかりませんでした。</p>`}
<div class="nav">${page > 1 ? `${pageLink(q, page - 1, '前へ')} ` : ''}${pages} ${results.length ? pageLink(q, page + 1, '次へ') : ''}</div>
</body></html>`;
};

/** 当時の Google のトップページ。 */
export const OLD_GOOGLE_HOME = `<!doctype html><html><head><meta charset="utf-8"><title>Google</title><style>
${STYLE}
form{display:flex;flex-direction:column;align-items:center;gap:12px;margin-top:22vh}
.logo{margin:0;font-size:72px;letter-spacing:-3px;text-shadow:3px 3px 2px #bbb}
.foot{margin-top:40px;color:#666;font-size:11px;text-align:center}
</style></head><body>
<form action="${GOOGLE_SEARCH_PATH}"><h1 class="logo">${LOGO}</h1><input name="q" size="55" autofocus><span><input type="submit" value="Google 検索"> <input type="submit" name="btnI" value="I'm Feeling Lucky"></span></form>
<p class="foot">&copy;2005 Google</p>
</body></html>`;

const searchGoogle = async (q: string, start: number, lang: string | undefined): Promise<Found> => {
  const query = new URLSearchParams({ key: GOOGLE_API_KEY ?? '', cx: GOOGLE_ENGINE_ID ?? '', q, start: String(start + 1), hl: 'ja' });
  const res = await fetch(`${GOOGLE_API}?${query}`, { headers: { 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  const body = (await res.json()) as { items?: GoogleItem[]; searchInformation?: { formattedTotalResults?: string }; error?: { message?: string } };
  if (!res.ok) throw new Error(`Google の検索の API が ${res.status} を返しました: ${body.error?.message ?? ''}`);
  return { results: googleResultsOf(body.items ?? []), count: body.searchInformation?.formattedTotalResults };
};

const searchBing = async (q: string, start: number, lang: string | undefined): Promise<Found> => {
  const res = await fetch(`${BING}/search?${new URLSearchParams({ q, first: String(start + 1) })}`, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Bing が ${res.status} を返しました`);
  return { results: bingResultsOf(await res.text()), notice: '~/.durun-dashboard/.env.local に GOOGLE_SEARCH_API_KEY と GOOGLE_SEARCH_ENGINE_ID が無いので、Bing の検索結果を出しています。' };
};

/** search は Google の /search に付ける ? からの問い合わせで、q が検索語、start が 0 から数えた何件目からか。 */
const searchFor = (search: string, lang: string | undefined): Promise<Found> => {
  const params = new URLSearchParams(search);
  const find = GOOGLE_API_KEY && GOOGLE_ENGINE_ID ? searchGoogle : searchBing;
  return find(params.get('q') ?? '', Number(params.get('start')) || 0, lang);
};

export const fetchGoogle = async (query: string, lang: string | undefined): Promise<string> => oldGoogle(await searchFor(query, lang), query);

/** I'm Feeling Lucky で開く、検索結果の 1 件目の行き先。結果が無ければ undefined。 */
export const fetchLucky = async (query: string, lang: string | undefined): Promise<string | undefined> => (await searchFor(query, lang)).results[0]?.href;
