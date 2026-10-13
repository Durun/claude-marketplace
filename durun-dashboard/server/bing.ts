// Internet Explorer のウィンドウに出す、2004 年ごろの Google のトップページと、Bing の検索結果をその頃の Google の結果のページの見た目で描き直したもの。
// Bing は結果のリンクに target="_blank" を付けるので、そのまま出すと押したリンクが別のタブで開く。描き直したページのリンクはウィンドウの中で開く。
import { BING_PATH, frameOf } from '../shared/retro.ts';

const BING = 'https://www.bing.com';
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

/** Bing の HTML の一部を、強調の太字だけ残した文にする。文字は Bing が HTML として書いたものなので、実体参照はそのまま使える。 */
const inline = (html: string): string => html.replace(/<(\/?)strong>/g, '<$1b>').replace(/<(?!\/?b>)[^>]*>/g, '');

type Result = { href: string; display: string; title: string; snippet: string };

const resultsOf = (html: string): Result[] =>
  html.split('<li class="b_algo"').slice(1).flatMap((item) => {
    const a = /<h2\b[^>]*>\s*<a\b[^>]*?href="([^"]*)"[^>]*>([\s\S]*?)<\/a>/.exec(item);
    const href = a && linkOf(a[1] ?? '');
    if (!a || !href) return [];
    const display = destinationOf(a[1] ?? '').replace(/^https?:\/\//, '');
    return [{ href, display: display.length > 60 ? `${display.slice(0, 60)}...` : display, title: inline(a[2] ?? ''), snippet: inline(/<p\b[^>]*>([\s\S]*?)<\/p>/.exec(item)?.[1] ?? '') }];
  });

const pageLink = (q: string, page: number, label: string): string => `<a href="${esc(`${BING_PATH}?${new URLSearchParams({ q, first: String((page - 1) * PAGE_SIZE + 1) })}`)}">${label}</a>`;

/** Bing の結果のページから結果を抜き出し、当時の Google の結果のページとして書く。search は Bing の /search に付ける ? からの問い合わせ。 */
export const oldGoogle = (html: string, search: string): string => {
  const params = new URLSearchParams(search);
  const q = params.get('q') ?? '';
  const page = Math.floor((Number(params.get('first') ?? 1) - 1) / PAGE_SIZE) + 1 || 1;
  const results = resultsOf(html);
  const count = /<span class="sb_count"[^>]*>([^<]*)<\/span>/.exec(html)?.[1];
  const from = (page - 1) * PAGE_SIZE + 1;
  const pages = Array.from({ length: PAGE_SIZE }, (_, i) => (i + 1 === page ? `<b style="color:#a90a08">${i + 1}</b>` : pageLink(q, i + 1, String(i + 1)))).join(' ');
  return `<!doctype html><html><head><meta charset="utf-8"><title>Google 検索: ${esc(q)}</title><style>
${STYLE}
.bar{display:flex;justify-content:space-between;margin:8px 0 12px;padding:2px 4px;border-top:1px solid #3366cc;background:#e5ecf9}
.g{margin:0 0 14px;max-width:42em}.g>a{font-size:16px}.url{color:#008000}
.nav{margin:24px 0;text-align:center}
</style></head><body>
<form action="${BING_PATH}" style="display:flex;align-items:center"><span class="logo">${LOGO}</span><input name="q" size="41" value="${esc(q)}"> <input type="submit" value="Google 検索"></form>
<div class="bar"><b>ウェブ</b><span>${results.length ? `${count ? `${esc(count)} 中 ` : ''}<b>${from}</b> - <b>${from + results.length - 1}</b> 件目` : ''} (<b>${esc(q)}</b>)</span></div>
${results.length ? results.map((r) => `<p class="g"><a href="${esc(r.href)}">${r.title}</a><br>${r.snippet ? `${r.snippet}<br>` : ''}<span class="url">${esc(r.display)}</span></p>`).join('\n') : `<p><b>${esc(q)}</b> に該当するページが見つかりませんでした。</p>`}
<div class="nav">${page > 1 ? `${pageLink(q, page - 1, '前へ')} ` : ''}${pages} ${results.length ? pageLink(q, page + 1, '次へ') : ''}</div>
</body></html>`;
};

/** 当時の Google のトップページ。検索欄は Bing の結果を描き直す中継に送る。 */
export const OLD_GOOGLE_HOME = `<!doctype html><html><head><meta charset="utf-8"><title>Google</title><style>
${STYLE}
form{display:flex;flex-direction:column;align-items:center;gap:12px;margin-top:22vh}
.logo{margin:0;font-size:72px;letter-spacing:-3px;text-shadow:3px 3px 2px #bbb}
.foot{margin-top:40px;color:#666;font-size:11px;text-align:center}
</style></head><body>
<form action="${BING_PATH}"><h1 class="logo">${LOGO}</h1><input name="q" size="55" autofocus><span><input type="submit" value="Google 検索"> <input type="submit" name="btnI" value="I'm Feeling Lucky"></span></form>
<p class="foot">&copy;2005 Google</p>
</body></html>`;

const fetchPage = async (search: string, lang: string | undefined): Promise<string> => {
  const res = await fetch(`${BING}/search${search}`, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Bing が ${res.status} を返しました`);
  return res.text();
};

export const fetchBing = async (search: string, lang: string | undefined): Promise<string> => oldGoogle(await fetchPage(search, lang), search);

/** I'm Feeling Lucky で開く、検索結果の 1 件目の行き先。結果が無ければ undefined。 */
export const luckyOf = (html: string): string | undefined => resultsOf(html)[0]?.href;

export const fetchLucky = async (search: string, lang: string | undefined): Promise<string | undefined> => luckyOf(await fetchPage(search, lang));
