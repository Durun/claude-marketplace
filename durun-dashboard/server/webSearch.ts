// Internet Explorer の昔の Google と昔の Yahoo! に出す、ウェブの検索結果。どちらも今の Yahoo! JAPAN の検索から取る。
// Google は JavaScript を動かさないブラウザに結果を返さず、公式の Custom Search JSON API も新しい申し込みを締め切っている。Yahoo! JAPAN の検索は Google の検索技術を使っているので、結果が Google に近い。
import { frameOf } from '../shared/retro.ts';

const YAHOO_SEARCH = 'https://search.yahoo.co.jp/search';
const TIMEOUT_MS = 10_000;
const MAX_DISPLAY = 60;
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36';

/** href は描き直したページでの行き先で、display は緑の字で添える URL。title と snippet は強調の太字だけ残した HTML。 */
export type Result = { href: string; display: string; title: string; snippet: string };

/** 検索エンジンが返した HTML の一部を、強調の太字だけ残した文にする。文字は HTML として書かれたものなので、実体参照はそのまま使える。 */
const inline = (html: string): string => html.replace(/<(\/?)strong>/g, '<$1b>').replace(/<(?!\/?b>)[^>]*>/g, '');

/** 今の Yahoo! 検索の結果のページから、ウェブの結果を取り出す。広告やニュースなどの枠は除く。行き先は http(s) に限り、昔の見た目で描けるサイトは中継に向ける。 */
export const yahooResultsOf = (html: string): Result[] =>
  html.split('class="sw-Card Algo').slice(1).flatMap((card) => {
    const url = URL.parse(/<a href="([^"]*)" class="sw-Card__titleInner"/.exec(card)?.[1]?.replaceAll('&amp;', '&') ?? '');
    if (url?.protocol !== 'https:' && url?.protocol !== 'http:') return [];
    const display = url.href.replace(/^https?:\/\//, '');
    return [{ href: frameOf(url.href), display: display.length > MAX_DISPLAY ? `${display.slice(0, MAX_DISPLAY)}...` : display, title: inline(/<h3\b[^>]*>([\s\S]*?)<\/h3>/.exec(card)?.[1] ?? ''), snippet: inline(/<p class="sw-Card__summary">([\s\S]*?)<\/p>/.exec(card)?.[1] ?? '') }];
  });

/** q を、from 件目（1 から数える）から検索する。 */
export const searchWeb = async (q: string, from: number, lang: string | undefined): Promise<Result[]> => {
  const res = await fetch(`${YAHOO_SEARCH}?${new URLSearchParams({ p: q, b: String(from) })}`, { headers: { 'user-agent': USER_AGENT, 'accept-language': lang ?? 'ja' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Yahoo! 検索が ${res.status} を返しました`);
  return yahooResultsOf(await res.text());
};
