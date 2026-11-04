// 「スレッド」のウィジェットが、登録した Slack のスレッドかチャンネルの会話を読む。
// Slack の API でなく、Claude Code が /mcp で接続した Slack の MCP サーバーを、その OAuth のトークンで直に呼ぶ。
// トークンの保存の形は Claude Code の内部の仕様なので、Claude Code の更新で読めなくなることがある。
import type { SlackConversation, SlackMessage } from '../shared/types.ts';
import { cached } from './cache.ts';
import { platform } from './platform.ts';

const MAX_AGE_MS = 5 * 60_000;
// チャンネルは新しい方からこの件数だけ読む。スレッドは全部読む。
const CHANNEL_LIMIT = 50;
const THREAD_LIMIT = 1000;

/** URL から読む先。thread はスレッドの親の ts。無ければチャンネルを読む。 */
export type SlackTarget = { channel: string; thread?: string };

/**
 * Slack の「リンクをコピー」の URL を読む。スレッドの返信のリンクは thread_ts に親を持つ。
 * /archives/<チャンネル>/p<ts の数字> ならメッセージのスレッド、/archives/<チャンネル> ならチャンネルを指す。
 */
export const slackTargetOf = (raw: string): SlackTarget | undefined => {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return undefined;
  }
  const m = url.pathname.match(/^\/archives\/([A-Z0-9]+)(?:\/p(\d{10})(\d{6}))?\/?$/);
  if (!m) return undefined;
  const [, channel = '', sec, micro] = m;
  const thread = url.searchParams.get('thread_ts') ?? (sec ? `${sec}.${micro}` : undefined);
  return thread ? { channel, thread } : { channel };
};

const ENTITIES: Record<string, string> = { '&lt;': '<', '&gt;': '>', '&amp;': '&', '&quot;': '"', '&#39;': "'" };
const decode = (s: string): string => s.replace(/&(?:lt|gt|amp|quot|#39);/g, (m) => ENTITIES[m] ?? m);

// チャンネルは 1 件を「=== Message from <表示名> <メール> (<ID>) at <日時> ===」の行から、スレッドは「=== THREAD PARENT MESSAGE ===」か「--- Reply n of m ---」の行から始め、発言者を From: の行に持つ。
// どちらも次に Message TS: の行があり、その後が本文になる。本文の後ろに Thread / Reactions / Files の行が付くことがある。出る順は決まっていない。
const BLOCK_START = /\n(?==== |--- )/;
const AUTHOR = /^(?:=== Message from |From: )(.+?) \(([UB][A-Z0-9]+)\)/;
const META = /^(?:Reactions: |Files: |Thread: \d+ repl)/;

/** Slack の <URL|文字> のリンクを Markdown のリンクにしてから、文字の参照を戻す。< と > は、本文の文字なら参照で届くので、リンクと見分けられる。 */
const toMarkdown = (s: string): string =>
  decode(s.replace(/<(https?:\/\/[^|>]+)(?:\|([^>]+))?>/g, (_, url: string, label?: string) => (label && label !== url ? `[${label}](${url})` : url)));

/** MCP が返すチャンネルかスレッドの messages を、古い順のメッセージにする。 */
export const parseSlackMessages = (text: string): SlackMessage[] =>
  text
    .split(BLOCK_START)
    .flatMap((block) => {
      const lines = block.trimEnd().split('\n');
      const tsAt = lines.findIndex((l) => /^Message TS: \d+\.\d+$/.test(l));
      const author = lines.slice(0, Math.max(tsAt, 0)).map((l) => l.trimEnd().match(AUTHOR)).find(Boolean);
      if (tsAt < 0 || !author) return [];
      const body = lines.slice(tsAt + 1);
      let replies: number | undefined;
      while (body.length && META.test(body.at(-1) ?? '')) replies = Number(body.pop()?.match(/^Thread: (\d+) repl/)?.[1]) || replies;
      const user = decode(author[1] ?? '').replace(/\s*<[^>]*>$/, '');
      return [{ ts: lines[tsAt]?.slice('Message TS: '.length) ?? '', user, text: toMarkdown(body.join('\n')).trim(), ...(replies ? { replies } : {}) }];
    })
    .sort((a, b) => Number(a.ts) - Number(b.ts));

type Credential = { serverUrl: string; accessToken: string; expiresAt?: number };

const credential = async (): Promise<Credential> => {
  const raw = await platform.claudeCredentials();
  if (!raw) throw new Error('Claude Code の認証情報を読めません');
  // キーは「<サーバー名>|<ハッシュ>」の形。サーバー名は環境で揺れるので、slack を含むものを使う。
  const entries = Object.entries((JSON.parse(raw) as { mcpOAuth?: Record<string, Credential> }).mcpOAuth ?? {});
  const entry = entries.find(([key]) => key.split('|')[0]?.toLowerCase().includes('slack'))?.[1];
  if (!entry?.accessToken) throw new Error('Slack の MCP に接続していません。Claude Code の /mcp から Slack に接続してください');
  if (entry.expiresAt && entry.expiresAt < Date.now()) throw new Error('Slack の MCP のトークンが期限切れです。Claude Code の /mcp から接続し直してください');
  return entry;
};

/** Streamable HTTP の応答は JSON か SSE で届く。SSE なら data 行から結果のメッセージを探す。 */
const rpcResult = (type: string, body: string): { result?: unknown; error?: unknown } | undefined => {
  if (!type.includes('text/event-stream')) return JSON.parse(body);
  return body
    .split('\n')
    .filter((l) => l.startsWith('data:'))
    .map((l) => JSON.parse(l.slice(5).trim()) as { result?: unknown; error?: unknown })
    .find((m) => 'result' in m || 'error' in m);
};

/** MCP のセッションを開き、ツールを 1 回呼んで、返った文字を JSON として読む。 */
const callSlack = async (tool: string, args: Record<string, unknown>): Promise<Record<string, unknown>> => {
  const { serverUrl, accessToken } = await credential();
  let session: string | null = null;
  const send = async (payload: object): Promise<unknown> => {
    const res = await fetch(serverUrl, {
      method: 'POST',
      headers: { authorization: `Bearer ${accessToken}`, 'content-type': 'application/json', accept: 'application/json, text/event-stream', ...(session ? { 'mcp-session-id': session } : {}) },
      body: JSON.stringify(payload),
      signal: AbortSignal.timeout(30_000),
    });
    session ??= res.headers.get('mcp-session-id');
    if (!res.ok) throw new Error(`Slack の MCP が ${res.status} を返しました`);
    if (!('id' in payload)) return undefined;
    const msg = rpcResult(res.headers.get('content-type') ?? '', await res.text());
    if (msg?.error) throw new Error(`Slack の MCP がエラーを返しました: ${JSON.stringify(msg.error)}`);
    return msg?.result;
  };
  await send({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'durun-dashboard', version: '1' } } });
  await send({ jsonrpc: '2.0', method: 'notifications/initialized' });
  const result = (await send({ jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: tool, arguments: args } })) as { isError?: boolean; content?: { type: string; text?: string }[] };
  const text = (result?.content ?? []).filter((c) => c.type === 'text').map((c) => c.text ?? '').join('\n');
  // ツールの失敗は JSON-RPC のエラーでなく isError で返る。
  if (result?.isError) throw new Error(`Slack を読めません: ${text.slice(0, 200)}`);
  try {
    return JSON.parse(text) as Record<string, unknown>;
  } catch {
    throw new Error(`Slack の応答を読めません: ${text.slice(0, 200)}`);
  }
};

/** 登録したスレッドかチャンネルの会話を読む。 */
export const slackConversation = (target: SlackTarget, fresh = false): Promise<SlackConversation> =>
  cached(`slack-v1:${target.channel}:${target.thread ?? ''}`, { maxAge: MAX_AGE_MS, fresh }, async () => {
    const payload = target.thread
      ? await callSlack('slack_read_thread', { channel_id: target.channel, message_ts: target.thread, limit: THREAD_LIMIT, response_format: 'detailed' })
      : await callSlack('slack_read_channel', { channel_id: target.channel, limit: CHANNEL_LIMIT, response_format: 'detailed' });
    return { messages: parseSlackMessages(String(payload.messages ?? '')) };
  });
