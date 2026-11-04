// 「スレッド」のウィジェットが、登録した Slack のスレッドかチャンネルの会話を読む。セッションでは、チャットに出た Slack のリンクも読む先にする。
// Slack の API でなく、Claude Code が /mcp で接続した Slack の MCP サーバーを、その OAuth のトークンで直に呼ぶ。
// トークンの保存の形は Claude Code の内部の仕様なので、Claude Code の更新で読めなくなることがある。
import type { SlackChannel, SlackConversation, SlackMessage } from '../shared/types.ts';
import { cached } from './cache.ts';
import { platform } from './platform.ts';

const MAX_AGE_MS = 5 * 60_000;
// チャンネルの名前と ID の対応はめったに変わらないので、長く覚える。
const CHANNELS_MAX_AGE_MS = 24 * 60 * 60_000;
// チャンネルは新しい方からこの件数だけ読む。スレッドは全部読む。
const CHANNEL_LIMIT = 50;
const THREAD_LIMIT = 1000;
const TITLE_CHARS = 40;

/**
 * 読む先。thread はスレッドの親の ts で、無ければチャンネルを読む。domain は URL で指したときのワークスペースのドメイン。
 * name は # で始めて打ったチャンネルの名前で、読む前に ID を探す。
 */
export type SlackTarget = { channel: string; thread?: string; domain?: string } | { name: string };

const CHANNEL_ID = /^[CGD][A-Z0-9]{6,}$/;

/**
 * 人間が登録した参照を読む。参照は Slack の「リンクをコピー」の URL か、チャンネルの ID か、# で始めたチャンネルの名前。
 * URL は /archives/<チャンネル>/p<ts の数字> ならメッセージのスレッドを、/archives/<チャンネル> ならチャンネルを指す。スレッドの返信のリンクは thread_ts に親を持つ。
 */
export const slackTargetOf = (raw: string): SlackTarget | undefined => {
  const text = raw.trim();
  if (CHANNEL_ID.test(text)) return { channel: text };
  const name = text.match(/^#([^\s#]+)$/)?.[1];
  if (name) return { name };
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return undefined;
  }
  const m = url.pathname.match(/^\/archives\/([A-Z0-9]+)(?:\/p(\d{10})(\d{6}))?\/?$/);
  if (!m) return undefined;
  const [, channel = '', sec, micro] = m;
  const thread = url.searchParams.get('thread_ts') ?? (sec ? `${sec}.${micro}` : undefined);
  const domain = url.hostname.match(/^([\w-]+)\.slack\.com$/)?.[1];
  return { channel, ...(thread ? { thread } : {}), ...(domain ? { domain } : {}) };
};

/** 同じ会話を指す参照を 1 つにまとめる鍵。返信のリンクと親のリンクは同じスレッドになる。 */
export const slackKeyOf = (ref: string): string | undefined => {
  const t = slackTargetOf(ref);
  return t && ('name' in t ? `#${t.name}` : `${t.channel}/${t.thread ?? ''}`);
};

// 発言の中の Slack のリンク。Markdown のリンクの閉じ括弧と、日本語の文の続きは含めない。
const SLACK_URL = /https:\/\/[\w-]+\.slack\.com\/archives\/[A-Z0-9]+(?:\/p\d{16})?(?:\?[\w=&.%-]*)?/g;

/** 発言に出た Slack のスレッドとチャンネルのリンク。同じ会話を指すものは最初の 1 つにする。 */
export const slackLinksIn = (texts: string[]): string[] => {
  const byKey = new Map<string, string>();
  for (const url of texts.flatMap((t) => t.match(SLACK_URL) ?? [])) {
    const key = slackKeyOf(url);
    if (key && !byKey.has(key)) byKey.set(key, url);
  }
  return [...byKey.values()];
};

const ENTITIES: Record<string, string> = { '&lt;': '<', '&gt;': '>', '&amp;': '&', '&quot;': '"', '&#39;': "'" };
const decode = (s: string): string => s.replace(/&(?:lt|gt|amp|quot|#39);/g, (m) => ENTITIES[m] ?? m);

// チャンネルは 1 件を「=== Message from <表示名> <メール> (<ID>) at <日時> ===」の行から、スレッドは「=== THREAD PARENT MESSAGE ===」か「--- Reply n of m ---」の行から始め、発言者を From: の行に持つ。
// どちらも次に Message TS: の行があり、その後が本文になる。本文の後ろに Thread / Reactions / Files の行が付くことがある。出る順は決まっていない。
const BLOCK_START = /\n(?==== |--- )/;
const AUTHOR = /^(?:=== Message from |From: )(.+?) \(([UB][A-Z0-9]+)\)/;
// スレッドの応答は、返信が無いと親の後ろに「No thread messsages」（Slack の綴りのまま）を付ける。
const META = /^(?:Reactions: |Files: |Thread: \d+ repl|No thread mess+ages)/;

/**
 * Slack の <URL|文字> のリンクを Markdown のリンクに、<@ユーザー|名前> と <#チャンネル|名前> を @名前 と #名前 にしてから、文字の参照を戻す。
 * < と > は、本文の文字なら参照で届くので、リンクと見分けられる。
 */
const toMarkdown = (s: string): string =>
  decode(
    s
      // 箇条書きは「• 」で始まり、入れ子は 4 文字ずつ下げた「◦ 」「▪ 」になる。字下げを描かせるよう、Markdown のリストにする。
      .replace(/^( *)[•◦▪▫] /gm, (_, indent: string) => `${'  '.repeat(Math.floor(indent.length / 4))}- `)
      .replace(/<(https?:\/\/[^|>]+)(?:\|([^>]+))?>/g, (_, url: string, label?: string) => (label && label !== url ? `[${label}](${url})` : url))
      .replace(/<([@#])[A-Z0-9]+(?:\|([^>]*))?>/g, (m, mark: string, name?: string) => (name ? `${mark}${name}` : m.slice(1, -1))),
  );

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
      while (body.length && (META.test(body.at(-1) ?? '') || !body.at(-1)?.trim())) replies = Number(body.pop()?.match(/^Thread: (\d+) repl/)?.[1]) || replies;
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

/** チャンネルの検索の結果の行「#<名前> (<ID>) - ...」から、名前と ID を読む。 */
export const parseSlackChannels = (text: string): SlackChannel[] => [...text.matchAll(/#([^\s(]+) \(([CG][A-Z0-9]+)\)/g)].map(([, name = '', id = '']) => ({ name, id }));

/** 名前か説明に q を含むチャンネル。補完の候補に出す。 */
export const slackChannels = (q: string): Promise<SlackChannel[]> =>
  cached(`slack-channels-v1:${q}`, { maxAge: CHANNELS_MAX_AGE_MS }, async () =>
    parseSlackChannels(String((await callSlack('slack_search_channels', { keywords: [q], natural_language_query: '', channel_types: 'public_channel,private_channel', response_format: 'concise', limit: 20 })).results ?? '')),
  );

/** # で始めて打ったチャンネルの名前の ID。名前が完全に一致するものだけを使う。 */
export const channelIdOf = async (name: string): Promise<string> => {
  const id = (await slackChannels(name)).find((c) => c.name === name)?.id;
  if (!id) throw new Error(`チャンネルが見つかりません: #${name}`);
  return id;
};

/** チャンネルは名前を、スレッドは親の発言の先頭を見出しにする。 */
const titleOf = (text: string, messages: SlackMessage[], thread: boolean): string | undefined => {
  if (!thread) return text.match(/^Channel: (#\S+) \(/m)?.[1];
  // 絵文字の :名前: は文字のまま出るので、見出しからは外す。
  const head = messages[0]?.text.split('\n')[0]?.replace(/:[\w+-]+:/g, '').trim();
  return head && (head.length > TITLE_CHARS ? `${head.slice(0, TITLE_CHARS)}…` : head);
};

const channelNameIn = (text: string): string | undefined => text.match(/^Channel: #(\S+) \(/m)?.[1];

/** スレッドの応答はチャンネルの名前を持たないので、チャンネルの最新の 1 件を読んで名前を知る。 */
const channelNameOf = (channel: string): Promise<string | undefined> =>
  cached(`slack-channel-name-v1:${channel}`, { maxAge: CHANNELS_MAX_AGE_MS }, async () =>
    channelNameIn(String((await callSlack('slack_read_channel', { channel_id: channel, limit: 1, response_format: 'concise' })).messages ?? '')),
  );

/** MCP がつながっているワークスペースのドメイン。どのツールも ID しか返さないので、検索の結果の permalink から読む。 */
const mcpDomain = (): Promise<string | undefined> =>
  cached('slack-domain-v1', { maxAge: CHANNELS_MAX_AGE_MS }, async () => {
    const found = await callSlack('slack_search_public', { filters: 'after:2000-01-01', natural_language_query: '', limit: 1, include_context: false });
    return String(found.results ?? '').match(/https:\/\/([\w-]+)\.slack\.com\//)?.[1];
  });

/** Slack のアプリが覚えているワークスペースから、ドメインに合うワークスペースの ID を探す。slack:// のリンクは ID で指すため。 */
export const teamIdIn = (appState: string | undefined, domain: string): string | undefined => {
  try {
    const { workspaces } = JSON.parse(appState ?? '{}') as { workspaces?: Record<string, { id?: string; domain?: string }> };
    return Object.values(workspaces ?? {}).find((w) => w.domain === domain)?.id;
  } catch {
    return undefined;
  }
};

const teams = new Map<string, string>();

/** ワークスペースの ID は変わらないので、見つけたら覚える。Slack のアプリの状態ファイルは、混んだマシンでは開くだけで数十ミリ秒かかり、会話を読むたびに開くと応答を止める。 */
const teamOf = (domain: string): string | undefined => {
  const id = teams.get(domain) ?? teamIdIn(platform.slackAppState(), domain);
  if (id) teams.set(domain, id);
  return id;
};

/** 名前とドメインは見出しとリンクに添えるだけなので、読めなくても会話は出す。 */
const orUndefined = <T,>(p: Promise<T>): Promise<T | undefined> => p.catch(() => undefined);

/** 登録したスレッドかチャンネルの会話を読む。 */
export const slackConversation = async (target: SlackTarget, fresh = false): Promise<SlackConversation> => {
  const { channel, thread, domain: given } = 'name' in target ? { channel: await channelIdOf(target.name), thread: undefined, domain: undefined } : target;
  const read = cached(`slack-v5:${channel}:${thread ?? ''}`, { maxAge: MAX_AGE_MS, fresh }, async () => {
    const payload = thread
      ? await callSlack('slack_read_thread', { channel_id: channel, message_ts: thread, limit: THREAD_LIMIT, response_format: 'detailed' })
      : await callSlack('slack_read_channel', { channel_id: channel, limit: CHANNEL_LIMIT, response_format: 'detailed' });
    const text = String(payload.messages ?? '');
    const messages = parseSlackMessages(text);
    return { title: titleOf(text, messages, !!thread), channelName: channelNameIn(text), messages };
  });
  const [conv, channelName, domain] = await Promise.all([read, thread ? orUndefined(channelNameOf(channel)) : undefined, given ?? orUndefined(mcpDomain())]);
  const team = domain && teamOf(domain);
  return { ...conv, channel, thread, channelName: conv.channelName ?? channelName, domain, team };
};
