// Internet Explorer のウィンドウに出す、2004 年ごろの Google のトップページと検索結果のページ。
// 検索結果は、Google の検索技術を使う今の Yahoo! JAPAN の検索から取る。
import { GOOGLE_SEARCH_PATH } from '../shared/retro.ts';
import { type Result, searchWeb } from './webSearch.ts';

const PAGE_SIZE = 10;
// 当時のロゴの色。
const BLUE = '#1851ce';
const RED = '#c41200';
const YELLOW = '#f3c518';
const GREEN = '#30a72f';

const letter = (c: string, color: string): string => `<span style="color:${color}">${c}</span>`;
const LOGO = [...'Google'].map((c, i) => letter(c, [BLUE, RED, YELLOW, BLUE, GREEN, RED][i] ?? BLUE)).join('');

const STYLE = `body{margin:8px;font-family:arial,sans-serif;font-size:13px;background:#fff;color:#000}
a:link{color:#0000cc}a:visited{color:#551a8b}
.logo{font:bold 32px "Book Antiqua",Georgia,"Times New Roman",serif;letter-spacing:-1px;text-shadow:2px 2px 1px #bbb;margin-right:12px}`;

const esc = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

/** start は、当時の Google と同じく 0 から数えた何件目からか。 */
const pageHref = (q: string, page: number): string => esc(`${GOOGLE_SEARCH_PATH}?${new URLSearchParams({ q, start: String((page - 1) * PAGE_SIZE) })}`);

/** ロゴの文字の下に添え字を置いた、ページ送りの 1 マス。href があれば、文字と添え字をまとめてリンクにする。 */
const pagerCell = (letters: string, label: string, href?: string): string => {
  const inner = `<span class="pl">${letters}</span><br><span class="pn">${label}</span>`;
  return `<td>${href ? `<a href="${href}">${inner}</a>` : inner}</td>`;
};

/** 当時の結果のページの下に並んだ「Gooooooooogle」のページ送り。o の 1 つずつが 1 ページで、今のページの o だけ赤い。 */
const pager = (q: string, page: number, hasNext: boolean): string => {
  const pages = Array.from({ length: PAGE_SIZE }, (_, i) =>
    i + 1 === page ? pagerCell(letter('o', RED), `<b style="color:${RED}">${i + 1}</b>`) : pagerCell(letter('o', YELLOW), String(i + 1), pageHref(q, i + 1)),
  );
  const prev = pagerCell(letter('G', BLUE), page > 1 ? '<b>前へ</b>' : '', page > 1 ? pageHref(q, page - 1) : undefined);
  const next = pagerCell(letter('g', BLUE) + letter('l', GREEN) + letter('e', RED), hasNext ? '<b>次へ</b>' : '', hasNext ? pageHref(q, page + 1) : undefined);
  return `<table class="pager"><tr>${prev}${pages.join('')}${next}</tr></table>`;
};

/** 検索結果を、当時の Google の結果のページとして書く。search は Google の /search に付ける ? からの問い合わせ。 */
export const oldGoogle = (results: Result[], search: string): string => {
  const params = new URLSearchParams(search);
  const q = params.get('q') ?? '';
  const page = Math.floor((Number(params.get('start')) || 0) / PAGE_SIZE) + 1;
  const from = (page - 1) * PAGE_SIZE + 1;
  return `<!doctype html><html><head><meta charset="utf-8"><title>Google 検索: ${esc(q)}</title><style>
${STYLE}
.bar{display:flex;justify-content:space-between;margin:8px 0 12px;padding:2px 4px;border-top:1px solid #3366cc;background:#e5ecf9}
.g{margin:0 0 14px;max-width:42em}.g>a{font-size:16px}.url{color:#008000}
.pager{margin:24px auto;border-collapse:collapse;text-align:center}.pager td{padding:0;vertical-align:top}
.pager a{text-decoration:none}.pager a .pn{text-decoration:underline}
.pl{font:bold 32px/1 "Book Antiqua",Georgia,"Times New Roman",serif;letter-spacing:-2px;text-shadow:2px 2px 1px #bbb}.pn{font-size:13px}
</style></head><body>
<form action="${GOOGLE_SEARCH_PATH}" style="display:flex;align-items:center"><span class="logo">${LOGO}</span><input name="q" size="41" value="${esc(q)}"> <input type="submit" value="Google 検索"></form>
<div class="bar"><b>ウェブ</b><span>${results.length ? `<b>${from}</b> - <b>${from + results.length - 1}</b> 件目` : ''} (<b>${esc(q)}</b>)</span></div>
${results.length ? results.map((r) => `<p class="g"><a href="${esc(r.href)}">${r.title}</a><br>${r.snippet ? `${r.snippet}<br>` : ''}<span class="url">${esc(r.display)}</span></p>`).join('\n') : `<p><b>${esc(q)}</b> に該当するページが見つかりませんでした。</p>`}
${pager(q, page, results.length > 0)}
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
