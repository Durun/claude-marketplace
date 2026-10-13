// Internet Explorer のウィンドウに出す、RETRO_SITES のサイトのページ。今の HTML から題・本文・見出しのリンクを抜き出し、2000 年代のポータルサイトの見た目で描き直す。
// 描き直したページのリンクは、同じく描き直せるサイトなら中継に向けるので、辿っても昔の見た目のままになる。
import { RETRO_SITES, frameOf, isRetroSite } from '../shared/retro.ts';

const TIMEOUT_MS = 10_000;
const MAX_LINKS = 60;
// これより短いリンクの文はメニューやタグとみなし、見出しに含めない。
const MIN_HEADLINE = 10;
const MAX_HEADLINE = 100;
// これより短い段落は、日付や「続きを読む」などとみなし、本文に含めない。
const MIN_PARAGRAPH = 40;
// 一部のサイトは、既定の User-Agent には中身の無いページを返す。
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36';

const esc = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

/** HTML の一部を、空白を詰めた文にする。文字は元の HTML が書いたものなので、実体参照はそのまま使える。タグの残りの < は逃がす。 */
const textOf = (html: string): string => html.replace(/<[^>]*>/g, '').replace(/\s+/g, ' ').trim().replaceAll('<', '&lt;');

const metaOf = (html: string, property: string): string | undefined => new RegExp(`<meta\\b[^>]*?property="${property}"[^>]*?content="([^"]*)"`, 'i').exec(html)?.[1];

type Link = { href: string; text: string };

/** 見出しらしいリンクを、出てくる順に重複を除いて集める。行き先は http(s) に限る。 */
const headlinesOf = (html: string, page: URL): Link[] => {
  const seen = new Set<string>([page.href]);
  return [...html.matchAll(/<a\b[^>]*?href="([^"#][^"]*)"[^>]*>([\s\S]*?)<\/a>/gi)]
    .flatMap(([, href = '', inner = '']) => {
      const url = URL.parse(href.replaceAll('&amp;', '&'), page);
      const text = textOf(inner);
      if ((url?.protocol !== 'https:' && url?.protocol !== 'http:') || text.length < MIN_HEADLINE || text.length > MAX_HEADLINE || seen.has(url.href)) return [];
      seen.add(url.href);
      return [{ href: frameOf(url.href), text }];
    })
    .slice(0, MAX_LINKS);
};

/** 長い段落を本文とみなす。 */
const paragraphsOf = (html: string): string[] =>
  [...html.matchAll(/<p\b[^>]*>([\s\S]*?)<\/p>/gi)].map(([, inner = '']) => textOf(inner)).filter((p) => p.length >= MIN_PARAGRAPH);

const columnOf = (links: Link[]): string => `<td valign="top" width="50%"><ul>${links.map((l) => `<li><a href="${esc(l.href)}">${l.text}</a></li>`).join('')}</ul></td>`;

/**
 * ページを 2000 年代のポータルサイトの見た目で書く。page は RETRO_SITES のサイトの URL。
 * ponytail: サイトごとの作りを見ずに、段落と長いリンクの文から本文と見出しを推し量る。崩れるサイトが出たら、そのサイトだけ抜き出し方を足す。
 */
export const renderRetro = (html: string, page: URL): string => {
  const site = RETRO_SITES[page.hostname] ?? { name: page.hostname, color: '#003399' };
  const top = page.pathname === '/';
  // トップページと記事の一覧は、見出しだけを並べる。
  const article = !top && !site.listing;
  const title = textOf(metaOf(html, 'og:title') ?? /<title\b[^>]*>([\s\S]*?)<\/title>/i.exec(html)?.[1] ?? '');
  const image = article ? URL.parse(metaOf(html, 'og:image')?.replaceAll('&amp;', '&') ?? '', page) : undefined;
  const code = html.replace(/<(script|style)\b[\s\S]*?<\/\1>/gi, '');
  const body = article ? paragraphsOf(code.replace(/<(nav|header|footer|aside)\b[\s\S]*?<\/\1>/gi, '')) : [];
  const links = headlinesOf(code, page);
  const half = Math.ceil(links.length / 2);
  const home = esc(frameOf(`${page.origin}/`));
  return `<!doctype html><html><head><meta charset="utf-8"><title>${title || esc(site.name)}</title><style>
body{margin:0;background:#fff;color:#000;font:12px/1.5 "ＭＳ Ｐゴシック","MS PGothic",Osaka,sans-serif}
a:link{color:#0000cc}a:visited{color:#660099}a:hover{color:#ff3300}
.wrap{max-width:760px;margin:0 auto}
.head{display:flex;align-items:flex-end;justify-content:space-between;padding:8px 4px 4px}
.logo{font:bold 26px/1 Verdana,"ＭＳ Ｐゴシック",sans-serif;letter-spacing:-1px;text-decoration:none}
.logo:link,.logo:visited{color:${site.color}}
.date{color:#666;font-size:10px}
.bar{padding:3px 8px;border-bottom:3px solid #ccc;background:${site.color};color:#fff;font-weight:bold}.bar a{color:#fff}
.box{clear:both;margin:10px 0;border:1px solid ${site.color}}
.box>h2{margin:0;padding:2px 6px;background:${site.color};color:#fff;font-size:12px}
.box ul{margin:6px 0;padding-left:18px;list-style:square}.box li{margin:2px 0}
h1{margin:12px 0 6px;padding-left:6px;border-left:6px solid ${site.color};font-size:16px}
.body{margin:0 4px}.body img{float:right;width:200px;margin:0 0 8px 12px;border:1px solid #999}.body p{margin:0 0 1em;text-indent:1em}
.foot{clear:both;margin-top:16px;padding:6px;border-top:1px solid #999;color:#666;text-align:center;font-size:10px}
</style></head><body><div class="wrap">
<div class="head"><a class="logo" href="${home}">${esc(site.name)}</a><span class="date">${new Date().toLocaleDateString('ja-JP', { dateStyle: 'full' })}</span></div>
<div class="bar"><a href="${home}">トップ</a> ｜ ${esc(page.hostname)}</div>
${top ? '' : `<h1>${title}</h1>`}
${body.length ? `<div class="body">${image ? `<img src="${esc(image.href)}" alt="">` : ''}${body.map((p) => `<p>${p}</p>`).join('')}</div>` : ''}
${links.length ? `<div class="box"><h2>${top ? '最新の見出し' : '関連リンク'}</h2><table width="100%"><tr>${columnOf(links.slice(0, half))}${columnOf(links.slice(half))}</tr></table></div>` : ''}
<div class="foot">Copyright (C) 2005 ${esc(site.name)} All Rights Reserved.</div>
</div></body></html>`;
};

/** url は RETRO_SITES のサイトのページ。ほかのサイトはサーバーに取りに行かせない。 */
export const fetchRetro = async (url: string, lang: string | undefined): Promise<string> => {
  const page = URL.parse(url);
  if (!page || !isRetroSite(page)) throw new Error('昔の見た目で描けるサイトの URL ではありません');
  const res = await fetch(page, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`${page.hostname} が ${res.status} を返しました`);
  return renderRetro(await res.text(), new URL(res.url));
};
