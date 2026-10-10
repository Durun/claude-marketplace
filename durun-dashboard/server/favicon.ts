// 資料に並べるリンクの favicon。サイトのトップページが <link rel="icon"> で指すものを、無ければ /favicon.ico を返す。
// claude.ai などは Cross-Origin-Resource-Policy で他のサイトの画像として読ませないので、画面から直接は読まず、サーバーが取ってくる。
const TIMEOUT_MS = 5_000;
const MAX_ICON_BYTES = 256 * 1024;
// Notion などは、既定の User-Agent をボットとみなして断る。
const HEADERS = { 'user-agent': 'Mozilla/5.0' };

export type Icon = { type: string; body: Buffer };

/** rel に icon を含む最初の <link> の href を、base から解決する。apple-touch-icon などは含めない。 */
export const findIcon = (html: string, base: string): string | undefined => {
  for (const tag of html.match(/<link\b[^>]*>/gi) ?? []) {
    const rel = /\brel\s*=\s*["']?([^"'>]*)/i.exec(tag)?.[1]?.toLowerCase().split(/\s+/);
    const href = /\bhref\s*=\s*["']?([^"'\s>]+)/i.exec(tag)?.[1]?.replaceAll('&amp;', '&');
    if (rel?.includes('icon') && href && URL.canParse(href, base)) return new URL(href, base).href;
  }
  return undefined;
};

const get = (url: string): Promise<Response> => fetch(url, { headers: HEADERS, signal: AbortSignal.timeout(TIMEOUT_MS) });

const iconUrl = async (origin: string): Promise<string> => {
  try {
    const res = await get(origin);
    const icon = findIcon(await res.text(), res.url);
    if (icon) return icon;
  } catch {
    // トップページを開けないサイトも、/favicon.ico はあることが多い。
  }
  return `${origin}/favicon.ico`;
};

const download = async (origin: string): Promise<Icon | undefined> => {
  try {
    const res = await get(await iconUrl(origin));
    const type = res.headers.get('content-type') ?? '';
    const body = Buffer.from(await res.arrayBuffer());
    return res.ok && type.startsWith('image/') && body.length <= MAX_ICON_BYTES ? { type, body } : undefined;
  } catch {
    return undefined;
  }
};

// ponytail: 取れなかったことも覚えるので、一時的に取れなかったサイトは、サーバーを起動し直すまで favicon が出ない。困るなら失敗だけ時間で忘れる。
const icons = new Map<string, Promise<Icon | undefined>>();

/** サイトごとに一度だけ取り、サーバーが動いている間は覚えておく。 */
export const faviconOf = (origin: string): Promise<Icon | undefined> => {
  const known = icons.get(origin);
  if (known) return known;
  const icon = download(origin);
  icons.set(origin, icon);
  return icon;
};
