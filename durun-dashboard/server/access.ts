// どの名前で届いたリクエストを受けるかを決める。
// サーバーは 127.0.0.1 でだけ listen するが、DNS リバインディングと他サイトからの書き込みを防ぐため、Host と Origin も確かめる。
// tailscale serve は Host を書き換えずに中継するので、tailnet から届いたものは、この端末の tailnet での名前と比べる。
import { platform } from './platform.ts';

const LOCAL_HOSTNAMES = ['127.0.0.1', 'localhost'];
// tailnet での名前は、Tailscale につなぎ直したり端末の名前を変えたりすると変わる。知らない Host が届いたときに、この間隔を空けて聞き直す。
const TAILNET_REFRESH_MS = 10_000;

const parse = (url: string): URL | undefined => (URL.canParse(url) ? new URL(url) : undefined);

/**
 * Host が自分自身を指し、Origin があれば Host と同じ相手のときだけ受ける。
 * 手元の名前はダッシュボードのポートに限る。tailnet での名前は、tailscale serve が別のポートや HTTPS で出すことがあるので、ポートを問わない。
 */
export const isAllowed = (host: string | undefined, origin: string | undefined, port: number, tailnetHosts: string[]): boolean => {
  const url = host ? parse(`http://${host}`) : undefined;
  if (!url) return false;
  const local = LOCAL_HOSTNAMES.includes(url.hostname) && url.port === String(port);
  if (!local && !tailnetHosts.includes(url.hostname)) return false;
  // Host と Origin はスキームが違ってもよいが、既定のポートの省き方がスキームごとに違うので、ヘッダーの文字のまま比べる。
  return origin === undefined || parse(origin)?.host === host?.toLowerCase();
};

let tailnet: string[] = [];
let readAt = 0;
let reading: Promise<void> | undefined;

const refreshTailnet = (): Promise<void> => {
  reading ??= platform.tailnetHosts().then((hosts) => {
    tailnet = hosts;
    readAt = Date.now();
    reading = undefined;
  });
  return reading;
};

/** リクエストを受けてよいか。知らない Host のときは、tailnet での名前を聞き直してからもう一度確かめる。 */
export const allowRequest = async (host: string | undefined, origin: string | undefined, port: number): Promise<boolean> => {
  if (isAllowed(host, origin, port, tailnet)) return true;
  if (Date.now() - readAt < TAILNET_REFRESH_MS) return false;
  await refreshTailnet();
  return isAllowed(host, origin, port, tailnet);
};
