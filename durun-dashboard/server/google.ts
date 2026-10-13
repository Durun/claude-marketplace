// Internet Explorer のウィンドウに出す、2004 年ごろの Google のトップページと検索結果のページ。
// 検索結果は、Google の検索技術を使う今の Yahoo! JAPAN の検索から取る。
import { GOOGLE_SEARCH_PATH } from '../shared/retro.ts';
import { type Result, searchWeb } from './webSearch.ts';

const PAGE_SIZE = 10;
// 当時のロゴの文字ごとの色。
const LOGO = [...'Google'].map((c, i) => `<span style="color:${['#1851ce', '#c41200', '#f3c518', '#1851ce', '#30a72f', '#c41200'][i]}">${c}</span>`).join('');

const STYLE = `body{margin:8px;font-family:arial,sans-serif;font-size:13px;background:#fff;color:#000}
a:link{color:#0000cc}a:visited{color:#551a8b}
.logo{font:bold 32px "Book Antiqua",Georgia,"Times New Roman",serif;letter-spacing:-1px;text-shadow:2px 2px 1px #bbb;margin-right:12px}`;

const esc = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

/** start は、当時の Google と同じく 0 から数えた何件目からか。 */
const pageLink = (q: string, page: number, label: string): string => `<a href="${esc(`${GOOGLE_SEARCH_PATH}?${new URLSearchParams({ q, start: String((page - 1) * PAGE_SIZE) })}`)}">${label}</a>`;

/** 検索結果を、当時の Google の結果のページとして書く。search は Google の /search に付ける ? からの問い合わせ。 */
export const oldGoogle = (results: Result[], search: string): string => {
  const params = new URLSearchParams(search);
  const q = params.get('q') ?? '';
  const page = Math.floor((Number(params.get('start')) || 0) / PAGE_SIZE) + 1;
  const from = (page - 1) * PAGE_SIZE + 1;
  const pages = Array.from({ length: PAGE_SIZE }, (_, i) => (i + 1 === page ? `<b style="color:#a90a08">${i + 1}</b>` : pageLink(q, i + 1, String(i + 1)))).join(' ');
  return `<!doctype html><html><head><meta charset="utf-8"><title>Google 検索: ${esc(q)}</title><style>
${STYLE}
.bar{display:flex;justify-content:space-between;margin:8px 0 12px;padding:2px 4px;border-top:1px solid #3366cc;background:#e5ecf9}
.g{margin:0 0 14px;max-width:42em}.g>a{font-size:16px}.url{color:#008000}
.nav{margin:24px 0;text-align:center}
</style></head><body>
<form action="${GOOGLE_SEARCH_PATH}" style="display:flex;align-items:center"><span class="logo">${LOGO}</span><input name="q" size="41" value="${esc(q)}"> <input type="submit" value="Google 検索"></form>
<div class="bar"><b>ウェブ</b><span>${results.length ? `<b>${from}</b> - <b>${from + results.length - 1}</b> 件目` : ''} (<b>${esc(q)}</b>)</span></div>
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

/** search は Google の /search に付ける ? からの問い合わせで、q が検索語、start が 0 から数えた何件目からか。 */
const searchFor = (search: string, lang: string | undefined): Promise<Result[]> => {
  const params = new URLSearchParams(search);
  return searchWeb(params.get('q') ?? '', (Number(params.get('start')) || 0) + 1, lang);
};

export const fetchGoogle = async (query: string, lang: string | undefined): Promise<string> => oldGoogle(await searchFor(query, lang), query);

/** I'm Feeling Lucky で開く、検索結果の 1 件目の行き先。結果が無ければ undefined。 */
export const fetchLucky = async (query: string, lang: string | undefined): Promise<string | undefined> => (await searchFor(query, lang))[0]?.href;
