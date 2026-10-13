// Internet Explorer のウィンドウで、Wikipedia を昔の見た目で開くための中継の入り口。クライアントとサーバーの両方が使う。
export const WIKIPEDIA_PATH = '/api/wikipedia';

/** 中継してよい URL を、https の各言語版の Wikipedia に限る。ほかのサイトをサーバーに取りに行かせない。 */
export const isWikipedia = (url: URL): boolean => url.protocol === 'https:' && /^[a-z0-9-]+\.wikipedia\.org$/.test(url.hostname);

/** Wikipedia のページを中継で開く、ダッシュボードの中の URL。 */
export const wikipediaFrame = (url: string): string => `${WIKIPEDIA_PATH}?${new URLSearchParams({ url })}`;
