// Internet Explorer のウィンドウで、サイトを昔の見た目で開くための中継の入り口。クライアントとサーバーの両方が使う。
// 中継は、サーバーがページを取ってきて描き直し、ダッシュボードのオリジンから出す。
export const BING_PATH = '/api/bing/search';
export const GOOGLE_HOME_PATH = '/api/google';
/** Internet Explorer の既定のページ。 */
export const GOOGLE = 'https://www.google.com/';
export const WIKIPEDIA_PATH = '/api/wikipedia';
export const RETRO_PATH = '/api/retro';

/** 2000 年代の見た目で描き直すサイト。キーはホスト名で、color はヘッダーの帯の色。listing は、どのページも記事の一覧で、段落を本文として出さないサイト。 */
export const RETRO_SITES: Record<string, { name: string; color: string; listing?: boolean }> = {
  'b.hatena.ne.jp': { name: 'はてなブックマーク', color: '#3d6ec8', listing: true },
  'www.weblio.jp': { name: 'Weblio 辞書', color: '#ef8200' },
  'dic.nicovideo.jp': { name: 'ニコニコ大百科', color: '#555555' },
  'ameblo.jp': { name: 'アメーバブログ', color: '#3c9a3c' },
  'news.livedoor.com': { name: 'livedoor ニュース', color: '#5ca800' },
  'www.excite.co.jp': { name: 'エキサイト', color: '#d50000' },
  'www.asahi.com': { name: 'asahi.com', color: '#00337a' },
  'mainichi.jp': { name: '毎日jp', color: '#003f8e' },
  'www.sankei.com': { name: 'MSN産経ニュース', color: '#004ea2' },
  'www.jiji.com': { name: '時事ドットコム', color: '#00467f' },
  'www.47news.jp': { name: '47NEWS', color: '#ea5504' },
  'pc.watch.impress.co.jp': { name: 'PC Watch', color: '#003399' },
  'www.4gamer.net': { name: '4Gamer.net', color: '#336600' },
  'www.famitsu.com': { name: 'ファミ通.com', color: '#e60012' },
};

/** 中継してよい URL を、https の各言語版の Wikipedia に限る。ほかのサイトをサーバーに取りに行かせない。 */
export const isWikipedia = (url: URL): boolean => url.protocol === 'https:' && /^[a-z0-9-]+\.wikipedia\.org$/.test(url.hostname);

export const isRetroSite = (url: URL): boolean => url.protocol === 'https:' && Object.hasOwn(RETRO_SITES, url.hostname);

export const wikipediaFrame = (url: string): string => `${WIKIPEDIA_PATH}?${new URLSearchParams({ url })}`;

/** url を Internet Explorer のウィンドウに出すときの URL。昔の見た目で描けるサイトは中継に向け、それ以外はそのまま開く。 */
export const frameOf = (url: string): string => {
  const u = URL.parse(url);
  if (!u) return url;
  // Google は JavaScript を動かさないブラウザに検索結果を返さないので、トップと検索は、Bing の結果を昔の Google の見た目で描く中継に向ける。
  if (u.origin === 'https://www.google.com' && (u.pathname === '/' || u.pathname === '/webhp')) return GOOGLE_HOME_PATH;
  if ((u.origin === 'https://www.google.com' || u.origin === 'https://www.bing.com') && u.pathname === '/search') return `${BING_PATH}${u.search}`;
  if (isWikipedia(u)) return wikipediaFrame(u.href);
  return isRetroSite(u) ? `${RETRO_PATH}?${new URLSearchParams({ url: u.href })}` : url;
};

/** frameOf の逆で、中継の URL から、ウィンドウが開いているサイトの URL を出す。中継でなければ undefined。 */
export const siteOfFrame = (frame: URL): string | undefined => {
  if (frame.pathname === GOOGLE_HOME_PATH) return GOOGLE;
  if (frame.pathname === BING_PATH) return `${GOOGLE}search${frame.search}`;
  return frame.pathname === WIKIPEDIA_PATH || frame.pathname === RETRO_PATH ? (frame.searchParams.get('url') ?? undefined) : undefined;
};
