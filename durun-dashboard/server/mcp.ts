// Claude 向けの MCP サーバー（Streamable HTTP）。人間への質問、資料の表示、レビュー依頼、コードに表示するブランチ、テーマの登録、文章校正を受ける。
import fs from 'node:fs';
import type { IncomingMessage, ServerResponse } from 'node:http';
import path from 'node:path';
import { applyEvent, emptyRun, newVersion, reviseInput, titleOf } from '../shared/proofread.ts';
import type { BranchView, DiffRange, DocFormat, ProofFile, ProofVersion } from '../shared/types.ts';
import { readLiveSessions } from './claude.ts';
import { dropStaleRequests, unwait, waitFor } from './deliver.ts';
import { sessionIdForPid } from './hooks.ts';
import { resolveCommit, resolveRange } from './integrations.ts';
import { proofCommentedAt, readProofFile, writeProofFile } from './proofFiles.ts';
import { MAX_SENTENCES, commentFindings, proofReport, proofread, revise } from './proofread.ts';
import { type ProofSentence, parseProof } from './proofText.ts';
import { changed, newId, state, touch, type StoredAttention } from './store.ts';

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
- 作業のテーマや関連する JIRA 課題が分かったら set_task で登録してください。
- 人間がメッセージに文章校正アプリを添付したら、proofread で文章を校正し、書き手の事情を知っている指摘に comment_proofread でコメントを返してください。主要な指摘にコメントを返したら、文章は直さずにターンを終えてください。人間がアプリで校正した版（v2 など）へのコメントを頼まれたら、proofread に file と version を渡して、その版の指摘を受け取ってください。人間に修正版を頼まれたら、revise_proofread に file と version を渡して作ってください。`;

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
  {
    name: 'proofread',
    description:
      'ダッシュボードの文章校正アプリで、初めて読む人が文章を前から読み進められるかを文ごとに調べ、番号を振った指摘を返す。人間がメッセージに文章校正アプリを添付したときに使う。結果はアプリのファイルに保存し、人間もアプリで開ける。文の数だけ sonnet を動かすので、数分かかる。file と version を渡すと、人間がアプリで校正を終えた版の指摘を、校正し直さずに返す。',
    inputSchema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: '校正する文章のファイルパス。Markdown の見出しと箇条書きを、説明の区切りとして読む。' },
        text: { type: 'string', description: '校正する文章。path を渡すときは省略する。' },
        glossary: { type: 'string', description: '読み手が知っている語の用語集。1 行に「語：説明」か「語」を書く。読み手は、書いた語を知っているものとして読む。' },
        file: { type: 'string', description: 'アプリのファイルの id。version と合わせて渡すと、その版の指摘を返す。path と text は省略する。' },
        version: { type: 'integer', description: '版の番号（v3 なら 3）' },
      },
    },
  },
  {
    name: 'comment_proofread',
    description:
      'proofread が返した版の指摘に、書き手の事情を踏まえたコメントを返す。コメントは、文章校正アプリか revise_proofread で修正版を作るときに、その指摘をどう直すかの指示になる。主要な指摘にコメントを返したら、修正版を頼まれていなければ、文章は直さずにターンを終える。',
    inputSchema: {
      type: 'object',
      properties: {
        file: { type: 'string', description: 'proofread が返したファイルの id' },
        version: { type: 'integer', description: 'コメントを付ける版の番号（v3 なら 3）。proofread が指摘を返した版を渡す。' },
        comments: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              n: { type: 'integer', description: '指摘の番号（#3 なら 3）' },
              text: { type: 'string', description: 'コメント。修正版でどう直してほしいか、または指摘が当たらない事情。' },
              ignore: { type: 'boolean', description: 'true なら「コメントして無視」にする。その指摘は数に入れず、修正版でも直さない。' },
            },
            required: ['n', 'text'],
          },
        },
      },
      required: ['file', 'version', 'comments'],
    },
  },
  {
    name: 'revise_proofread',
    description:
      '文章校正アプリの「修正版を作る」と同じく、proofread が返した版を、無視していない指摘とコメントに沿って opus に書き直させる。書き直した文章を次の版としてアプリのファイルに足し、その文章を返す。人間に修正版を頼まれたときに使う。数分かかることがある。',
    inputSchema: {
      type: 'object',
      properties: {
        file: { type: 'string', description: 'proofread が返したファイルの id' },
        version: { type: 'integer', description: '書き直す版の番号（v3 なら 3）。校正を終えた版を渡す。' },
      },
      required: ['file', 'version'],
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

/** 結果が出るまで長くかかる呼び出しを SSE で返し、打ち切られないよう、終えるまで message() を進捗として通知し続ける。返す関数に結果を渡すと終える。 */
const openStream = (res: ServerResponse, rpcId: RpcId, params: RpcParams, message: () => string): ((result: object) => void) => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  // SSE のコメント行では打ち切りが延びないため、呼び出しに付いた progressToken で進捗通知を送る。
  const progressToken = (params._meta as { progressToken?: string | number } | undefined)?.progressToken;
  let progress = 0;
  const keepalive = setInterval(() => {
    progress += 1;
    if (progressToken === undefined) res.write(': keepalive\n\n');
    else res.write(sseMessage({ method: 'notifications/progress', params: { progressToken, progress, message: message() } }));
  }, KEEPALIVE_MS);
  res.on('close', () => clearInterval(keepalive));
  return (result) => {
    clearInterval(keepalive);
    res.end(sseMessage({ id: rpcId, result }));
  };
};

const waitForHuman = (req: IncomingMessage, res: ServerResponse, rpcId: RpcId, params: RpcParams, item: StoredAttention): void => {
  const callKey = `${String(req.headers['x-claude-pid'])}:${rpcId}`;
  pendingCalls.set(callKey, item.id);
  const finish = openStream(res, rpcId, params, () => '人間の回答を待っています');
  const waiter = (text: string): void => {
    pendingCalls.delete(callKey);
    finish(textResult(text));
  };
  waitFor(item.id, waiter);
  // 接続が切れても項目は残す。後で人間が答えたら、送信キュー経由で届ける。
  res.on('close', () => unwait(item.id, waiter));
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

/** 校正する文章。path は作業ディレクトリから読む。読めなければ、Claude に返すエラーの結果。 */
const proofSource = (sessionId: string, args: ToolArgs): { text: string; sentences: ProofSentence[] } | { error: object } => {
  const file = str(args.path) && path.resolve(state.sessions[sessionId]?.cwd ?? '', String(args.path));
  try {
    const text = file ? fs.readFileSync(file, 'utf8') : str(args.text);
    const sentences = text ? parseProof(text) : [];
    if (!text || !sentences.length) return { error: textResult('校正する文章を、path か text で渡してください。', true) };
    if (sentences.length > MAX_SENTENCES) return { error: textResult(`文が多すぎます。${MAX_SENTENCES} 文までにしてください（${sentences.length} 文あります）。`, true) };
    return { text, sentences };
  } catch (e) {
    return { error: textResult(`ファイルを読めません: ${(e as Error).message}`, true) };
  }
};

/** proofread の結果に添える、comment_proofread の呼び方。k は版の番号（0 から）。 */
const commentHint = (k: number): string => `書き手の事情を知っている指摘には、comment_proofread に file と version: ${k + 1} を渡し、番号を指してコメントを返してください。`;

/**
 * 文章校正アプリと同じ校正を走らせ、結果をアプリのファイルに保存してから、番号を振った指摘を返す。
 * 呼び出しが取り消されたら残りの文は読まず、読み終えた分を保存する。アプリの「続きから校正」で続けられる。
 */
const runProofread = async (res: ServerResponse, rpcId: RpcId, params: RpcParams, sessionId: string, args: ToolArgs): Promise<void> => {
  const source = proofSource(sessionId, args);
  if ('error' in source) return send(res, { id: rpcId, result: source.error });
  const { text, sentences } = source;
  const glossary = typeof args.glossary === 'string' ? args.glossary : '';
  let run = emptyRun(text);
  const stop = new AbortController();
  res.on('close', () => stop.abort());
  const finish = openStream(res, rpcId, params, () => `${run.readings.filter(Boolean).length} / ${sentences.length} 文を読みました`);
  let result: object;
  try {
    await proofread(text, sentences, glossary, (e) => (run = applyEvent(run, e)), stop.signal);
    const file: ProofFile = { id: newId(), title: '', glossary, versions: [{ ...newVersion(text), run }], updatedAt: Date.now() };
    const title = titleOf(file);
    writeProofFile({ ...file, title });
    const meta = state.sessions[sessionId];
    if (meta) {
      meta.proofFiles = [...(meta.proofFiles ?? []), { id: file.id, title, at: Date.now() }];
      touch();
    }
    const saved = `校正の結果を、文章校正アプリのファイル「${title}」（file: ${file.id}）に保存しました。人間は、このセッションの資料か、アプリの「ファイル」の「最近使ったファイル」から開けます。\n${commentHint(0)}`;
    result = textResult(`${saved}\n\n${proofReport(run)}`);
  } catch (e) {
    result = textResult(`校正の結果を保存できませんでした。${(e as Error).message}`, true);
  }
  if (!stop.signal.aborted) finish(result);
};

const toComment = (v: unknown): { n: number; text: string; ignore: boolean } | undefined => {
  const { n, text, ignore } = (v ?? {}) as ToolArgs;
  const body = str(text);
  return Number.isInteger(n) && body ? { n: n as number, text: body.trim(), ignore: ignore === true } : undefined;
};

/** file と version で指した、校正を終えた版。k は版の番号（0 から）。指せなければ、Claude に返すエラーの結果。 */
const proofVersion = (args: ToolArgs): { file: ProofFile; k: number; ver: ProofVersion } | { error: object } => {
  const id = str(args.file);
  // id はファイル名になるので、英数字と - と _ だけに絞る。
  const file = id && /^[\w-]{1,64}$/.test(id) ? readProofFile(id) : null;
  if (!file) return { error: textResult('file に、文章校正アプリのファイルの id を渡してください。', true) };
  // version を知らないスキーマのまま動いているセッションは文字列で渡すので、数に直して引く。
  const k = Number(args.version) - 1;
  const ver = file.versions[k];
  if (!ver) return { error: textResult(`version に版の番号を渡してください。このファイルには v${file.versions.length} までの版があります。`, true) };
  // 校正の途中の版は、続きを読むと指摘が増えて番号がずれるので、校正を終えた版だけを指せるようにする。
  if (!ver.run?.done) return { error: textResult(`v${k + 1} は校正を終えていません。人間がアプリで校正を終えてから、もう一度呼んでください。`, true) };
  return { file, k, ver };
};

/** 人間がアプリで校正を終えた版の指摘を、校正し直さずに返す。 */
const versionReport = (args: ToolArgs): object => {
  const target = proofVersion(args);
  if ('error' in target) return target.error;
  const { file, k, ver } = target;
  return textResult(`文章校正アプリのファイル「${file.title}」（file: ${file.id}）の v${k + 1} の校正の結果です。\n${commentHint(k)}\n\n${proofReport(ver.run!)}`);
};

/** 指した版の指摘に、番号で指したコメントを残す。開いているアプリには、スナップショットで書き込んだことを知らせ、コメントを取り込ませる。 */
const commentProofread = (args: ToolArgs): object => {
  const target = proofVersion(args);
  if ('error' in target) return target.error;
  const { file, k, ver } = target;
  const given = Array.isArray(args.comments) ? args.comments : [];
  const comments = given.flatMap((c) => toComment(c) ?? []);
  if (!comments.length || comments.length !== given.length) return textResult('comments の各要素に、指摘の番号 n とコメント text を渡してください。', true);
  const { ver: next, missing } = commentFindings(ver, comments);
  const notFound = missing.length ? `#${missing.join('、#')} の指摘はありません。` : '';
  if (missing.length === comments.length) return textResult(notFound, true);
  writeProofFile({ ...file, versions: file.versions.map((x, n) => (n === k ? next : x)), updatedAt: Date.now() });
  proofCommentedAt[file.id] = Date.now();
  changed();
  return textResult(`v${k + 1} にコメントを ${comments.length - missing.length} 件残しました。${notFound}主要な指摘にコメントを返し終えたら、人間に修正版を頼まれていれば revise_proofread で作ってください。頼まれていなければ、文章は直さずにターンを終えてください。人間が文章校正アプリでコメントを読み、修正版を作ります。`);
};

/**
 * アプリの「修正版を作る」と同じく、指した版を指摘とコメントに沿って書き直し、次の版としてファイルの末尾に足す。
 * 書き直している間に人間がアプリで足した版を消さないよう、足す直前にファイルを読み直す。開いているアプリには、スナップショットで知らせて取り込ませる。
 */
const reviseProofread = async (res: ServerResponse, rpcId: RpcId, params: RpcParams, args: ToolArgs): Promise<void> => {
  const target = proofVersion(args);
  if ('error' in target) return send(res, { id: rpcId, result: target.error });
  const { file, k, ver } = target;
  const run = ver.run!;
  const { fixed, notes } = reviseInput(ver, run);
  const finish = openStream(res, rpcId, params, () => `v${k + 1} の修正版を書いています`);
  try {
    const text = await revise(run.text, parseProof(run.text), file.glossary, new Set(fixed), notes);
    const disk = readProofFile(file.id) ?? file;
    const versions = [...disk.versions, newVersion(text, k, ver.fixed)];
    const title = titleOf({ ...disk, versions });
    writeProofFile({ ...disk, title, versions, updatedAt: Date.now() });
    proofCommentedAt[file.id] = Date.now();
    changed();
    finish(textResult(`v${k + 1} の指摘とコメントに沿って書き直し、文章校正アプリのファイル「${title}」（file: ${file.id}）に v${versions.length} として足しました。人間はアプリでこの版を開けます。書き直した文章は次のとおりです。\n\n${text}`));
  } catch (e) {
    finish(textResult(`修正版を作れませんでした。${(e as Error).message}`, true));
  }
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
    case 'proofread':
      return str(args.file) ? send(res, { id: rpcId, result: versionReport(args) }) : void runProofread(res, rpcId, params, sessionId, args);
    case 'comment_proofread':
      return send(res, { id: rpcId, result: commentProofread(args) });
    case 'revise_proofread':
      return void reviseProofread(res, rpcId, params, args);
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
