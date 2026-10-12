// Claude 向けの MCP サーバー（Streamable HTTP）。人間への質問、資料の表示、レビュー依頼、コードに表示するブランチ、テーマの登録を受ける。
import fs from 'node:fs';
import type { IncomingMessage, ServerResponse } from 'node:http';
import path from 'node:path';
import type { BranchView, DiffRange, DocFormat } from '../shared/types.ts';
import { readLiveSessions } from './claude.ts';
import { dropStaleRequests, unwait, waitFor } from './deliver.ts';
import { sessionIdForPid } from './hooks.ts';
import { resolveCommit, resolveRange } from './integrations.ts';
import { newId, state, touch, type StoredAttention } from './store.ts';

const FALLBACK_PROTOCOL = '2025-06-18';
// Claude Code は無通信が 300 秒続いたツール呼び出しを打ち切る。人間の回答を待つ間は、それより短い間隔で進捗を通知する。
const KEEPALIVE_MS = 20_000;
const METHOD_NOT_FOUND = -32601;
const INVALID_REQUEST = -32600;

const INSTRUCTIONS = `durun-dashboard は、人間が複数の Claude Code セッションを横断して見ているダッシュボードです。
- 判断や確認が必要になったら ask_human で質問してください。回答が届くまで待ち、そのまま作業を続けられます。選択肢がある質問は options に並べてください。
- 図解や資料で説明すると伝わりやすいときは present で表示してください。ファイルに書いた資料は path で渡すと、開くたびに最新の内容が表示されます。
- まとまった実装を終えたらコミットし、request_review でコードレビューを依頼してください。レビューしてほしいコミットの範囲は from と to に渡します。返ってきた指摘は反映してください。
- レビューする PR など、作業ツリーに無いブランチを人間に見せたいときは、show_branches でコードの表示をそのブランチに切り替えてください。
- 作業のテーマや関連する JIRA 課題が分かったら set_task で登録してください。`;

const TOOLS = [
  {
    name: 'ask_human',
    description: 'ダッシュボードで人間に質問し、回答が届くまで待つ。回答は文字列で返る。',
    inputSchema: {
      type: 'object',
      properties: {
        question: { type: 'string', description: '人間への質問。背景と、何を決めてほしいかを書く。' },
        options: { type: 'array', items: { type: 'string' }, description: '選択肢。自由回答だけなら省略する。' },
      },
      required: ['question'],
    },
  },
  {
    name: 'present',
    description: '図解や資料をダッシュボードに表示して人間に見せる。Markdown 内の ```mermaid は図として描画される。',
    inputSchema: {
      type: 'object',
      properties: {
        title: { type: 'string', description: '資料の題名' },
        content: { type: 'string', description: '資料の本文。path を渡すときは省略する。' },
        path: { type: 'string', description: '資料のファイルパス。開くたびに最新の内容を表示する。HTML のファイルは、同じディレクトリより下の音声、画像、動画を相対パスで埋め込める。音声は wav ではなく、軽い ogg にして添える。audio などの要素の src に書いたファイルは、人間が HTML でダウンロードするときに HTML の中に入る。' },
        format: { type: 'string', enum: ['markdown', 'html', 'mermaid', 'svg', 'text'], description: '省略時は拡張子から判断し、分からなければ markdown。' },
      },
      required: ['title'],
    },
  },
  {
    name: 'request_review',
    description: 'このセッションの変更のコードレビューを人間に依頼し、指摘が届くまで待つ。指摘は「ファイル:行」と本文の一覧で返る。',
    inputSchema: {
      type: 'object',
      properties: {
        summary: { type: 'string', description: '何を実装したか、どこを重点的に見てほしいか。' },
        from: { type: 'string', description: 'レビューしてほしいコミットのうち、最も古いもの（例: abc1234、HEAD~2）。省略すると、分岐点から未コミットの変更までを見せる。' },
        to: { type: 'string', description: 'レビューしてほしいコミットのうち、最も新しいもの。省略すると HEAD。' },
      },
    },
  },
  {
    name: 'show_branches',
    description:
      'ダッシュボードのコードの表示の選択肢に、ブランチを並べ、先頭のものを表示する。人間は、並べたものと、このセッションの変更を切り替えて見られる。レビューする PR など、作業ツリーに無いブランチを人間に見せたいときに使う。views を空にすると、このセッションの変更だけに戻す。',
    inputSchema: {
      type: 'object',
      properties: {
        views: {
          type: 'array',
          description: '表示の選択肢。それぞれ、branch の、base との分岐点から先の差分を見せる。',
          items: {
            type: 'object',
            properties: {
              branch: { type: 'string', description: '表示するブランチ（例: origin/feature-x）。リモートのブランチは、先に git fetch しておく。' },
              base: { type: 'string', description: '比べる相手（例: PR のベースの origin/main）。省くと origin の既定のブランチと比べる。' },
              label: { type: 'string', description: '選択肢に出す名前（例: PR #123）。省くと branch を出す。' },
            },
            required: ['branch'],
          },
        },
      },
      required: ['views'],
    },
  },
  {
    name: 'set_task',
    description: 'このセッションのテーマ、作業名、関連する JIRA 課題をダッシュボードに登録する。ダッシュボードはテーマごとにセッションをまとめる。',
    inputSchema: {
      type: 'object',
      properties: {
        theme: { type: 'string', description: '作業のテーマ' },
        title: { type: 'string', description: 'このセッションの作業名' },
        jira: { type: 'array', items: { type: 'string' }, description: '関連する JIRA の課題キー（例: PROJ-1102）' },
      },
    },
  },
];

type RpcId = string | number;
type RpcParams = Record<string, unknown>;
type RpcMessage = { jsonrpc?: string; id?: RpcId; method?: string; params?: RpcParams };
type ToolArgs = Record<string, unknown>;

const send = (res: ServerResponse, body: object): void => {
  res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ jsonrpc: '2.0', ...body }));
};

const textResult = (text: string, isError = false): object => ({ content: [{ type: 'text', text }], ...(isError ? { isError } : {}) });

const str = (v: unknown): string | undefined => (typeof v === 'string' && v.trim() ? v : undefined);
const strList = (v: unknown): string[] | undefined => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : undefined);

/** headersHelper が付けた呼び出し元 claude プロセスの PID から、今のセッションを引く。 */
const sessionIdOf = (req: IncomingMessage): string | undefined => {
  const pid = Number(req.headers['x-claude-pid']);
  for (const live of readLiveSessions().values()) if (live.pid === pid) return live.sessionId;
  return sessionIdForPid(pid);
};

/**
 * 同じ質問の再呼び出し（接続が切れた後の再試行など）は、既存の項目に待ち手を付け替える。
 * 新しく尋ねたときは、待ち手のいない前の質問を片付ける。Claude は先へ進んでおり、文面を変えて尋ね直した質問が二重に残る。
 */
const upsertRequest = (sessionId: string, kind: 'ask' | 'review', body: string | undefined, extra: Pick<StoredAttention, 'options' | 'range'> = {}): StoredAttention => {
  const existing = state.attention.find((a) => a.sessionId === sessionId && a.kind === kind && a.body === body);
  if (existing) return existing;
  dropStaleRequests(sessionId);
  const item: StoredAttention = { id: newId(), kind, sessionId, title: kind === 'ask' ? '質問' : 'レビュー依頼', body, ...extra, createdAt: Date.now() };
  state.attention.push(item);
  touch();
  return item;
};

// MCP のキャンセル通知は JSON-RPC の id で届くので、どの待ちかを引けるようにしておく。
const pendingCalls = new Map<string, string>();

const sseMessage = (body: object): string => `event: message\ndata: ${JSON.stringify({ jsonrpc: '2.0', ...body })}\n\n`;

const waitForHuman = (req: IncomingMessage, res: ServerResponse, rpcId: RpcId, params: RpcParams, item: StoredAttention): void => {
  const callKey = `${String(req.headers['x-claude-pid'])}:${rpcId}`;
  pendingCalls.set(callKey, item.id);
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  // SSE のコメント行では打ち切りが延びないため、呼び出しに付いた progressToken で進捗通知を送る。
  const progressToken = (params._meta as { progressToken?: string | number } | undefined)?.progressToken;
  let progress = 0;
  const keepalive = setInterval(() => {
    progress += 1;
    if (progressToken === undefined) res.write(': keepalive\n\n');
    else res.write(sseMessage({ method: 'notifications/progress', params: { progressToken, progress, message: '人間の回答を待っています' } }));
  }, KEEPALIVE_MS);
  const waiter = (text: string): void => {
    clearInterval(keepalive);
    pendingCalls.delete(callKey);
    res.end(sseMessage({ id: rpcId, result: textResult(text) }));
  };
  waitFor(item.id, waiter);
  // 接続が切れても項目は残す。後で人間が答えたら、送信キュー経由で届ける。
  res.on('close', () => {
    clearInterval(keepalive);
    unwait(item.id, waiter);
  });
};

const inferFormat = (file: string | undefined): DocFormat => {
  const ext = file ? path.extname(file).toLowerCase() : '';
  if (ext === '.html' || ext === '.htm') return 'html';
  if (ext === '.mmd' || ext === '.mermaid') return 'mermaid';
  if (ext === '.svg') return 'svg';
  if (ext === '.md' || ext === '.markdown' || !ext) return 'markdown';
  return 'text';
};

const present = (req: IncomingMessage, sessionId: string, args: ToolArgs): object => {
  const title = str(args.title);
  const content = str(args.content);
  const cwd = state.sessions[sessionId]?.cwd ?? '';
  const file = str(args.path) && path.resolve(cwd, String(args.path));
  if (!title || (!content && !file)) return textResult('title と、content か path のどちらかを渡してください。', true);
  if (file && !fs.existsSync(file)) return textResult(`ファイルが見つかりません: ${file}`, true);
  const format = (str(args.format) as DocFormat | undefined) ?? inferFormat(file);
  const id = newId();
  state.docs.push({ id, sessionId, title, format, path: file, content: file ? undefined : content, createdAt: Date.now(), seen: false });
  touch();
  return textResult(`資料「${title}」をダッシュボードに表示しました: http://${req.headers.host}/#/doc/${id}`);
};

const setTask = (sessionId: string, args: ToolArgs): object => {
  const meta = state.sessions[sessionId];
  if (!meta) return textResult('このセッションはまだダッシュボードに登録されていません。', true);
  const jira = strList(args.jira)?.map((k) => k.toUpperCase());
  Object.assign(meta, { theme: str(args.theme) ?? meta.theme, title: str(args.title) ?? meta.title, jira: jira ?? meta.jira });
  touch();
  return textResult('登録しました。');
};

const toView = (v: unknown): BranchView | undefined => {
  const { branch, base, label } = (v ?? {}) as ToolArgs;
  const name = str(branch);
  return name ? { branch: name, base: str(base), label: str(label) } : undefined;
};

/** 見つからないブランチは、画面に出す前に Claude に知らせる。 */
const showBranches = async (req: IncomingMessage, sessionId: string, args: ToolArgs): Promise<object> => {
  const meta = state.sessions[sessionId];
  if (!meta) return textResult('このセッションはまだダッシュボードに登録されていません。', true);
  const given = Array.isArray(args.views) ? args.views : [];
  const views = given.flatMap((v) => toView(v) ?? []);
  if (views.length !== given.length) return textResult('views の各要素に branch を渡してください。', true);
  try {
    for (const v of views) for (const ref of [v.branch, v.base]) if (ref) await resolveCommit(meta.cwd, ref);
  } catch (e) {
    return textResult(`${(e as Error).message}。リモートのブランチは、先に git fetch してください。`, true);
  }
  meta.branchViews = views.length ? views : undefined;
  touch();
  const url = `http://${req.headers.host}/#/s/${sessionId}/code`;
  const [first] = views;
  return textResult(first ? `コードの表示を ${first.label ?? first.branch} に切り替えました: ${url}` : `コードの表示を、このセッションの変更だけに戻しました: ${url}`);
};

/** from を省いたときは範囲を決めず、画面の既定（分岐点から未コミットの変更まで）に任せる。 */
const reviewRange = async (sessionId: string, args: ToolArgs): Promise<DiffRange | undefined> => {
  const from = str(args.from);
  const cwd = state.sessions[sessionId]?.cwd;
  if (!from || !cwd) return undefined;
  return resolveRange(cwd, from, str(args.to) ?? 'HEAD');
};

const callTool = (req: IncomingMessage, res: ServerResponse, rpcId: RpcId, params: RpcParams = {}): void => {
  const sessionId = sessionIdOf(req);
  const args = (params.arguments ?? {}) as ToolArgs;
  if (!sessionId) return send(res, { id: rpcId, result: textResult('呼び出し元のセッションを特定できません。durun-dashboard の setup を確認してください。', true) });
  switch (params.name) {
    case 'ask_human': {
      const question = str(args.question);
      if (!question) return send(res, { id: rpcId, result: textResult('question を渡してください。', true) });
      return waitForHuman(req, res, rpcId, params, upsertRequest(sessionId, 'ask', question, { options: strList(args.options) }));
    }
    case 'request_review':
      return void reviewRange(sessionId, args).then(
        (range) => waitForHuman(req, res, rpcId, params, upsertRequest(sessionId, 'review', str(args.summary), { range })),
        (e: Error) => send(res, { id: rpcId, result: textResult(`レビューの範囲を決められません。${e.message}`, true) }),
      );
    case 'present':
      return send(res, { id: rpcId, result: present(req, sessionId, args) });
    case 'show_branches':
      return void showBranches(req, sessionId, args).then((result) => send(res, { id: rpcId, result }));
    case 'set_task':
      return send(res, { id: rpcId, result: setTask(sessionId, args) });
    default:
      return send(res, { id: rpcId, error: { code: METHOD_NOT_FOUND, message: `Unknown tool: ${String(params.name)}` } });
  }
};

/** Esc などで Claude 側が呼び出しを取り消したら、質問も取り下げる。接続の切断と通知の順序は前後しうる。 */
const onCancelled = (req: IncomingMessage, params: RpcParams | undefined): void => {
  const callKey = `${String(req.headers['x-claude-pid'])}:${String(params?.requestId)}`;
  const attentionId = pendingCalls.get(callKey);
  if (!attentionId) return;
  pendingCalls.delete(callKey);
  state.attention = state.attention.filter((a) => a.id !== attentionId);
  touch();
};

export const handleMcp = (req: IncomingMessage, res: ServerResponse, body: unknown): void => {
  // 2025-06-18 以降の仕様は JSON-RPC のバッチを使わない。
  if (!body || typeof body !== 'object' || Array.isArray(body)) {
    return send(res, { id: null, error: { code: INVALID_REQUEST, message: 'Invalid request' } });
  }
  const msg = body as RpcMessage;
  if (msg.id === undefined) {
    if (msg.method === 'notifications/cancelled') onCancelled(req, msg.params);
    res.writeHead(202).end();
    return;
  }
  if (msg.method === 'initialize') {
    const protocolVersion = str(msg.params?.protocolVersion) ?? FALLBACK_PROTOCOL;
    return send(res, {
      id: msg.id,
      result: { protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'durun-dashboard', version: '0.1.0' }, instructions: INSTRUCTIONS },
    });
  }
  if (msg.method === 'ping') return send(res, { id: msg.id, result: {} });
  if (msg.method === 'tools/list') return send(res, { id: msg.id, result: { tools: TOOLS } });
  if (msg.method === 'tools/call') return callTool(req, res, msg.id, msg.params);
  // server/discover など未対応のメソッドは、クライアントが initialize へ切り替えられるよう標準のエラーを返す。
  send(res, { id: msg.id, error: { code: METHOD_NOT_FOUND, message: `Method not found: ${String(msg.method)}` } });
};
