// durun-dashboard の HTTP サーバー。ブラウザ向けの API と SSE、Claude Code 向けのフックの受け口と MCP を同じポートで出す。
import fs from 'node:fs';
import http, { type IncomingMessage, type ServerResponse } from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { type AssistantMessage, type BranchView, type ChatEntry, type Delivery, type DiffFile, type DiffResult, type Doc, type ExplorerDir, EFFORT_LEVELS, type FileTarget, JIRA_ISSUE_KEY, MEDIA_TYPES, MODELS, PR_URL, type ImproveSettings, JIRA_MAX_RESULTS, type JiraIssue, type PrInfo, type PrNotes, type PrThread, type ReviewComment, type SkillInfo } from '../shared/types.ts';
import { withBase } from '../shared/html.ts';
import { firstHeading } from '../shared/markdown.ts';
import { WIKIPEDIA_PATH } from '../shared/wikipedia.ts';
import { type Focus, askDolphin } from './assistant.ts';
import { allowRequest } from './access.ts';
import { fillCallerPid } from './callerPid.ts';
import { compress, compressedFile, compressedStream, etagOf, notModified, pickEncoding } from './compress.ts';
import { CLAUDE_DIR, PROJECTS_DIR, SESSIONS_DIR, STATUSLINE_DIR, TASKS_DIR, readSubagents, summarizeLog } from './claude.ts';
import { answerQuestion, compactSession, isWaiting, redeliverInterrupted, redeliver, removeSession, resolveAttention, sendMessage, startSession, submitReview, switchSession } from './deliver.ts';
import { makeDiagram } from './diagram.ts';
import { docAsset, inlineAssets } from './docAssets.ts';
import { type ExportFormat, exportHtml } from './docExport.ts';
import { BING_PATH, fetchBing } from './bing.ts';
import { recordError } from './errors.ts';
import { faviconOf } from './favicon.ts';
import { UpstreamError } from './haiku.ts';
import { handleHook, type HookInput } from './hooks.ts';
import { ensureImprover, saveFeedback, updateImprove } from './improve.ts';
import { listDir } from './explorer.ts';
import { notionPage, notionTitle } from './notion.ts';
import { pageTitleOf } from './pageTitle.ts';
import { cachedPr, gitDiff, githubInbox, inRepo, jiraBase, jiraDetail, mergeClaudePrCache, postJiraComment, postPrComment, postPrReview, prDetail, prThreads, prViewed, refreshJira, repoFile, searchJira, setPrViewed, treeOf } from './integrations.ts';
import { handleMcp } from './mcp.ts';
import { platform } from './platform.ts';
import { notesFor } from './prReview.ts';
import { searchSessions } from './search.ts';
import { buildSnapshot, conversationOf, renameTheme } from './snapshot.ts';
import { DATA_DIR, onChange, save, state, touch } from './store.ts';
import { type Conversation, suggestReplies, suggestTitle, summarize, summaryText } from './suggest.ts';
import { fetchWikipedia } from './wikipedia.ts';
import { WALLPAPER_TYPES, clearWallpaper, readWallpaper, saveWallpaper } from './wallpaper.ts';

const PORT = Number(process.env.DURUN_DASHBOARD_PORT ?? 7337);
const ROOT = path.join(import.meta.dirname, '..');
const MAX_BODY_BYTES = 10 * 1024 * 1024;
// 壁紙の画像は、画面いっぱいの写真でも受けられる大きさまで。
const MAX_WALLPAPER_BYTES = 30 * 1024 * 1024;
// ファイル変更は連続して届くので、画面への配信はこの間隔にまとめる。軽量モードの画面には、通信と描き直しを減らすよう、間隔を空けて配る。
const BROADCAST_INTERVAL_MS = 400;
const LIGHT_BROADCAST_INTERVAL_MS = 3000;
// これより小さい応答は、縮めても通信量がほとんど変わらない。
const MIN_COMPRESS_BYTES = 1024;
const PR_CACHE_FILE = 'gh-pr-status-cache.json';

const STATIC: Record<string, string> = {
  '/': 'public/index.html',
  '/app.js': 'public/app.js',
  '/style.css': 'public/style.css',
  '/diagram.svg': 'public/diagram.svg',
  '/claude-new.svg': 'public/claude-new.svg',
  '/claude-open.svg': 'public/claude-open.svg',
  '/explorer.svg': 'public/explorer.svg',
  '/notepad.svg': 'public/notepad.svg',
  '/ie.svg': 'public/ie.svg',
  '/vendor/marked.js': 'node_modules/marked/lib/marked.umd.js',
  '/vendor/mermaid.js': 'node_modules/mermaid/dist/mermaid.min.js',
};
// 要るときに読み込むチャンク。名前に中身のハッシュが入る。
const CHUNK = /^\/chunks\/[\w-]+\.js$/;
const CONTENT_TYPES: Record<string, string> = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' };

class HttpError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

const json = (req: IncomingMessage, res: ServerResponse, body: unknown, status = 200): void => {
  const raw = Buffer.from(JSON.stringify(body));
  const head = { 'content-type': 'application/json; charset=utf-8', vary: 'accept-encoding' };
  const plain = () => void res.writeHead(status, head).end(raw);
  const encoding = raw.length >= MIN_COMPRESS_BYTES ? pickEncoding(req.headers['accept-encoding']) : undefined;
  if (!encoding) return plain();
  compress(raw, encoding).then((packed) => void res.writeHead(status, { ...head, 'content-encoding': encoding }).end(packed), plain);
};

const readBytes = async (req: IncomingMessage, max: number): Promise<Buffer> => {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of req) {
    size += (chunk as Buffer).length;
    if (size > max) throw new HttpError(413, 'body too large');
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks);
};

const readBody = async (req: IncomingMessage): Promise<unknown> => {
  const text = (await readBytes(req, MAX_BODY_BYTES)).toString('utf8');
  if (!text) return {};
  try {
    return JSON.parse(text);
  } catch {
    throw new HttpError(400, 'invalid JSON');
  }
};

const field = <T>(body: unknown, key: string, type: 'string' | 'boolean' | 'object'): T => {
  const v = (body as Record<string, unknown>)[key];
  if (typeof v !== type) throw new HttpError(400, `${key} must be ${type}`);
  return v as T;
};

const strings = (body: unknown, key: string): string[] => {
  const v = (body as Record<string, unknown>)[key];
  if (!Array.isArray(v) || !v.every((x) => typeof x === 'string')) throw new HttpError(400, `${key} must be string[]`);
  return v;
};

const sessionMeta = (id: string) => {
  const meta = state.sessions[id];
  if (!meta) throw new HttpError(404, `unknown session: ${id}`);
  return meta;
};

/** 開いている画面。配る間隔は画面ごとに違うので、間隔ごとにまとめて配る。 */
const clients = new Map<ServerResponse, { send: (text: string) => void; interval: number }>();
const broadcastTimers = new Map<number, NodeJS.Timeout>();

const snapshotEvent = (): string => `data: ${JSON.stringify(buildSnapshot())}\n\n`;

const broadcast = (interval: number): void => {
  broadcastTimers.delete(interval);
  const targets = [...clients.values()].filter((c) => c.interval === interval);
  if (!targets.length) return;
  const data = snapshotEvent();
  for (const c of targets) c.send(data);
};

const scheduleBroadcast = (): void => {
  for (const { interval } of clients.values()) {
    if (!broadcastTimers.has(interval)) broadcastTimers.set(interval, setTimeout(() => broadcast(interval), interval));
  }
};

const openEvents = (req: IncomingMessage, res: ServerResponse, url: URL): void => {
  const encoding = pickEncoding(req.headers['accept-encoding']);
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', ...(encoding && { 'content-encoding': encoding }) });
  const send = compressedStream(res, encoding);
  send(snapshotEvent());
  clients.set(res, { send, interval: url.searchParams.has('light') ? LIGHT_BROADCAST_INTERVAL_MS : BROADCAST_INTERVAL_MS });
  req.on('close', () => clients.delete(res));
};

/**
 * 資料に並べるリンクの favicon を返す。ブラウザにも 1 日覚えさせる。
 * 他のサイトの画像として読ませて手元のネットワークを探らせないよう、ダッシュボードの画面からの読み込みに限る。
 * 取ってきた SVG を直接開かれても中のスクリプトが動かないよう、sandbox で返す。
 */
const sendFavicon = async (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => {
  const fetchSite = req.headers['sec-fetch-site'];
  if (fetchSite && fetchSite !== 'same-origin') throw new HttpError(403, 'forbidden');
  const param = url.searchParams.get('site') ?? '';
  // ウィジェットのアイコンは、JIRA のホストを知らないので jira と書いて頼む。
  const site = URL.parse(param === 'jira' ? (jiraBase() ?? '') : param);
  if (site?.protocol !== 'https:' && site?.protocol !== 'http:') throw new HttpError(400, 'site must be an http(s) URL');
  const icon = await faviconOf(site.origin);
  if (!icon) throw new HttpError(404, `favicon がありません: ${site.origin}`);
  res
    .writeHead(200, {
      'content-type': icon.type,
      'cache-control': 'max-age=86400',
      'x-content-type-options': 'nosniff',
      'content-security-policy': "default-src 'none'; style-src 'unsafe-inline'; sandbox",
    })
    .end(icon.body);
};

/**
 * Internet Explorer のウィンドウに出す Bing の検索結果。ダッシュボードと同じオリジンから返すので、sandbox で別のオリジンの扱いにし、スクリプトも止める。
 * ダッシュボードの画面を読み書きさせないため。
 */
const sendBing = (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => sendRetro(res, fetchBing(url.search, req.headers['accept-language']));

const sendWikipedia = (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => {
  const self = `http://${req.headers.host}`;
  return sendRetro(res, fetchWikipedia(url.searchParams.get('url') ?? '', req.headers['accept-language'], self + url.pathname + url.search, self));
};

/** Internet Explorer のウィンドウに出す、サーバーが描き直したサイトのページ。スクリプトを残していても動かさない。 */
const sendRetro = async (res: ServerResponse, page: Promise<string>): Promise<void> => {
  const html = await page.catch((e: Error) => {
    throw new HttpError(502, e.message);
  });
  res
    .writeHead(200, {
      'content-type': 'text/html; charset=utf-8',
      'x-content-type-options': 'nosniff',
      'content-security-policy': "sandbox allow-forms; script-src 'none'",
    })
    .end(html);
};

/** 画面が表示しているブランチ。Claude が並べた選択肢から、人間が選んだもの。 */
const viewOf = (q: URLSearchParams): BranchView | undefined => {
  const branch = q.get('branch');
  return branch ? { branch, base: q.get('base') ?? undefined } : undefined;
};

/** ブランチの差分では、このセッションが編集したファイルに印を付けない。作業ツリーで同じパスを編集していても、ブランチの変更ではないため。 */
const diff = async (id: string, url: URL): Promise<DiffResult> => {
  const meta = sessionMeta(id);
  const log = meta.transcriptPath ? summarizeLog(meta.transcriptPath) : undefined;
  const q = url.searchParams;
  const want = { from: q.get('from') ?? undefined, to: q.get('to') ?? undefined };
  const view = viewOf(q);
  const edited = view ? new Set<string>() : (log?.editedFiles ?? new Set<string>());
  try {
    return await gitDiff(meta.cwd, cachedPr(meta.cwd, log?.branch)?.baseRef, q.get('full') === '1', edited, want, view);
  } catch (e) {
    throw new HttpError(400, `差分を取得できません: ${(e as Error).message.split('\n')[0]}`);
  }
};

/** 差分に無いファイルへのリンクで開いたときに、ファイル全体を返す。ブランチを表示しているときは、そのブランチから読む。 */
const wholeFile = async (id: string, url: URL): Promise<DiffFile> => {
  const ref = url.searchParams.get('path') ?? '';
  const branch = url.searchParams.get('branch') ?? undefined;
  const file = await repoFile(sessionMeta(id).cwd, ref, branch).catch(() => undefined);
  if (!file) throw new HttpError(404, `「${ref}」は${branch ? ` ${branch} ` : 'リポジトリ'}にありません`);
  return file;
};

// ウィンドウに浮かべて読むファイルの上限。大きなログなどで画面が固まらないようにする。
const MAX_LOCAL_FILE_BYTES = 1024 * 1024;

/** ファイルを探す起点。エクスプローラーからセッションの外で開いたファイルは id が空で、ホームディレクトリから探す。 */
const baseDir = (id: string): string => (id ? sessionMeta(id).cwd : os.homedir());

/** リポジトリの外のファイルの場所。~ はホームディレクトリにし、相対パスはセッションの作業ディレクトリから探す。行の指定は外す。 */
const localPath = (id: string, url: URL): { ref: string; full: string; stat?: fs.Stats } => {
  const ref = (url.searchParams.get('path') ?? '').replace(/:[\d-]+$/, '');
  const full = path.resolve(baseDir(id), ref.replace(/^~(?=\/|$)/, os.homedir()));
  const stat = fs.statSync(full, { throwIfNoEntry: false });
  return { ref, full, stat: stat?.isFile() ? stat : undefined };
};

const mediaType = (full: string): string | undefined => MEDIA_TYPES[path.extname(full).toLowerCase()];

/** リポジトリの外のファイルを読む。 */
const localFile = (id: string, url: URL): { path: string; content: string } => {
  const { ref, full, stat } = localPath(id, url);
  if (!stat || stat.size > MAX_LOCAL_FILE_BYTES) throw new HttpError(404, `「${ref}」を読めません`);
  return { path: full, content: fs.readFileSync(full, 'utf8') };
};

/** ウィンドウに浮かべたファイルが書き換わるたびに知らせる。保存で置き換えるエディタでも追えるよう、ファイルではなくディレクトリを見る。 */
const watchLocalFile = (req: IncomingMessage, res: ServerResponse, id: string, url: URL): void => {
  const { ref, full } = localPath(id, url);
  let watcher: fs.FSWatcher;
  try {
    watcher = fs.watch(path.dirname(full), (_, file) => file === path.basename(full) && res.write('data: change\n\n'));
  } catch {
    throw new HttpError(404, `「${ref}」を見張れません`);
  }
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  req.on('close', () => watcher.close());
};

/**
 * チャットのファイルパスの開き先。画像と動画はコードの画面で見られず、Markdown は整えた見た目で読みたいので、リポジトリにあってもウィンドウに浮かべる。
 * Markdown には、資料の題名に添える最初の見出しを付けて返す。どこでも開けなければ 404 にする。
 */
const resolveFile = async (id: string, url: URL): Promise<FileTarget> => {
  const { ref, full, stat } = localPath(id, url);
  if (stat && /\.md$/i.test(full) && stat.size <= MAX_LOCAL_FILE_BYTES) return { where: 'local', heading: firstHeading(fs.readFileSync(full, 'utf8')) };
  if (stat && mediaType(full)) return { where: 'local' };
  if (id && (await inRepo(sessionMeta(id).cwd, ref))) return { where: 'repo' };
  if (stat && stat.size <= MAX_LOCAL_FILE_BYTES) return { where: 'local' };
  throw new HttpError(404, `「${ref}」を開けません`);
};

/** メディアのファイルをそのまま返す。動画や音声の再生位置を動かせるよう、Range で求められた範囲だけを返す。 */
const sendRange = (req: IncomingMessage, res: ServerResponse, full: string, stat: fs.Stats, type: string): void => {
  const [, from = '', to = ''] = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? '') ?? [];
  const head = { 'content-type': type, 'accept-ranges': 'bytes', 'cache-control': 'no-cache', 'x-content-type-options': 'nosniff' };
  if (!from && !to) {
    res.writeHead(200, { ...head, 'content-length': stat.size });
    return void fs.createReadStream(full).pipe(res);
  }
  // bytes=-n は末尾の n バイトを指す。
  const start = from ? Number(from) : Math.max(0, stat.size - Number(to));
  const end = from && to ? Math.min(Number(to), stat.size - 1) : stat.size - 1;
  if (start > end) return void res.writeHead(416, { 'content-range': `bytes */${stat.size}` }).end();
  res.writeHead(206, { ...head, 'content-length': end - start + 1, 'content-range': `bytes ${start}-${end}/${stat.size}` });
  fs.createReadStream(full, { start, end }).pipe(res);
};

/** ウィンドウに浮かべる画像と動画を返す。 */
const sendMedia = (req: IncomingMessage, res: ServerResponse, id: string, url: URL): void => {
  // ほかのサイトの img や video から、手元のファイルを読み込ませない。
  const fetchSite = req.headers['sec-fetch-site'];
  if (fetchSite && fetchSite !== 'same-origin') throw new HttpError(403, 'forbidden');
  const { ref, full, stat } = localPath(id, url);
  const type = mediaType(full);
  if (!stat || !type) throw new HttpError(404, `「${ref}」は画像や動画ではありません`);
  sendRange(req, res, full, stat, type);
};

/**
 * HTML の資料が相対パスで埋め込んだ音声、画像、動画を返す。
 * 資料の iframe は sandbox で別オリジンの扱いになり、sec-fetch-site では画面からの読み込みと見分けられない。
 * 代わりに、present でファイルを渡した資料の、そのディレクトリの中のメディアに限る。
 */
const sendDocAsset = (req: IncomingMessage, res: ServerResponse, id: string, rel: string): void => {
  const doc = state.docs.find((d) => d.id === id);
  const asset = doc?.path && doc.format === 'html' ? docAsset(doc.path, rel) : undefined;
  const stat = asset && fs.statSync(asset.full, { throwIfNoEntry: false });
  if (!asset || !stat?.isFile()) throw new HttpError(404, `「${rel}」は資料に埋め込めるファイルではありません`);
  sendRange(req, res, asset.full, stat, asset.type);
};

/** セッションのブランチの、開いている PR。閉じた PR のレビューコメントには対応しないので、扱わない。 */
const openPr = (id: string): { cwd: string; branch: string; pr: PrInfo } => {
  const meta = sessionMeta(id);
  const branch = meta.transcriptPath ? summarizeLog(meta.transcriptPath).branch : undefined;
  const pr = cachedPr(meta.cwd, branch);
  if (!branch || pr?.state !== 'OPEN') throw new HttpError(404, `開いている PR がありません: ${id}`);
  return { cwd: meta.cwd, branch, pr };
};

/** gh や JIRA、Notion の失敗はダッシュボードの不具合ではないので、改善要望にしない。 */
const viaGh = async <T,>(what: string, call: () => Promise<T>): Promise<T> => {
  try {
    return await call();
  } catch (e) {
    throw new HttpError(502, `${what}（${(e as Error).message.split('\n')[0]}）`);
  }
};

const threadsOf = (pr: PrInfo): Promise<PrThread[]> => viaGh('レビューコメントを取得できません', () => prThreads(pr));

const setViewed = (id: string, body: unknown): Promise<object> => {
  const file = field<string>(body, 'path', 'string');
  const viewed = field<boolean>(body, 'viewed', 'boolean');
  return viaGh('Viewed を切り替えられません', () => setPrViewed(openPr(id).pr, file, viewed)).then(() => ({}));
};

/** 人間がダッシュボードで書いたコメントを、PR のレビューとして投稿する。commit は、画面が行番号を数えた PR の先端。 */
const postReview = (id: string, body: unknown): Promise<{ url: string }> => {
  const commit = field<string>(body, 'commit', 'string');
  return viaGh('PR に投稿できません', () => postPrReview(openPr(id).pr, commit, toComments(body), field<string>(body, 'summary', 'string')));
};

const prUrlOf = (url: unknown): string => {
  if (typeof url !== 'string' || !PR_URL.test(url)) throw new HttpError(400, 'PR の URL ではありません');
  return url;
};

/** 人間がダッシュボードで書いたコメントを、PR の会話に投稿する。 */
const commentPr = (body: unknown): Promise<{ url: string }> => {
  const text = field<string>(body, 'body', 'string').trim();
  if (!text) throw new HttpError(400, 'body is empty');
  const url = prUrlOf(field<string>(body, 'url', 'string'));
  return viaGh('PR にコメントできません', () => postPrComment(url, text));
};

const prNotes = async (id: string): Promise<PrNotes> => {
  const { cwd, branch, pr } = openPr(id);
  const threads = await threadsOf(pr);
  const tree = await treeOf(cwd, branch).catch((e: Error) => {
    throw new HttpError(400, `リポジトリを読めません（${e.message.split('\n')[0]}）`);
  });
  return notesFor(tree, pr, threads);
};

/** JIRA の一覧のウィジェットが、設定した JQL で課題を探す。件数は JIRA が 1 回で返せる範囲に収め、無ければ上限まで返す。 */
const jiraSearch = async (url: URL): Promise<JiraIssue[]> => {
  const jql = url.searchParams.get('jql')?.trim();
  if (!jql) throw new HttpError(400, 'jql is required');
  const max = Math.min(Math.max(Math.trunc(Number(url.searchParams.get('max'))) || JIRA_MAX_RESULTS, 1), JIRA_MAX_RESULTS);
  try {
    return await searchJira(jql, max);
  } catch (e) {
    throw new HttpError(400, (e as Error).message);
  }
};

const jiraKeyOf = (key: string): string => {
  if (!JIRA_ISSUE_KEY.test(key)) throw new HttpError(400, 'JIRA の課題のキーではありません');
  return key;
};

/** 人間がダッシュボードで書いたコメントを、課題に投稿する。 */
const commentJira = async (key: string, body: unknown): Promise<void> => {
  const text = field<string>(body, 'body', 'string').trim();
  if (!text) throw new HttpError(400, 'body is empty');
  await viaGh('課題にコメントできません', () => postJiraComment(jiraKeyOf(key), text));
};

/** 会話の全体。表示する範囲は画面が決める。 */
const chatOf = (id: string): ChatEntry[] => {
  const meta = sessionMeta(id);
  return meta.transcriptPath ? summarizeLog(meta.transcriptPath).texts : [];
};

// 一覧から探すので、URL の id がファイルの名前として使われることはない。
const subagentChatOf = (id: string, agentId: string): ChatEntry[] => {
  const meta = sessionMeta(id);
  const agent = meta.transcriptPath ? readSubagents(meta.transcriptPath).find((a) => a.id === agentId) : undefined;
  if (!agent) throw new HttpError(404, `unknown subagent: ${agentId}`);
  return agent.log.texts;
};

// 出力の末尾だけ返す。長く動く shell や Monitor の出力は数 MB になることがある。
const BG_OUTPUT_BYTES = 64 * 1024;

/** バックグラウンドの shell と Monitor の出力。セッションが終わると、Claude Code が一時ファイルごと消すことがある。 */
const bgOutputOf = (id: string, taskId: string): { output?: string } => {
  const meta = sessionMeta(id);
  const logs = meta.transcriptPath ? [summarizeLog(meta.transcriptPath), ...readSubagents(meta.transcriptPath).map((a) => a.log)] : [];
  const task = logs.map((l) => l.bgTasks.get(taskId)).find((t) => t);
  if (!task) throw new HttpError(404, `unknown task: ${taskId}`);
  if (!task.outputFile) return {};
  try {
    const size = fs.statSync(task.outputFile).size;
    const buf = Buffer.alloc(Math.min(size, BG_OUTPUT_BYTES));
    const fd = fs.openSync(task.outputFile, 'r');
    try {
      fs.readSync(fd, buf, 0, buf.length, size - buf.length);
    } finally {
      fs.closeSync(fd);
    }
    return { output: buf.toString('utf8') };
  } catch {
    return {};
  }
};

/** Claude Code がこのセッションに見せた skills。会話ログに無いセッションでは空になる。 */
const skillsOf = (id: string): SkillInfo[] => {
  const meta = sessionMeta(id);
  const skills = meta.transcriptPath ? summarizeLog(meta.transcriptPath).skills : new Map<string, string>();
  return [...skills].map(([name, description]) => ({ name, description }));
};

const readDoc = (id: string): Doc => {
  const doc = state.docs.find((d) => d.id === id);
  if (!doc) throw new HttpError(404, `unknown doc: ${id}`);
  if (!doc.path) return { ...doc, content: doc.content ?? '' };
  try {
    return { ...doc, content: fs.readFileSync(doc.path, 'utf8') };
  } catch {
    return { ...doc, format: 'text', content: `ファイルを読めません: ${doc.path}` };
  }
};

/**
 * HTML と SVG の資料を PDF か PNG にして返す。ほかの形式は画面のスクリプトで描くので、Chrome に開かせても同じ見た目にならない。
 * HTML のファイルの資料は、相対パスで埋め込んだ音声や画像を中に入れた、1 つで完結する HTML でも返す。
 */
const sendExport = async (res: ServerResponse, id: string, format: string): Promise<void> => {
  const doc = readDoc(id);
  if (doc.format !== 'html' && doc.format !== 'svg') throw new HttpError(400, `${doc.format} の資料は書き出せません`);
  if (format === 'html') {
    const whole = Buffer.from(doc.path && doc.format === 'html' ? inlineAssets(doc.content, doc.path) : doc.content);
    return void res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'content-length': whole.length }).end(whole);
  }
  // 一時ファイルにして開くので、相対パスで埋め込んだ画像は、資料のディレクトリから読ませる。
  const html = doc.path && doc.format === 'html' ? withBase(doc.content, `${pathToFileURL(path.dirname(doc.path)).href}/`) : doc.content;
  const body = await exportHtml(html, format as ExportFormat);
  res.writeHead(200, { 'content-type': format === 'pdf' ? 'application/pdf' : 'image/png', 'content-length': body.length }).end(body);
};

/** 派生の要対応は種類ごとに既読の付け方が違う。質問とレビュー依頼は、待っている Claude に取り下げを伝える。 */
const dismiss = (id: string): void => {
  const [kind, rest = ''] = id.split(/:(.*)/s);
  const doc = kind === 'doc' ? state.docs.find((d) => d.id === rest) : undefined;
  if (kind === 'reply') sessionMeta(rest).replySeenAt = Date.now();
  else if (doc) doc.seen = true;
  else if (kind === 'pr') state.dismissed.push(id);
  else if (isWaiting(id)) resolveAttention(id, '人間はこの依頼を回答せずに閉じました。', '');
  else state.attention = state.attention.filter((a) => a.id !== id);
  touch();
};

const updateOutbox = (id: string, action: 'manual' | 'cancel' | 'retry'): void => {
  const message = state.outbox.find((m) => m.id === id && !m.deliveredAt);
  if (!message) throw new HttpError(404, `no pending message: ${id}`);
  if (action === 'retry') redeliver(message.sessionId);
  else if (action === 'manual') Object.assign(message, { deliveredAt: Date.now(), via: 'manual' });
  else state.outbox = state.outbox.filter((m) => m !== message);
  touch();
};

type Handler = (m: string[], body: unknown, url: URL, req: IncomingMessage) => unknown;

const setTheme = (id: string, body: unknown): void => {
  sessionMeta(id).theme = field<string>(body, 'theme', 'string').trim() || undefined;
  touch();
};

/** アーカイブしたセッションも、一覧に出なくなったセッションも、まとめて新しい名前へ移す。 */
const setThemeName = (body: unknown): void => {
  const to = field<string>(body, 'to', 'string').trim();
  if (!to) throw new HttpError(400, 'to is required');
  renameTheme(Object.values(state.sessions), field<string>(body, 'from', 'string'), to);
  touch();
};

const setTitle = (id: string, body: unknown): void => {
  sessionMeta(id).title = field<string>(body, 'title', 'string').trim() || undefined;
  touch();
};

/**
 * アーカイブしたら、バックグラウンドのセッションを Claude Code の一覧からも消す。消したセッションには claude attach できないので、そのコマンドも出さない。
 * 消せなくてもアーカイブは保ち、消せなかった理由を返す。
 */
const setArchived = async (id: string, body: unknown): Promise<{ removeError?: string }> => {
  const meta = sessionMeta(id);
  meta.archived = field<boolean>(body, 'archived', 'boolean');
  touch();
  if (!meta.archived) return {};
  try {
    await removeSession(id);
  } catch (e) {
    return { removeError: (e as Error).message };
  }
  meta.jobId = undefined;
  touch();
  return {};
};

const setPinned = (id: string, body: unknown): void => {
  sessionMeta(id).pinned = field<boolean>(body, 'pinned', 'boolean') || undefined;
  touch();
};

/**
 * 資料に足すリンク。画面がリンクとして開くので、http と https に限る。同じ URL を足し直したら、題名と時刻を新しくし、外していたものはまた並べる。
 * 題名が空なら、すぐに並べてから、ページの <title> を読んで付ける。
 */
const addLink = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const url = URL.parse(field<string>(body, 'url', 'string').trim());
  if (url?.protocol !== 'http:' && url?.protocol !== 'https:') throw new HttpError(400, 'url must be an http(s) URL');
  const link = { url: url.href, title: field<string>(body, 'title', 'string').trim(), at: Date.now() };
  meta.links = [...(meta.links ?? []).filter((l) => l.url !== url.href), link];
  meta.hiddenDocs = meta.hiddenDocs?.filter((k) => k !== url.href);
  touch();
  if (link.title) return;
  void pageTitleOf(url).then((title) => {
    if (!title) return;
    link.title = title;
    touch();
  });
};

/** 消えたセッションの id は残さない。 */
const orderSessions = (body: unknown): void => {
  state.sessionOrder = strings(body, 'ids').filter((id) => id in state.sessions);
  touch();
};

const orderDocs = (id: string, body: unknown): void => {
  sessionMeta(id).docOrder = strings(body, 'keys');
  touch();
};

/** 題名を空にすると、付け直す前の題名に戻す。 */
const renameDoc = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const key = field<string>(body, 'key', 'string');
  const title = field<string>(body, 'title', 'string').trim();
  const titles = { ...meta.docTitles, [key]: title };
  if (!title) delete titles[key];
  meta.docTitles = titles;
  touch();
};

/** エクスプローラーから資料に足すファイル。絶対パスで受け、同じファイルを足し直したら時刻を新しくし、外していたものはまた並べる。 */
const addFile = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const file = field<string>(body, 'path', 'string');
  if (!path.isAbsolute(file) || !fs.statSync(file, { throwIfNoEntry: false })?.isFile()) throw new HttpError(400, `「${file}」はファイルではありません`);
  meta.files = [...(meta.files ?? []).filter((f) => f.path !== file), { path: file, at: Date.now() }];
  meta.hiddenDocs = meta.hiddenDocs?.filter((k) => k !== file);
  touch();
};

/** エクスプローラーで開くフォルダーの中身。読めないフォルダーはダッシュボードの不具合ではないので、理由を添えて 404 にする。 */
const explore = (url: URL): ExplorerDir => {
  const dir = url.searchParams.get('path') || undefined;
  try {
    return listDir(dir);
  } catch (e) {
    throw new HttpError(404, `「${dir}」を開けません（${(e as NodeJS.ErrnoException).code ?? (e as Error).message}）`);
  }
};

/** 外した資料は、発言にまた出ても並べない。足したリンクとファイルは、足したものも消す。 */
const hideDocs = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const keys = new Set(strings(body, 'keys'));
  meta.hiddenDocs = [...new Set([...(meta.hiddenDocs ?? []), ...keys])];
  meta.links = meta.links?.filter((l) => !keys.has(l.url));
  meta.files = meta.files?.filter((f) => !keys.has(f.path));
  meta.docOrder = meta.docOrder?.filter((k) => !keys.has(k));
  touch();
};

/** 起動したセッションは、セッションファイルとフックから一覧に現れる。 */
const newSession = (body: unknown): Promise<void> => {
  const cwd = field<string>(body, 'cwd', 'string');
  const prompt = field<string>(body, 'prompt', 'string');
  if (!prompt.trim()) throw new HttpError(400, 'prompt is required');
  if (!path.isAbsolute(cwd) || !fs.statSync(cwd, { throwIfNoEntry: false })?.isDirectory()) {
    throw new HttpError(400, `ディレクトリがありません: ${cwd}`);
  }
  return startSession(cwd, prompt, oneOf(body, 'model', MODELS), oneOf(body, 'effort', EFFORT_LEVELS));
};

const conversation = (id: string): Conversation => {
  const c = conversationOf(id);
  if (!c) throw new HttpError(404, `no conversation: ${id}`);
  return c;
};

const suggestions = (id: string): Promise<string[]> => {
  const c = conversation(id);
  if (!c.lastMessage) throw new HttpError(404, `no message to reply: ${id}`);
  return suggestReplies(c);
};

const isMessage = (m: unknown): m is AssistantMessage => {
  const { role, text } = (m ?? {}) as Record<string, unknown>;
  return (role === 'human' || role === 'dolphin') && typeof text === 'string';
};

const toMessages = (body: unknown): AssistantMessage[] => {
  const messages = field<unknown[]>(body, 'messages', 'object');
  if (!Array.isArray(messages) || !messages.every(isMessage) || messages.at(-1)?.role !== 'human') {
    throw new HttpError(400, 'messages must be [{ role, text }] ending with human');
  }
  return messages;
};

/** 開いている画面のセッションを、イルカに渡す。セッションの画面でなければ渡さない。 */
const focusOf = async (body: unknown): Promise<Focus | undefined> => {
  const id = (body as Record<string, unknown>).sessionId;
  const c = typeof id === 'string' && id ? conversationOf(id) : undefined;
  return c && { name: c.name, summary: await summaryText(c), chat: chatOf(c.id) };
};

const MAX_CLIENT_ERROR_CHARS = 4000;

/** 画面で起きた例外。改善要望の材料になるので、長すぎるものは切る。 */
const clientError = (body: unknown): void => {
  const { message, stack, url } = (body ?? {}) as Record<string, unknown>;
  if (typeof message !== 'string' || !message.trim()) throw new HttpError(400, 'message is required');
  const text = (v: unknown): string | undefined => (typeof v === 'string' ? v.slice(0, MAX_CLIENT_ERROR_CHARS) : undefined);
  recordError('client', message.slice(0, MAX_CLIENT_ERROR_CHARS), text(stack), text(url));
};

const toComments = (body: unknown): ReviewComment[] =>
  field<unknown[]>(body, 'comments', 'object').map((c) => {
    const r = c as Partial<ReviewComment>;
    if (typeof r.path !== 'string' || typeof r.body !== 'string') throw new HttpError(400, 'comment needs path and body');
    return { path: r.path, line: Number(r.line) || 0, side: r.side === 'old' ? 'old' : 'new', lineText: String(r.lineText ?? ''), body: r.body };
  });

/** branch は、人間がブランチを表示してコメントを付けたときに送られる。 */
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** 過去の依頼を書き直して、その依頼の直前まで会話を巻き戻して送る。巻き戻せないのはセッションの状態によるので 409 にする。 */
const rewind = (id: string, body: unknown): ReturnType<typeof sendMessage> => {
  const to = field<string>(body, 'rewindTo', 'string');
  if (!UUID.test(to)) throw new HttpError(400, 'rewindTo must be uuid');
  const sessionId = sessionMeta(id).id;
  try {
    return sendMessage(sessionId, field<string>(body, 'text', 'string'), to);
  } catch (e) {
    throw new HttpError(409, (e as Error).message);
  }
};

const review = (id: string, body: unknown): { delivery: Delivery } => {
  const { branch } = body as { branch?: unknown };
  return { delivery: submitReview(sessionMeta(id).id, toComments(body), field<string>(body, 'summary', 'string'), typeof branch === 'string' ? branch : undefined) };
};

/** 要望は、ダッシュボードを直す Claude が後から読めるよう 1 件 1 ファイルで残す。 */
const feedback = (body: unknown): { path: string } => {
  const text = field<string>(body, 'text', 'string').trim();
  const targets = field<unknown[]>(body, 'targets', 'object');
  if (!Array.isArray(targets)) throw new HttpError(400, 'targets must be array');
  if (!text && targets.length === 0) throw new HttpError(400, 'text or targets is required');
  return { path: saveFeedback(text, targets, field<object>(body, 'context', 'object')) };
};

// モデルと effort は claude の引数になるので、決めた値だけを通す。空文字は Claude Code の設定に従う。
const oneOf = (body: unknown, key: string, allowed: readonly string[]): string | undefined => {
  const v = (body as Record<string, unknown>)[key];
  if (v === undefined || v === '' || allowed.includes(v as string)) return v as string | undefined;
  throw new HttpError(400, `${key} must be one of: ${allowed.join(', ')}`);
};

const flag = (body: unknown, key: string): boolean | undefined => {
  const v = (body as Record<string, unknown>)[key];
  if (v !== undefined && typeof v !== 'boolean') throw new HttpError(400, `${key} must be boolean`);
  return v as boolean | undefined;
};

// 選び直さなかったほうは、画面が今のモデルの正式な名前を送る。
const MODEL_ID = /^claude-[a-z0-9-]+$/;

/** モデルと effort を変えて依頼を渡す。変えられないのはセッションの状態によるので 409 にする。 */
const switchModel = async (id: string, body: unknown): Promise<{ sessionId: string }> => {
  const text = field<string>(body, 'text', 'string');
  if (!text.trim()) throw new HttpError(400, 'text is required');
  const model = field<string>(body, 'model', 'string');
  if (model && !(MODELS as readonly string[]).includes(model) && !MODEL_ID.test(model)) throw new HttpError(400, 'model must be alias or model id');
  const sessionId = sessionMeta(id).id;
  try {
    return { sessionId: await switchSession(sessionId, text, model, oneOf(body, 'effort', EFFORT_LEVELS)) };
  } catch (e) {
    throw new HttpError(409, (e as Error).message);
  }
};

const improveSettings = (body: unknown): Partial<ImproveSettings> => {
  const patch = {
    enabled: flag(body, 'enabled'),
    model: oneOf(body, 'model', MODELS),
    effort: oneOf(body, 'effort', EFFORT_LEVELS),
  };
  // 送られなかった項目は、今の設定を undefined で消さないよう外す。
  return Object.fromEntries(Object.entries(patch).filter(([, v]) => v !== undefined));
};

const POST_ROUTES: [RegExp, Handler][] = [
  [/^\/api\/wallpaper\/url$/, (_, b) => setWallpaperUrl(b)],
  [/^\/api\/wallpaper\/reset$/, () => clearWallpaper(DATA_DIR)],
  [/^\/api\/sessions$/, (_, b) => newSession(b)],
  [/^\/api\/sessions\/order$/, (_, b) => orderSessions(b)],
  [/^\/api\/sessions\/([^/]+)\/theme$/, ([, id = ''], b) => setTheme(id, b)],
  [/^\/api\/themes\/rename$/, (_, b) => setThemeName(b)],
  [/^\/api\/sessions\/([^/]+)\/title$/, ([, id = ''], b) => setTitle(id, b)],
  [/^\/api\/sessions\/([^/]+)\/archive$/, ([, id = ''], b) => setArchived(id, b)],
  [/^\/api\/sessions\/([^/]+)\/pin$/, ([, id = ''], b) => setPinned(id, b)],
  [/^\/api\/sessions\/([^/]+)\/links$/, ([, id = ''], b) => addLink(id, b)],
  [/^\/api\/sessions\/([^/]+)\/files$/, ([, id = ''], b) => addFile(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/order$/, ([, id = ''], b) => orderDocs(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/hide$/, ([, id = ''], b) => hideDocs(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/rename$/, ([, id = ''], b) => renameDoc(id, b)],
  [/^\/api\/sessions\/([^/]+)\/message$/, ([, id = ''], b) => sendMessage(sessionMeta(id).id, field<string>(b, 'text', 'string'))],
  [/^\/api\/sessions\/([^/]+)\/rewind$/, ([, id = ''], b) => rewind(id, b)],
  [/^\/api\/sessions\/([^/]+)\/switch$/, ([, id = ''], b) => switchModel(id, b)],
  [/^\/api\/sessions\/([^/]+)\/compact$/, ([, id = '']) => compactSession(sessionMeta(id).id, sessionMeta(id).cwd)],
  [/^\/api\/sessions\/([^/]+)\/review$/, ([, id = ''], b) => review(id, b)],
  [/^\/api\/sessions\/([^/]+)\/pr-review$/, ([, id = ''], b) => postReview(id, b)],
  [/^\/api\/sessions\/([^/]+)\/pr-viewed$/, ([, id = ''], b) => setViewed(id, b)],
  // 候補や概要や PR の分析は作るたびに API を使うので、他のサイトから GET で起こされないよう POST で受ける。
  [/^\/api\/sessions\/([^/]+)\/suggestions$/, ([, id = '']) => suggestions(id)],
  [/^\/api\/sessions\/([^/]+)\/title-suggestion$/, async ([, id = '']) => ({ title: await suggestTitle(conversation(id)) })],
  [/^\/api\/sessions\/([^/]+)\/summary$/, ([, id = '']) => summarize(conversation(id))],
  [/^\/api\/sessions\/([^/]+)\/diagram$/, ([, id = ''], b) => makeDiagram(sessionMeta(id).id, sessionMeta(id).cwd, chatOf(id), field<string>(b, 'note', 'string'))],
  [/^\/api\/sessions\/([^/]+)\/pr-notes$/, ([, id = '']) => prNotes(id)],
  [/^\/api\/attention\/([^/]+)\/answer$/, ([, id = ''], b) => ({ delivery: answerQuestion(id, field<string>(b, 'answer', 'string')) })],
  [/^\/api\/attention\/([^/]+)\/dismiss$/, ([, id = '']) => dismiss(id)],
  [/^\/api\/outbox\/([^/]+)\/(manual|cancel|retry)$/, ([, id = '', action]) => updateOutbox(id, action as 'manual' | 'cancel' | 'retry')],
  [/^\/api\/pr\/comment$/, (_, b) => commentPr(b)],
  [/^\/api\/jira\/refresh$/, () => refreshJira()],
  [/^\/api\/jira\/issues\/([^/]+)\/comment$/, ([, key = ''], b) => commentJira(key, b)],
  [/^\/api\/feedback$/, (_, b) => feedback(b)],
  [/^\/api\/errors$/, (_, b) => clientError(b)],
  [/^\/api\/assistant$/, async (_, b) => askDolphin(buildSnapshot(), toMessages(b), await focusOf(b))],
  [/^\/api\/improve$/, (_, b) => updateImprove(improveSettings(b))],
  [/^\/api\/hook$/, (_, b, __, req) => handleHook(b as HookInput, Number(req.headers['x-claude-pid']) || 0)],
];

const GET_ROUTES: [RegExp, Handler][] = [
  [/^\/api\/snapshot$/, () => buildSnapshot()],
  [/^\/api\/search$/, (_, __, url) => searchSessions(url.searchParams.get('q') ?? '')],
  [/^\/api\/jira$/, (_, __, url) => jiraSearch(url)],
  [/^\/api\/github\/inbox$/, () => viaGh('GitHub の PR を探せません', githubInbox)],
  [/^\/api\/jira\/issues\/([^/]+)$/, ([, key = '']) => viaGh('課題を読めません', () => jiraDetail(jiraKeyOf(key)))],
  [/^\/api\/notion\/pages\/([0-9a-f]{32})$/, ([, id = '']) => viaGh('ページを読めません', () => notionPage(id))],
  [/^\/api\/notion\/pages\/([0-9a-f]{32})\/title$/, ([, id = '']) => viaGh('ページを読めません', async () => ({ title: await notionTitle(id) }))],
  [/^\/api\/pr$/, (_, __, url) => {
    const pr = prUrlOf(url.searchParams.get('url'));
    return viaGh('PR を読めません', () => prDetail(pr));
  }],
  [/^\/api\/memory$/, () => platform.readMemory()],
  [/^\/api\/sessions\/([^/]+)\/diff$/, ([, id = ''], _, url) => diff(id, url)],
  [/^\/api\/sessions\/([^/]+)\/file$/, ([, id = ''], _, url) => wholeFile(id, url)],
  [/^\/api\/sessions\/([^/]+)\/local-file$/, ([, id = ''], _, url) => localFile(id, url)],
  [/^\/api\/sessions\/([^/]+)\/resolve-file$/, ([, id = ''], _, url) => resolveFile(id, url)],
  // エクスプローラーからセッションの外で開いたファイルは、セッションの id を付けずに読む。
  [/^\/api\/local-file$/, (_, __, url) => localFile('', url)],
  [/^\/api\/resolve-file$/, (_, __, url) => resolveFile('', url)],
  [/^\/api\/explorer$/, (_, __, url) => explore(url)],
  [/^\/api\/sessions\/([^/]+)\/chat$/, ([, id = '']) => chatOf(id)],
  [/^\/api\/sessions\/([^/]+)\/subagents\/([^/]+)\/chat$/, ([, id = '', agentId = '']) => subagentChatOf(id, agentId)],
  [/^\/api\/sessions\/([^/]+)\/background\/([^/]+)\/output$/, ([, id = '', taskId = '']) => bgOutputOf(id, taskId)],
  [/^\/api\/sessions\/([^/]+)\/skills$/, ([, id = '']) => skillsOf(id)],
  [/^\/api\/sessions\/([^/]+)\/pr-threads$/, ([, id = '']) => threadsOf(openPr(id).pr)],
  [/^\/api\/sessions\/([^/]+)\/pr-viewed$/, ([, id = '']) => viaGh('Viewed を取得できません', () => prViewed(openPr(id).pr))],
  [/^\/api\/docs\/([^/]+)$/, ([, id = '']) => readDoc(id)],
];

const serveStatic = async (req: IncomingMessage, res: ServerResponse, pathname: string): Promise<boolean> => {
  const chunk = CHUNK.test(pathname);
  const file = STATIC[pathname] ?? (chunk ? `public${pathname}` : undefined);
  if (!file) return false;
  const full = path.join(ROOT, file);
  const stat = fs.statSync(full, { throwIfNoEntry: false });
  if (!stat) throw new HttpError(404, `${file} がありません。pnpm build を実行してください。`);
  const etag = etagOf(stat);
  const head = {
    'content-type': `${CONTENT_TYPES[path.extname(full)] ?? 'application/octet-stream'}; charset=utf-8`,
    // コードの変更で作り直したファイルは、読み込み直したときに必ず確かめさせ、変わっていなければ本体を送らない。チャンクは中身が変われば名前も変わるので、確かめさせない。
    'cache-control': chunk ? 'max-age=31536000, immutable' : 'no-cache',
    etag,
    vary: 'accept-encoding',
  };
  if (notModified(req.headers['if-none-match'], etag)) {
    res.writeHead(304, head).end();
    return true;
  }
  const encoding = pickEncoding(req.headers['accept-encoding']);
  if (encoding) res.writeHead(200, { ...head, 'content-encoding': encoding }).end(await compressedFile(full, etag, encoding));
  else fs.createReadStream(full).pipe(res.writeHead(200, { ...head, 'content-length': stat.size }));
  return true;
};

/**
 * XP スキンの壁紙。画像の権利の都合でリポジトリに含めず、メニューの「壁紙」で渡した画像だけを配信する。渡していなければ 404 になり、CSS のグラデーションで描く。
 * URL で渡した画像は、サーバーからは取りに行かず、画面をその URL へ送って読ませる。
 */
const sendWallpaper = (req: IncomingMessage, res: ServerResponse): void => {
  const wallpaper = readWallpaper(DATA_DIR);
  if (!wallpaper) throw new HttpError(404, '壁紙がありません');
  if ('url' in wallpaper) return void res.writeHead(302, { location: wallpaper.url, 'cache-control': 'no-cache' }).end();
  // 画面を開くたびに確かめさせ、変えていなければ画像を送り直さない。
  const etag = etagOf(fs.statSync(wallpaper.file));
  const head = { 'content-type': wallpaper.type, 'cache-control': 'no-cache', etag, 'x-content-type-options': 'nosniff' };
  if (notModified(req.headers['if-none-match'], etag)) return void res.writeHead(304, head).end();
  res.writeHead(200, head);
  fs.createReadStream(wallpaper.file).pipe(res);
};

/** 本文の画像をそのまま壁紙にする。種類は content-type で受ける。 */
const uploadWallpaper = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
  const type = (req.headers['content-type'] ?? '').split(';')[0]?.trim() ?? '';
  if (!WALLPAPER_TYPES[type]) throw new HttpError(400, 'JPEG、PNG、WebP、GIF の画像を渡してください');
  const bytes = await readBytes(req, MAX_WALLPAPER_BYTES);
  if (!bytes.length) throw new HttpError(400, '画像が空です');
  saveWallpaper(DATA_DIR, { type, bytes });
  res.writeHead(204).end();
};

const setWallpaperUrl = (body: unknown): void => {
  const url = URL.parse(field<string>(body, 'url', 'string').trim());
  if (url?.protocol !== 'http:' && url?.protocol !== 'https:') throw new HttpError(400, 'url must be an http(s) URL');
  saveWallpaper(DATA_DIR, { url: url.href });
};

const route = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
  const url = new URL(req.url ?? '/', `http://${req.headers.host}`);
  const routes = req.method === 'POST' ? POST_ROUTES : GET_ROUTES;
  if (url.pathname === '/mcp') {
    if (req.method !== 'POST') return void res.writeHead(405, { allow: 'POST' }).end();
    await fillCallerPid(req);
    return handleMcp(req, res, await readBody(req));
  }
  if (req.method === 'GET' && url.pathname === '/api/events') return openEvents(req, res, url);
  if (req.method === 'GET' && url.pathname === '/wallpaper') return sendWallpaper(req, res);
  // JSON ではなく画像そのものを受けるので、ほかの POST より先に本文を読む。
  if (req.method === 'POST' && url.pathname === '/api/wallpaper') return uploadWallpaper(req, res);
  if (req.method === 'GET' && url.pathname === '/api/favicon') return sendFavicon(req, res, url);
  if (req.method === 'GET' && url.pathname === BING_PATH) return sendBing(req, res, url);
  if (req.method === 'GET' && url.pathname === WIKIPEDIA_PATH) return sendWikipedia(req, res, url);
  // セッションの id が無いものは、エクスプローラーからセッションの外で開いたファイル。
  const media = req.method === 'GET' && /^\/api(?:\/sessions\/([^/]+))?\/media$/.exec(url.pathname);
  if (media) return sendMedia(req, res, decodeURIComponent(media[1] ?? ''), url);
  const fileEvents = req.method === 'GET' && /^\/api(?:\/sessions\/([^/]+))?\/local-file\/events$/.exec(url.pathname);
  if (fileEvents) return watchLocalFile(req, res, decodeURIComponent(fileEvents[1] ?? ''), url);
  const asset = req.method === 'GET' && /^\/api\/docs\/([^/]+)\/files\/(.+)$/.exec(url.pathname);
  if (asset) return sendDocAsset(req, res, decodeURIComponent(asset[1] ?? ''), decodeURIComponent(asset[2] ?? ''));
  // Chrome を起動するので、他のサイトから GET で起こされないよう POST で受ける。
  const exported = req.method === 'POST' && /^\/api\/docs\/([^/]+)\/export\/(pdf|png|html)$/.exec(url.pathname);
  if (exported) return sendExport(res, decodeURIComponent(exported[1] ?? ''), exported[2] ?? '');
  if (req.method === 'GET' && (await serveStatic(req, res, url.pathname))) return;
  for (const [pattern, handler] of routes) {
    const m = pattern.exec(url.pathname);
    if (!m) continue;
    const body = req.method === 'POST' ? await readBody(req) : undefined;
    const result = await handler(m.map((s) => decodeURIComponent(s)), body, url, req);
    if (result === undefined) return void res.writeHead(204).end();
    return json(req, res, result);
  }
  throw new HttpError(404, 'not found');
};

const server = http.createServer(async (req, res) => {
  if (!(await allowRequest(req.headers.host, req.headers.origin, PORT))) return void res.writeHead(403).end('forbidden');
  route(req, res).catch((e: unknown) => {
    // 入力の誤り（4xx）と claude の失敗（502）は、ダッシュボードの不具合ではないので集めない。
    const status = e instanceof HttpError ? e.status : e instanceof UpstreamError ? 502 : 500;
    if (status === 500) {
      console.error(e);
      recordError('server', (e as Error).message ?? String(e), (e as Error).stack, `${req.method} ${req.url}`);
    }
    if (!res.headersSent) json(req, res, { error: (e as Error).message }, status);
    else res.end();
  });
});

const watch = (target: string, recursive: boolean, onEvent: (file: string | null) => void): void => {
  try {
    fs.watch(target, { recursive }, (_, file) => onEvent(file));
  } catch {
    // まだ存在しないディレクトリは監視しない。Claude Code を一度使えば作られる。
  }
};

fs.mkdirSync(STATUSLINE_DIR, { recursive: true });
onChange(scheduleBroadcast);
for (const dir of [SESSIONS_DIR, STATUSLINE_DIR]) watch(dir, false, scheduleBroadcast);
for (const dir of [TASKS_DIR, PROJECTS_DIR]) watch(dir, true, scheduleBroadcast);
// 画面のファイルが作り直されたら、開いている画面に新しい更新時刻を配って読み込み直させる。
watch(path.join(ROOT, 'public'), false, scheduleBroadcast);
watch(CLAUDE_DIR, false, (file) => file === PR_CACHE_FILE && mergeClaudePrCache(path.join(CLAUDE_DIR, PR_CACHE_FILE)));
ensureImprover();
redeliverInterrupted();

// 落ちる前に記録して改善要望にし、これまでどおり止まる。コードが直れば node --watch が起動し直す。
for (const event of ['uncaughtException', 'unhandledRejection'] as const) {
  process.on(event, (e: unknown) => {
    console.error(e);
    recordError('server', e instanceof Error ? e.message : String(e), e instanceof Error ? e.stack : undefined, event);
    save();
    process.exit(1);
  });
}

for (const signal of ['SIGINT', 'SIGTERM'] as const) {
  process.on(signal, () => {
    save();
    process.exit(0);
  });
}

server.on('error', (e: NodeJS.ErrnoException) => {
  process.stderr.write(e.code === 'EADDRINUSE' ? `ポート ${PORT} は使用中です。DURUN_DASHBOARD_PORT で変更できます。\n` : `${e.message}\n`);
  process.exit(1);
});
server.listen(PORT, '127.0.0.1', () => process.stdout.write(`durun-dashboard: http://localhost:${PORT}\n`));
