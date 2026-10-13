// Internet Explorer のウィンドウに出す、2005 年ごろの Yahoo! JAPAN のトップページと検索結果のページ。
// トピックスは今の Yahoo! JAPAN のトップから、検索結果は今の Yahoo! 検索から取る。
import { YAHOO, YAHOO_SEARCH_PATH, frameOf } from '../shared/retro.ts';
import { type Result, searchWeb } from './webSearch.ts';

const TIMEOUT_MS = 10_000;
const PAGE_SIZE = 10;
const MAX_TOPICS = 8;
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36';
// 当時のトップページに並んでいたカテゴリの一部。カテゴリ検索は終わっているので、押すとその言葉で検索する。
const CATEGORIES: [string, string[]][] = [
  ['エンターテインメント', ['映画', '音楽', 'テレビ', 'ゲーム']],
  ['趣味とスポーツ', ['スポーツ', '旅行', '自動車', 'アウトドア']],
  ['芸術と人文', ['文学', '写真', '歴史', '美術']],
  ['生活と文化', ['料理', '住まい', 'ファッション', 'ペット']],
  ['ビジネスと経済', ['企業', '就職', '投資', 'ショッピング']],
  ['ニュースと情報', ['新聞', '天気', '雑誌']],
  ['コンピュータとインターネット', ['ソフトウェア', 'ハードウェア', 'インターネット']],
  ['自然科学と技術', ['生物', '天文学', '工学']],
  ['健康と医学', ['病気', '医療', 'フィットネス']],
  ['教育', ['大学', '資格', '語学']],
];

const esc = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

const textOf = (html: string): string => html.replace(/<[^>]*>/g, '').replace(/\s+/g, ' ').trim().replaceAll('<', '&lt;');

const searchLink = (p: string, label = esc(p), b?: number): string => `<a href="${esc(`${YAHOO_SEARCH_PATH}?${new URLSearchParams(b ? { p, b: String(b) } : { p })}`)}">${label}</a>`;

const STYLE = `body{margin:0;background:#fff;color:#000;font:13px/1.5 "ＭＳ Ｐゴシック","MS PGothic",Osaka,sans-serif}
a:link{color:#0000de}a:visited{color:#660099}
.wrap{max-width:770px;margin:0 auto;padding:8px}
.logo{display:inline-block;font:bold 40px/1 Georgia,"Times New Roman",serif;letter-spacing:-1px;text-decoration:none;text-shadow:2px 2px 0 #ccc}
.logo:link,.logo:visited{color:#ff0033}
.logo small{margin-left:4px;font-size:16px;letter-spacing:0}
form{margin:8px 0}form input[name=p]{width:320px}
.foot{clear:both;margin-top:16px;padding-top:6px;border-top:1px solid #ccc;color:#666;font-size:11px;text-align:center}`;

const LOGO = `<a class="logo" href="${frameOf(YAHOO)}">YAHOO!<small>JAPAN</small></a>`;
const FOOT = '<div class="foot">Copyright (C) 2005 Yahoo Japan Corporation. All Rights Reserved.</div>';

const searchForm = (p = ''): string => `<form action="${YAHOO_SEARCH_PATH}"><input name="p" value="${esc(p)}"> <input type="submit" value="検索"></form>`;

type Topic = { href: string; title: string };

/** 今の Yahoo! JAPAN のトップから、トピックスの記事を取り出す。題は記事のリンクの中の h1 にある。 */
export const topicsOf = (html: string): Topic[] =>
  [...html.matchAll(/<a\b[^>]*?href="(https:\/\/news\.yahoo\.co\.jp\/pickup\/\d+)"[^>]*>([\s\S]*?)<\/a>/g)]
    .flatMap(([, href = '', inner = '']) => {
      const title = textOf(/<h1\b[^>]*>([\s\S]*?)<\/h1>/.exec(inner)?.[1] ?? '');
      return title ? [{ href: frameOf(href), title }] : [];
    })
    .slice(0, MAX_TOPICS);

export const oldYahooHome = (topics: Topic[]): string => {
  const categories = CATEGORIES.map(([name, subs]) => `<li><b>${searchLink(name)}</b><br><small>${subs.map((s) => searchLink(s)).join(', ')}...</small></li>`).join('');
  return `<!doctype html><html><head><meta charset="utf-8"><title>Yahoo! JAPAN</title><style>
${STYLE}
.top{text-align:center}
.cols{display:flex;gap:12px;margin-top:8px}
.box{border:1px solid #9cb2d6}.box>h2{margin:0;padding:2px 6px;background:#dce6f5;color:#333;font-size:13px}
.box ul{margin:6px 0;padding-left:20px}.box li{margin:3px 0}
.cat{flex:3}.cat ul{columns:2;list-style:none;padding-left:8px}.cat li{break-inside:avoid}
.topics{flex:2}
</style></head><body><div class="wrap">
<div class="top">${LOGO}${searchForm()}</div>
<div class="cols">
<div class="box cat"><h2>カテゴリ</h2><ul>${categories}</ul></div>
<div class="box topics"><h2>トピックス</h2><ul>${topics.map((t) => `<li><a href="${esc(t.href)}">${t.title}</a></li>`).join('')}</ul><p style="margin:0 6px 6px;text-align:right"><a href="${esc(frameOf('https://news.yahoo.co.jp/'))}">ニュース一覧</a></p></div>
</div>
${FOOT}
</div></body></html>`;
};

/** 検索結果を、当時の Yahoo! 検索の結果のページとして書く。search は Yahoo! 検索の ? からの問い合わせで、p が検索語、b が何件目からか。 */
export const oldYahooSearch = (results: Result[], search: string): string => {
  const params = new URLSearchParams(search);
  const p = params.get('p') ?? '';
  const from = Math.max(1, Number(params.get('b')) || 1);
  const page = Math.floor((from - 1) / PAGE_SIZE) + 1;
  const pages = Array.from({ length: PAGE_SIZE }, (_, i) => (i + 1 === page ? `<b>${i + 1}</b>` : searchLink(p, String(i + 1), i * PAGE_SIZE + 1))).join(' ');
  return `<!doctype html><html><head><meta charset="utf-8"><title>Yahoo!検索 - ${esc(p)}</title><style>
${STYLE}
.head{display:flex;align-items:center;gap:12px}.head .logo{font-size:26px}.head small{font-size:11px}
.bar{display:flex;justify-content:space-between;padding:2px 6px;border-top:2px solid #ff0033;background:#f0f0f0}
ol{padding-left:24px}li{margin:0 0 12px}.url{color:#008000}
.nav{text-align:center}
</style></head><body><div class="wrap">
<div class="head">${LOGO}${searchForm(p)}</div>
<div class="bar"><b>ウェブ検索結果</b><span>${results.length ? `<b>${esc(p)}</b> の検索結果 <b>${from}</b> - <b>${from + results.length - 1}</b> 件目` : ''}</span></div>
${results.length ? `<ol start="${from}">${results.map((r) => `<li><a href="${esc(r.href)}">${r.title}</a><br>${r.snippet ? `${r.snippet}<br>` : ''}<span class="url">${esc(r.display)}</span></li>`).join('')}</ol>` : `<p><b>${esc(p)}</b> に該当するページは見つかりませんでした。</p>`}
<div class="nav">${page > 1 ? `${searchLink(p, '前へ', from - PAGE_SIZE)} ` : ''}${pages}${results.length ? ` ${searchLink(p, '次へ', from + PAGE_SIZE)}` : ''}</div>
${FOOT}
</div></body></html>`;
};

export const fetchYahooHome = async (lang: string | undefined): Promise<string> => {
  const res = await fetch(YAHOO, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Yahoo! JAPAN が ${res.status} を返しました`);
  return oldYahooHome(topicsOf(await res.text()));
};

export const fetchYahooSearch = async (search: string, lang: string | undefined): Promise<string> => {
  const params = new URLSearchParams(search);
  return oldYahooSearch(await searchWeb(params.get('p') ?? '', Number(params.get('b')) || 1, lang), search);
};
