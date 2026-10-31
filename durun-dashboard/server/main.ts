// durun-dashboard の HTTP サーバー。ブラウザ向けの API と SSE、Claude Code 向けのフックの受け口と MCP を同じポートで出す。
import fs from 'node:fs';
import http, { type IncomingMessage, type ServerResponse } from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { pathToFileURL } from 'node:url';
import { type AssistantMessage, type BranchView, type ChatEntry, bookmarkKey, type Delivery, type DiffFile, type DiffResult, type Doc, type DocEditResult, type ExplorerDir, EFFORT_LEVELS, type FileTarget, GITHUB_ATTACHMENT, type Glossary, JIRA_ISSUE_KEY, MEDIA_TYPES, MODELS, PR_URL, type ImproveSettings, JIRA_LIST_MAX, type JiraIssue, type PrInfo, type PrNotes, type PrThread, type ProofEvent, type ProofFile, type ProofMode, type ProofVersion, type QueuedMessage, type Replies, type ReviewComment, type ReviseNote, type SkillInfo, type TextSaveResult } from '../shared/types.ts';
import { withBase } from '../shared/html.ts';
import { firstHeading } from '../shared/markdown.ts';
import { GOOGLE_HOME_PATH, GOOGLE_SEARCH_PATH, RETRO_PATH, WIKIPEDIA_PATH, YAHOO_HOME_PATH, YAHOO_SEARCH_PATH, siteOfFrame } from '../shared/retro.ts';
import { type Focus, askDolphin } from './assistant.ts';
import { allowRequest } from './access.ts';
import { answerAuth, loginClaude } from './auth.ts';
import { fillCallerPid } from './callerPid.ts';
import { compress, compressedFile, compressedStream, etagOf, notModified, pickEncoding } from './compress.ts';
import { bump, saveCache } from './cache.ts';
import { CLAUDE_DIR, borrowSkills, PROJECTS_DIR, SESSIONS_DIR, STATUSLINE_DIR, TASKS_DIR, readLiveSessions, readSubagents, saveLogSummaries, summarizeLog } from './claude.ts';
import { allowPermission, answerQuestion, archiveSession, compactSession, flushArchives, flushQueue, forkSession, isWaiting, redeliverInterrupted, redeliver, resolveAttention, retryRemovals, sendMessage, sendQueued, SessionStateError, startSession, stopSession, submitReview, switchSession } from './deliver.ts';
import { makeDiagram } from './diagram.ts';
import { docAsset, inlineAssets } from './docAssets.ts';
import { type ExportFormat, exportHtml } from './docExport.ts';
import { OLD_GOOGLE_HOME, fetchGoogle, fetchLucky } from './google.ts';
import { recordError, recordSlow } from './errors.ts';
import { faviconOf } from './favicon.ts';
import { UpstreamError } from './haiku.ts';
import { handleHook, holdPermission, type HookInput } from './hooks.ts';
import { ensureImprover, saveFeedback, updateImprove } from './improve.ts';
import { fileCandidates, listDir, saveText } from './explorer.ts';
import { notionPage, notionTitle } from './notion.ts';
import { findTitle, pageTitleOf } from './pageTitle.ts';
import { badgeOf, badgePr, cachedPr, gitDiff, githubAttachment, githubInbox, inRepo, jiraBase, jiraDetail, jiraList, jiraRefs, loadBadge, mergeClaudePrCache, postJiraComment, postPrComment, postPrReview, prDetail, prThreads, prViewed, refreshJira, repoFile, repoRefs, setPrViewed, transitionJira, treeOf } from './integrations.ts';
import { answerOffline, checkLimits, onTranscriptChange, setAutoContinue } from './limitResume.ts';
import { handleMcp } from './mcp.ts';
import { latestMemory } from './memory.ts';
import { platform } from './platform.ts';
import { notesFor } from './prReview.ts';
import { recentProofFiles, readProofFile, writeProofFile } from './proofFiles.ts';
import { MAX_SENTENCES, priorReadings, proofread, revise, summarizeProof } from './proofread.ts';
import { parseProof } from './proofText.ts';
import { recordRateLimits } from './rateLimits.ts';
import { searchSessions, warmSearch } from './search.ts';
import { buildSnapshot, conversationOf, renameTheme, warmLogs } from './snapshot.ts';
import { DATA_DIR, newId, onChange, save, state, touch } from './store.ts';
import { type Conversation, explainTerm, peekReplies, sessionGlossary, suggestTitle, summarize, summaryText, termKey } from './suggest.ts';
import { hasJev } from './jev.ts';
import { MINES_ROW, minesHint } from './minesweeper.ts';
import { fetchRetro } from './retro.ts';
import { fetchWikipedia } from './wikipedia.ts';
import { fetchYahooHome, fetchYahooSearch } from './yahoo.ts';
import { resizeTerminal, streamTerminal, writeTerminal } from './terminal.ts';
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
// アーカイブしたのに Claude Code の一覧から消せなかったセッションを、消し直す間隔。
// 消せない理由の多くは作業ツリーに残る未 push の変更で、人間が push したことはダッシュボードから見えないので、間を置いて試す。
const REMOVE_RETRY_MS = 10 * 60_000;

const STATIC: Record<string, string> = {
  '/': 'public/index.html',
  '/app.js': 'public/app.js',
  '/style.css': 'public/style.css',
  '/diagram.svg': 'public/diagram.svg',
  '/claude-new.svg': 'public/claude-new.svg',
  '/claude-open.svg': 'public/claude-open.svg',
  '/explorer.svg': 'public/explorer.svg',
  '/notepad.svg': 'public/notepad.svg',
  '/bookmark.svg': 'public/bookmark.svg',
  '/proofread.svg': 'public/proofread.svg',
  '/terminal.svg': 'public/terminal.svg',
  '/ie.svg': 'public/ie.svg',
  '/ie-toolbar.svg': 'public/ie-toolbar.svg',
  '/vendor/marked.js': 'node_modules/marked/lib/marked.umd.js',
  '/vendor/mermaid.js': 'node_modules/mermaid/dist/mermaid.min.js',
  '/vendor/xterm.css': 'node_modules/@xterm/xterm/css/xterm.css',
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

const field = <T>(body: unknown, key: string, type: 'string' | 'number' | 'boolean' | 'object'): T => {
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

// 起動した直後に、会話ログの要約を裏で作る。作り終えるまでは、スナップショットを組み立てずに待たせ、画面にも配り始めない。
const logsReady = warmLogs();
// 検索だけが読む会話ログは、スナップショットを待たせないよう、一覧に出る分を読み終えてから読む。
void logsReady.then(warmSearch);

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
  let closed = false;
  req.on('close', () => {
    closed = true;
    clients.delete(res);
  });
  void logsReady.then(() => {
    if (closed) return;
    send(snapshotEvent());
    clients.set(res, { send, interval: url.searchParams.has('light') ? LIGHT_BROADCAST_INTERVAL_MS : BROADCAST_INTERVAL_MS });
  });
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

// 添付は中身が変わらないので、ブラウザに長く覚えさせる。
const ATTACHMENT_MAX_AGE_S = 30 * 24 * 60 * 60;

// GitHub の応答から、そのまま画面へ渡すヘッダー。範囲のヘッダーは、動画の再生位置を動かすのに要る。
const ATTACHMENT_HEADERS = ['content-type', 'content-length', 'content-range', 'accept-ranges'];

/**
 * PR に添付した画像や動画を、gh のログインで読んで返す。ほかのサイトの img から非公開の添付を読ませないよう、ダッシュボードの画面からの読み込みに限る。
 * cache.json には覚えず、ブラウザに覚えさせる。cache.json は保存のたびに全体を書き直すので、画像や動画を入れると重くなるため。
 * 取ってきた SVG を直接開かれても中のスクリプトが動かないよう、sandbox で返す。
 */
const sendGithubAttachment = async (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => {
  const fetchSite = req.headers['sec-fetch-site'];
  if (fetchSite && fetchSite !== 'same-origin') throw new HttpError(403, 'forbidden');
  const src = url.searchParams.get('url') ?? '';
  if (!GITHUB_ATTACHMENT.test(src)) throw new HttpError(400, 'url must be a GitHub attachment');
  const range = req.headers.range;
  const file = await viaGh('添付を読めません', () => githubAttachment(src, range && /^bytes=\d*-\d*$/.test(range) ? range : undefined));
  res.on('close', file.stop);
  const type = file.headers.get('content-type') ?? '';
  if (file.status !== 200 && file.status !== 206) throw new HttpError(502, `添付を読めません（GitHub が ${file.status} を返しました）`);
  if (!/^(image|video)\//.test(type)) throw new HttpError(415, `画像や動画ではありません: ${type}`);
  res.writeHead(file.status, {
    ...Object.fromEntries(ATTACHMENT_HEADERS.flatMap((h) => (file.headers.has(h) ? [[h, file.headers.get(h)]] : []))),
    'cache-control': `private, max-age=${ATTACHMENT_MAX_AGE_S}`,
    'x-content-type-options': 'nosniff',
    'content-security-policy': "default-src 'none'; style-src 'unsafe-inline'; sandbox",
  });
  file.body.pipe(res);
};

/** I'm Feeling Lucky は検索結果の 1 件目へ移す。結果が無ければ、Google と同じく結果のページを出す。 */
const sendGoogle = async (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => {
  const lang = req.headers['accept-language'];
  if (url.searchParams.has('btnI')) {
    const to = await fetchLucky(url.search, lang).catch((e: Error) => {
      throw new HttpError(502, e.message);
    });
    if (to) return void res.writeHead(302, { location: to }).end();
  }
  return sendRetro(res, fetchGoogle(url.search, lang), url);
};

const sendWikipedia = (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => {
  const self = `http://${req.headers.host}`;
  return sendRetro(res, fetchWikipedia(url.searchParams.get('url') ?? '', req.headers['accept-language'], self + url.pathname + url.search, self), url);
};

const sendRetroSite = (req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> => sendRetro(res, fetchRetro(url.searchParams.get('url') ?? '', req.headers['accept-language']), url);

/**
 * 最後に描き直したページ。描き直したページはスクリプトを動かさず、ダッシュボードとも別のオリジンになるので、ウィンドウはページの中で移った先と題をここから知る。seq は描き直すたびに増える。
 * ponytail: ウィンドウを区別しないので、2 つのウィンドウで同時に移ると取り違える。困ったら、ウィンドウごとの id を中継の URL に付けて分ける。
 */
let lastFrame = { seq: 0, url: '', title: '' };

/**
 * Internet Explorer のウィンドウに出す、サーバーが描き直したサイトのページ。frame は中継の URL。
 * ダッシュボードと同じオリジンから返すので、CSP の sandbox で別のオリジンの扱いにし、スクリプトも止めて、ダッシュボードの画面を読み書きさせない。
 */
const sendRetro = async (res: ServerResponse, page: Promise<string>, frame: URL): Promise<void> => {
  const html = await page.catch((e: Error) => {
    throw new HttpError(502, e.message);
  });
  lastFrame = { seq: lastFrame.seq + 1, url: siteOfFrame(frame) ?? '', title: findTitle(html) ?? '' };
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

/** リポジトリの外のファイルを読む。modifiedAt は、メモ帳で上書きするときに、読んだ後にほかで書き換えられていないかを確かめるのに使う。 */
const localFile = (id: string, url: URL): { path: string; content: string; modifiedAt: number } => {
  const { ref, full, stat } = localPath(id, url);
  if (!stat || stat.size > MAX_LOCAL_FILE_BYTES) throw new HttpError(404, `「${ref}」を読めません`);
  return { path: platform.displayPath(full), content: fs.readFileSync(full, 'utf8'), modifiedAt: stat.mtimeMs };
};

/** メモ帳で編集したファイルを上書きする。メモ帳には「名前を付けて保存」が無いので、すでにあるファイルだけを上書きする。 */
const saveLocalFile = (url: URL, body: unknown): TextSaveResult => {
  const { ref, full, stat } = localPath('', url);
  if (!stat) throw new HttpError(404, `「${ref}」に書き込めません`);
  return saveText(full, field<string>(body, 'content', 'string'), field<number>(body, 'modifiedAt', 'number'));
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
  if (stat && /\.md$/i.test(full) && stat.size <= MAX_LOCAL_FILE_BYTES) return { where: 'local', path: full, heading: firstHeading(fs.readFileSync(full, 'utf8')) };
  if (stat && mediaType(full)) return { where: 'local', path: full };
  if (id && (await inRepo(sessionMeta(id).cwd, ref))) return { where: 'repo' };
  if (stat && stat.size <= MAX_LOCAL_FILE_BYTES) return { where: 'local', path: full };
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

/**
 * セッションのブランチの、開いている PR。url を渡すと、居たことのあるブランチの PR からその URL のものを選ぶ。
 * 閉じた PR のレビューコメントには対応しないので、扱わない。
 */
const openPr = (id: string, url?: unknown): { cwd: string; branch: string; pr: PrInfo } => {
  const meta = sessionMeta(id);
  const log = meta.transcriptPath ? summarizeLog(meta.transcriptPath) : undefined;
  const branch = url ? [...(log?.branches ?? [])].find((b) => cachedPr(meta.cwd, b)?.url === url) : log?.branch;
  // セッションが居なかったブランチの PR は、人間がバッジに登録したものから探す。
  const pr = cachedPr(meta.cwd, branch) ?? (typeof url === 'string' && meta.badges?.includes(url) ? badgePr(url) : undefined);
  const head = branch ?? pr?.headRef;
  if (!head || pr?.state !== 'OPEN') throw new HttpError(404, `開いている PR がありません: ${id}`);
  return { cwd: meta.cwd, branch: head, pr };
};

const badgeOfBody = (body: unknown): { kind: 'pr' | 'issue' | 'jira'; url: string } => {
  const badge = badgeOf(field<string>(body, 'url', 'string').trim());
  if (!badge) throw new HttpError(400, 'PR、Issue、JIRA の課題の URL か、#番号か、課題のキーではありません');
  return badge;
};

/**
 * PR と Issue は、読めることを確かめてから登録する。外していたバッジは、また出す。
 * #番号はセッションのリポジトリの PR か Issue を指し、どちらかは読むまで分からないので Issue として読む。
 */
const addBadge = async (id: string, body: unknown): Promise<void> => {
  const meta = sessionMeta(id);
  const text = field<string>(body, 'url', 'string').trim();
  const numbered = /^#\d+$/.test(text);
  const { kind, url: given } = numbered ? { kind: 'issue' as const, url: text } : badgeOfBody(body);
  // URL の PR と Issue は、作業ツリーを消したセッションでも登録できるよう、cwd を渡さずに読む。
  const url = kind === 'jira' ? given : await viaGh(kind === 'pr' ? 'PR を読めません' : 'Issue を読めません', () => loadBadge(kind, given, numbered ? meta.cwd : undefined));
  meta.badges = [...(meta.badges ?? []).filter((u) => u !== url), url];
  meta.hiddenBadges = meta.hiddenBadges?.filter((u) => u !== url);
  touch();
};

/** 登録したバッジは登録から消す。ブランチや set_task から見つけたバッジは、見つけ直しても出さないよう覚えておく。 */
const removeBadge = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const { url } = badgeOfBody(body);
  meta.badges = meta.badges?.filter((u) => u !== url);
  meta.hiddenBadges = [...new Set([...(meta.hiddenBadges ?? []), url])];
  touch();
};

/** gh や JIRA、Notion の失敗はダッシュボードの不具合ではないので、改善要望にしない。 */
const viaGh = async <T,>(what: string, call: () => Promise<T>): Promise<T> => {
  try {
    return await call();
  } catch (e) {
    throw new HttpError(502, `${what}（${(e as Error).message.split('\n')[0]}）`);
  }
};

/** 人間が「更新」を押したか。外部の情報源から読む値を、覚えた値を使わずに読み直す。 */
const wantsFresh = (url: URL): boolean => url.searchParams.has('fresh');

const threadsOf = (pr: PrInfo, fresh = false): Promise<PrThread[]> => viaGh('レビューコメントを取得できません', () => prThreads(pr, fresh));

/** 補完の候補を探すディレクトリ。セッションの欄ではセッションの作業ディレクトリ、新規セッションの欄では入力したディレクトリ。 */
const completionCwd = (body: unknown): string => {
  const { sessionId } = body as { sessionId?: unknown };
  return typeof sessionId === 'string' ? sessionMeta(sessionId).cwd : field<string>(body, 'cwd', 'string');
};

/** # の補完に出す、そのディレクトリのリポジトリの PR と Issue。 */
const githubRefs = (body: unknown) => {
  const cwd = completionCwd(body);
  const { fresh } = body as { fresh?: unknown };
  return viaGh('PR と Issue を読めません', () => repoRefs(cwd, fresh === true));
};

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

const prNotes = async (id: string, body: unknown): Promise<PrNotes> => {
  const { cwd, branch, pr } = openPr(id, (body as { pr?: unknown }).pr);
  const threads = await threadsOf(pr);
  const tree = await treeOf(cwd, branch).catch((e: Error) => {
    throw new HttpError(400, `リポジトリを読めません（${e.message.split('\n')[0]}）`);
  });
  return notesFor(tree, pr, threads);
};

/** JIRA の一覧のウィジェットが、設定した JQL で課題を探す。件数は JIRA_LIST_MAX までに収め、無ければ上限まで返す。 */
const jiraSearch = async (url: URL): Promise<JiraIssue[]> => {
  const jql = url.searchParams.get('jql')?.trim();
  if (!jql) throw new HttpError(400, 'jql is required');
  const max = Math.min(Math.max(Math.trunc(Number(url.searchParams.get('max'))) || JIRA_LIST_MAX, 1), JIRA_LIST_MAX);
  try {
    return await jiraList(jql, max);
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

/** 人間がダッシュボードで選んだトランジションで、課題のステータスを移す。id は JIRA のトランジションの id で、数字だけでできている。 */
const transitionIssue = async (key: string, body: unknown): Promise<void> => {
  const id = field<string>(body, 'id', 'string');
  if (!/^\d+$/.test(id)) throw new HttpError(400, 'id must be digits');
  await viaGh('ステータスを変えられません', () => transitionJira(jiraKeyOf(key), id));
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

/**
 * / の補完に出す skills。セッションの欄では Claude Code がそのセッションに見せたものを、新規セッションの欄では入力したディレクトリのものを出す。
 * 一覧がまだ無ければ、アーカイブしていないほかのセッションから借りる。アーカイブしたものまで会話ログを読むと遅いので見ない。
 */
const skillsOf = (target: { sessionId?: string; cwd?: string }): SkillInfo[] => {
  const meta = target.sessionId ? sessionMeta(target.sessionId) : undefined;
  const own = meta?.transcriptPath ? summarizeLog(meta.transcriptPath).skills : new Map<string, string>();
  const sources = () =>
    Object.values(state.sessions).flatMap((m) => {
      if (m.archived || !m.transcriptPath) return [];
      const log = summarizeLog(m.transcriptPath);
      return [{ cwd: m.cwd, skills: log.skills, at: log.lastActivityAt ?? 0 }];
    });
  const skills = own.size ? own : borrowSkills(meta?.cwd ?? target.cwd, sources());
  return [...skills].map(([name, description]) => ({ name, description }));
};

const readDoc = (id: string): Doc => {
  const doc = state.docs.find((d) => d.id === id);
  if (!doc) throw new HttpError(404, `unknown doc: ${id}`);
  if (!doc.path) return { ...doc, content: doc.content ?? '' };
  // 画像と動画は、画面がファイルをそのまま読み込む。
  if (doc.format === 'image' || doc.format === 'video') return { ...doc, content: '' };
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

const queueText = (body: unknown): string => {
  const text = field<string>(body, 'text', 'string');
  if (!text.trim()) throw new HttpError(400, 'text is required');
  return text;
};

/** 「キュー」で保留する。今送れるセッションなら、touch から呼ばれる flushQueue がすぐに送る。送らずに残したかを返す。 */
const queueMessage = (id: string, body: unknown): { queued: boolean } => {
  const message: QueuedMessage = { id: newId(), sessionId: sessionMeta(id).id, text: queueText(body), createdAt: Date.now() };
  state.queue.push(message);
  touch();
  return { queued: state.queue.includes(message) };
};

const findQueued = (id: string): QueuedMessage => {
  const message = state.queue.find((m) => m.id === id);
  if (!message) throw new HttpError(404, `no queued message: ${id}`);
  return message;
};

/** edit で書き直しを始め、save で書き直した文に置き換えて書き直しを終える。cancel で保留をやめる。 */
const updateQueue = (id: string, action: 'edit' | 'save' | 'cancel', body: unknown): void => {
  const message = findQueued(id);
  if (action === 'cancel') state.queue = state.queue.filter((m) => m !== message);
  else Object.assign(message, action === 'edit' ? { editing: true } : { editing: undefined, text: queueText(body) });
  touch();
};

type Handler = (m: string[], body: unknown, url: URL, req: IncomingMessage) => unknown;

const setTheme = (id: string, body: unknown): void => {
  sessionMeta(id).theme = field<string>(body, 'theme', 'string').trim() || undefined;
  touch();
};

/** アーカイブしたセッションも、一覧に出なくなったセッションも、まとめて新しい名前へ移す。並べ替えたテーマは、並べた位置に残す。 */
const setThemeName = (body: unknown): void => {
  const from = field<string>(body, 'from', 'string');
  const to = field<string>(body, 'to', 'string').trim();
  if (!to) throw new HttpError(400, 'to is required');
  renameTheme(Object.values(state.sessions), from, to);
  state.themeOrder = state.themeOrder.map((t) => (t === from ? to : t));
  touch();
};

const setTitle = (id: string, body: unknown): void => {
  sessionMeta(id).title = field<string>(body, 'title', 'string').trim() || undefined;
  touch();
};

/**
 * 作業中のセッションはアーカイブ待ちにして、deferred を返す。Claude Code の一覧から消し終わるのは待たない。
 * アーカイブを解除すると、アーカイブ待ちもやめる。
 */
const setArchived = (id: string, body: unknown): { deferred?: boolean } => {
  const meta = sessionMeta(id);
  if (!field<boolean>(body, 'archived', 'boolean')) {
    Object.assign(meta, { archived: false, archiveWhenIdle: undefined });
    touch();
    return {};
  }
  return { deferred: archiveSession(meta) };
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

const orderThemes = (body: unknown): void => {
  state.themeOrder = strings(body, 'themes');
  touch();
};

const orderDocs = (id: string, body: unknown): void => {
  sessionMeta(id).docOrder = strings(body, 'keys');
  touch();
};

/** 題名を空にすると、付け直す前の題名に戻す。 */
const renameDoc = (id: string, body: unknown): DocEditResult => {
  const meta = sessionMeta(id);
  const key = field<string>(body, 'key', 'string');
  const title = field<string>(body, 'title', 'string').trim();
  const titles = { ...meta.docTitles, [key]: title };
  if (!title) delete titles[key];
  meta.docTitles = titles;
  touch();
  return { at: Date.now() };
};

const pinDocs = (id: string, body: unknown): DocEditResult => {
  const meta = sessionMeta(id);
  const keys = new Set(strings(body, 'keys'));
  const rest = (meta.pinnedDocs ?? []).filter((k) => !keys.has(k));
  meta.pinnedDocs = field<boolean>(body, 'pinned', 'boolean') ? [...rest, ...keys] : rest;
  touch();
  return { at: Date.now() };
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

/** ほかのセッションの資料から足した present の資料。同じ資料を足し直したら時刻を新しくし、外していたものはまた並べる。このセッションの資料は、外していたものをまた並べるだけにする。 */
const shareDoc = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const docId = field<string>(body, 'id', 'string');
  const doc = state.docs.find((d) => d.id === docId);
  if (!doc) throw new HttpError(404, '資料が見つかりません');
  if (doc.sessionId !== meta.id) meta.sharedDocs = [...(meta.sharedDocs ?? []).filter((d) => d.id !== docId), { id: docId, at: Date.now() }];
  meta.hiddenDocs = meta.hiddenDocs?.filter((k) => k !== docId);
  touch();
};

/** ほかのセッションの資料から足した、校正した文章。題名はファイルの今の名前にする。同じ文章を足し直したら時刻を新しくし、外していたものはまた並べる。 */
const addProof = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const proofId = field<string>(body, 'id', 'string');
  const file = /^[\w-]{1,64}$/.test(proofId) ? readProofFile(proofId) : null;
  if (!file) throw new HttpError(404, '文章校正アプリのファイルが見つかりません');
  meta.proofFiles = [...(meta.proofFiles ?? []).filter((p) => p.id !== proofId), { id: proofId, title: file.title, at: Date.now() }];
  meta.hiddenDocs = meta.hiddenDocs?.filter((k) => k !== proofId);
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

// 用語集に並べる語句の長さの上限。文章を貼り付けたものは断る。
const MAX_TERM_CHARS = 100;

/** 用語集に足す語。説明が空なら、haiku にやり取りから書かせてから足す。同じ語を足し直したら、説明と時刻を新しくし、外していた語はまた並べる。 */
const addTerm = async (id: string, body: unknown): Promise<void> => {
  const meta = sessionMeta(id);
  const term = field<string>(body, 'term', 'string').trim();
  if (!term || term.length > MAX_TERM_CHARS) throw new HttpError(400, `term must be 1 to ${MAX_TERM_CHARS} characters`);
  const description = field<string>(body, 'description', 'string').trim() || (await explainTerm(term, chatOf(id)));
  const key = termKey(term);
  meta.terms = [...(meta.terms ?? []).filter((t) => termKey(t.term) !== key), { term, description, at: Date.now() }];
  meta.hiddenTerms = meta.hiddenTerms?.filter((k) => k !== key);
  touch();
};

/** 外した語は、やり取りから拾い直しても並べない。人間が足した語は、覚えたものも消す。 */
const hideTerm = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const key = termKey(field<string>(body, 'term', 'string'));
  meta.terms = meta.terms?.filter((t) => termKey(t.term) !== key);
  meta.hiddenTerms = [...new Set([...(meta.hiddenTerms ?? []), key])];
  touch();
};

// ブックマークに持つ本文の長さ。一覧で発言を見分けられるだけの先頭を持つ。
const BOOKMARK_TEXT_CHARS = 200;

/** チャットの発言をブックマークに足すか、外す。発言は種類と時刻で見分け、本文は一覧に出す先頭だけを持つ。 */
const setBookmark = (id: string, body: unknown): void => {
  const meta = sessionMeta(id);
  const kind = field<string>(body, 'kind', 'string');
  if (kind !== 'prompt' && kind !== 'message') throw new HttpError(400, 'kind must be prompt or message');
  const at = field<number>(body, 'at', 'number');
  const rest = (meta.bookmarks ?? []).filter((b) => bookmarkKey(b) !== bookmarkKey({ kind, at }));
  meta.bookmarks = field<boolean>(body, 'on', 'boolean') ? [...rest, { kind, at, text: field<string>(body, 'text', 'string').slice(0, BOOKMARK_TEXT_CHARS), createdAt: Date.now() }] : rest;
  touch();
};

const glossaryOf = (id: string): Glossary => {
  const meta = sessionMeta(id);
  return sessionGlossary(meta.id, chatOf(id), readLiveSessions().get(meta.id)?.status === 'busy', meta.terms ?? [], meta.hiddenTerms ?? []);
};

/** 外した資料は、発言にまた出ても並べない。足したリンクとファイルとほかのセッションの資料と、Claude が校正した文章は、覚えたものも消す。 */
const hideDocs = (id: string, body: unknown): DocEditResult => {
  const meta = sessionMeta(id);
  const keys = new Set(strings(body, 'keys'));
  meta.hiddenDocs = [...new Set([...(meta.hiddenDocs ?? []), ...keys])];
  meta.links = meta.links?.filter((l) => !keys.has(l.url));
  meta.files = meta.files?.filter((f) => !keys.has(f.path));
  meta.proofFiles = meta.proofFiles?.filter((p) => !keys.has(p.id));
  meta.sharedDocs = meta.sharedDocs?.filter((d) => !keys.has(d.id));
  meta.docOrder = meta.docOrder?.filter((k) => !keys.has(k));
  meta.pinnedDocs = meta.pinnedDocs?.filter((k) => !keys.has(k));
  touch();
  return { at: Date.now() };
};

/** 閉じた端末への入力と大きさの変更は、画面がつなぎ直すまでの間に届くので、404 で知らせる。 */
const terminalInput = (id: string, body: unknown): void => {
  if (!writeTerminal(id, field<string>(body, 'data', 'string'))) throw new HttpError(404, '端末は閉じています');
};

const terminalResize = (id: string, body: unknown): void => {
  const { cols, rows } = body as { cols?: unknown; rows?: unknown };
  if (!Number.isInteger(cols) || !Number.isInteger(rows) || Number(cols) < 1 || Number(rows) < 1) throw new HttpError(400, 'cols and rows must be positive integers');
  if (!resizeTerminal(id, Number(cols), Number(rows))) throw new HttpError(404, '端末は閉じています');
};

/** 起動したセッションは、セッションファイルとフックから一覧に現れる。 */
const newSession = (body: unknown): Promise<void> => {
  const cwd = field<string>(body, 'cwd', 'string');
  const prompt = field<string>(body, 'prompt', 'string');
  if (!prompt.trim()) throw new HttpError(400, 'prompt is required');
  if (!path.isAbsolute(cwd) || !fs.statSync(cwd, { throwIfNoEntry: false })?.isDirectory()) {
    throw new HttpError(400, `ディレクトリがありません: ${cwd}`);
  }
  const { theme } = body as { theme?: unknown };
  return startSession(cwd, prompt, oneOf(body, 'model', MODELS), oneOf(body, 'effort', EFFORT_LEVELS), typeof theme === 'string' ? theme.trim() || undefined : undefined);
};

const conversation = (id: string): Conversation => {
  const c = conversationOf(id);
  if (!c) throw new HttpError(404, `no conversation: ${id}`);
  return c;
};

const suggestions = (id: string): Replies => {
  const c = conversation(id);
  if (!c.lastMessage) throw new HttpError(404, `no message to reply: ${id}`);
  return peekReplies(c);
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

/** 画面で起きた例外と、画面が固まった処理。固まった処理には、固まっていた時間が ms に付く。改善要望の材料になるので、長すぎるものは切る。 */
const clientError = (body: unknown): void => {
  const { message, stack, url, ms } = (body ?? {}) as Record<string, unknown>;
  if (typeof message !== 'string' || !message.trim()) throw new HttpError(400, 'message is required');
  const text = (v: unknown): string | undefined => (typeof v === 'string' ? v.slice(0, MAX_CLIENT_ERROR_CHARS) : undefined);
  if (typeof ms === 'number') recordSlow('client', message.slice(0, MAX_CLIENT_ERROR_CHARS), ms, text(url), text(stack));
  else recordError('client', message.slice(0, MAX_CLIENT_ERROR_CHARS), text(stack), text(url));
};

const toComments = (body: unknown): ReviewComment[] =>
  field<unknown[]>(body, 'comments', 'object').map((c) => {
    const r = c as Partial<ReviewComment>;
    if (typeof r.path !== 'string' || typeof r.body !== 'string') throw new HttpError(400, 'comment needs path and body');
    return { path: r.path, line: Number(r.line) || 0, side: r.side === 'old' ? 'old' : 'new', lineText: String(r.lineText ?? ''), body: r.body };
  });

/** branch は、人間がブランチを表示してコメントを付けたときに送られる。 */
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** 過去の依頼を書き直して、その依頼の直前まで会話を巻き戻して送る。 */
const rewind = (id: string, body: unknown): ReturnType<typeof sendMessage> => {
  const to = field<string>(body, 'rewindTo', 'string');
  if (!UUID.test(to)) throw new HttpError(400, 'rewindTo must be uuid');
  return sendMessage(sessionMeta(id).id, field<string>(body, 'text', 'string'), to);
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

/** 依頼と、起動に渡すモデルと effort。 */
const launchOf = (body: unknown): { text: string; model: string; effort?: string } => {
  const text = field<string>(body, 'text', 'string');
  if (!text.trim()) throw new HttpError(400, 'text is required');
  const model = field<string>(body, 'model', 'string');
  if (model && !(MODELS as readonly string[]).includes(model) && !MODEL_ID.test(model)) throw new HttpError(400, 'model must be alias or model id');
  return { text, model, effort: oneOf(body, 'effort', EFFORT_LEVELS) };
};

/** モデルと effort を変えて依頼を渡す。 */
const switchModel = async (id: string, body: unknown): Promise<{ sessionId: string }> => {
  const { text, model, effort } = launchOf(body);
  return { sessionId: await switchSession(sessionMeta(id).id, text, model, effort) };
};

/** 会話を引き継いだ別のセッションを起動して依頼を渡し、元のセッションは残す。起動できないのは claude の失敗なので 502 にする。 */
const fork = async (id: string, body: unknown): Promise<{ sessionId: string }> => {
  const { text, model, effort } = launchOf(body);
  const sessionId = sessionMeta(id).id;
  try {
    return { sessionId: await forkSession(sessionId, text, model, effort) };
  } catch (e) {
    throw new HttpError(502, (e as Error).message);
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

const proofSentences = (text: string): ReturnType<typeof parseProof> => {
  const sentences = parseProof(text);
  if (!sentences.length) throw new HttpError(400, 'text is required');
  if (sentences.length > MAX_SENTENCES) throw new HttpError(400, `文が多すぎます。${MAX_SENTENCES} 文までにしてください（${sentences.length} 文あります）`);
  return sentences;
};

/** 文章校正の実行の種類。mode を送らない画面は明確化を求めている。 */
const proofMode = (body: unknown): ProofMode => ((body as { mode?: unknown }).mode === 'summarize' ? 'summarize' : 'clarify');

/** 文章校正の進み具合を、出来事ごとに 1 行の JSON で流す。画面が読むのをやめたら、残りの文を読ませない。続きから明確化するときは、前に読んだ文の答えを readings で受ける。 */
const proofreadStream = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
  const body = await readBody(req);
  const text = field<string>(body, 'text', 'string');
  const glossary = field<string>(body, 'glossary', 'string');
  const sentences = proofSentences(text);
  const stop = new AbortController();
  res.on('close', () => stop.abort());
  res.writeHead(200, { 'content-type': 'application/x-ndjson; charset=utf-8', 'cache-control': 'no-store' });
  const emit = (e: ProofEvent) => res.write(`${JSON.stringify(e)}\n`);
  if (proofMode(body) === 'summarize') await summarizeProof(text, sentences, glossary, emit, stop.signal);
  else await proofread(text, sentences, glossary, emit, stop.signal, priorReadings((body as { readings?: unknown }).readings, sentences.length));
  res.end();
};

// 上級の盤面（30×16）まで受ける。
const MAX_MINES_CELLS = 480;

/** rows は人間に見えているマインスイーパの盤面。地雷の位置は渡させない。 */
const minesHintOf = async (body: unknown): Promise<{ odds: (number | null)[] }> => {
  if (!hasJev()) throw new HttpError(503, 'Jev のキーを設定していません');
  const rows = strings(body, 'rows');
  const mines = (body as Record<string, unknown>).mines;
  const width = rows[0]?.length ?? 0;
  if (!width || !rows.every((r) => MINES_ROW.test(r) && r.length === width) || rows.length * width > MAX_MINES_CELLS || !Number.isInteger(mines)) {
    throw new HttpError(400, 'rows must be rows of the same length made of . F 0-8, and mines must be an integer');
  }
  return { odds: await minesHint(rows, mines as number) };
};

/** 校正した文章を、残した指摘とコメントに沿って書き直す。fixed は固定した文の番号（0 から）。 */
const reviseProof = async (body: unknown): Promise<{ text: string }> => {
  const text = field<string>(body, 'text', 'string');
  const fixed = field<unknown[]>(body, 'fixed', 'object');
  const notes = field<unknown[]>(body, 'notes', 'object');
  if (!Array.isArray(fixed) || !Array.isArray(notes)) throw new HttpError(400, 'fixed and notes must be arrays');
  const valid = notes.filter((n): n is ReviseNote => Number.isInteger((n as ReviseNote)?.i) && typeof (n as ReviseNote).message === 'string');
  return { text: await revise(text, proofSentences(text), field<string>(body, 'glossary', 'string'), new Set(fixed.filter(Number.isInteger) as number[]), valid, proofMode(body)) };
};

/** 文章校正のファイルは画面が組み立てたものを残す。開き直したときに画面が壊れないよう、骨組みだけを確かめる。 */
const proofFileOf = (id: string, body: unknown): ProofFile => {
  const versions = field<unknown[]>(body, 'versions', 'object');
  const isVersion = (v: unknown): boolean => {
    const { text, fixed, comments } = (v ?? {}) as Partial<ProofVersion>;
    return typeof text === 'string' && Array.isArray(fixed) && typeof comments === 'object' && comments !== null;
  };
  if (!Array.isArray(versions) || !versions.length || !versions.every(isVersion)) throw new HttpError(400, 'versions must be a non-empty array of versions');
  return { id, title: field<string>(body, 'title', 'string'), glossary: field<string>(body, 'glossary', 'string'), versions: versions as ProofVersion[], updatedAt: Date.now() };
};

// API が返るまでの見込み。超えたら、遅い処理として改善要望にする。
// ダッシュボードの中で済む処理は、待たされたと感じさせないよう 1 秒で見込む。外部を待つ API は、待つ相手に合わせて見込む。
const OWN_MS = 1000;
// gh、JIRA、Notion、Jev の API。
const WEB_API_MS = 5000;
// claude でセッションを起動し、止め、再開する。止まるのを待つだけで十数秒かかることがある。
const CLAUDE_MS = 20_000;
const HAIKU_MS = 15_000;
// コードや長い文章を読ませて書かせる。数分かかることがある。
const READER_MS = 5 * 60_000;

/** 経路、処理、返るまでの見込み（ミリ秒）。見込みを省くと OWN_MS。 */
type Route = [RegExp, Handler, number?];

const POST_ROUTES: Route[] = [
  [/^\/api\/wallpaper\/url$/, (_, b) => setWallpaperUrl(b)],
  [/^\/api\/wallpaper\/reset$/, () => clearWallpaper(DATA_DIR)],
  [/^\/api\/sessions$/, (_, b) => newSession(b), CLAUDE_MS],
  [/^\/api\/sessions\/order$/, (_, b) => orderSessions(b)],
  [/^\/api\/sessions\/([^/]+)\/theme$/, ([, id = ''], b) => setTheme(id, b)],
  [/^\/api\/themes\/rename$/, (_, b) => setThemeName(b)],
  [/^\/api\/themes\/order$/, (_, b) => orderThemes(b)],
  [/^\/api\/sessions\/([^/]+)\/title$/, ([, id = ''], b) => setTitle(id, b)],
  [/^\/api\/sessions\/([^/]+)\/archive$/, ([, id = ''], b) => setArchived(id, b)],
  [/^\/api\/sessions\/([^/]+)\/pin$/, ([, id = ''], b) => setPinned(id, b)],
  [/^\/api\/sessions\/([^/]+)\/links$/, ([, id = ''], b) => addLink(id, b)],
  [/^\/api\/sessions\/([^/]+)\/files$/, ([, id = ''], b) => addFile(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs$/, ([, id = ''], b) => shareDoc(id, b)],
  [/^\/api\/sessions\/([^/]+)\/proofs$/, ([, id = ''], b) => addProof(id, b)],
  [/^\/api\/sessions\/([^/]+)\/badges$/, ([, id = ''], b) => addBadge(id, b), WEB_API_MS],
  [/^\/api\/sessions\/([^/]+)\/badges\/remove$/, ([, id = ''], b) => removeBadge(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/order$/, ([, id = ''], b) => orderDocs(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/hide$/, ([, id = ''], b) => hideDocs(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/rename$/, ([, id = ''], b) => renameDoc(id, b)],
  [/^\/api\/sessions\/([^/]+)\/docs\/pin$/, ([, id = ''], b) => pinDocs(id, b)],
  [/^\/api\/sessions\/([^/]+)\/terms$/, ([, id = ''], b) => addTerm(id, b), HAIKU_MS],
  [/^\/api\/sessions\/([^/]+)\/terms\/hide$/, ([, id = ''], b) => hideTerm(id, b)],
  [/^\/api\/sessions\/([^/]+)\/bookmarks$/, ([, id = ''], b) => setBookmark(id, b)],
  [/^\/api\/sessions\/([^/]+)\/message$/, ([, id = ''], b) => sendMessage(sessionMeta(id).id, field<string>(b, 'text', 'string'))],
  [/^\/api\/sessions\/([^/]+)\/queue$/, ([, id = ''], b) => queueMessage(id, b)],
  [/^\/api\/sessions\/([^/]+)\/rewind$/, ([, id = ''], b) => rewind(id, b)],
  [/^\/api\/sessions\/([^/]+)\/switch$/, ([, id = ''], b) => switchModel(id, b), CLAUDE_MS],
  [/^\/api\/sessions\/([^/]+)\/fork$/, ([, id = ''], b) => fork(id, b), CLAUDE_MS],
  [/^\/api\/sessions\/([^/]+)\/compact$/, ([, id = '']) => compactSession(sessionMeta(id).id, sessionMeta(id).cwd), CLAUDE_MS],
  [/^\/api\/sessions\/([^/]+)\/stop$/, ([, id = '']) => stopSession(sessionMeta(id).id), CLAUDE_MS],
  [/^\/api\/sessions\/([^/]+)\/offline$/, ([, id = ''], b) => answerOffline(sessionMeta(id).id, field<boolean>(b, 'resume', 'boolean')), CLAUDE_MS],
  [/^\/api\/sessions\/([^/]+)\/review$/, ([, id = ''], b) => review(id, b)],
  [/^\/api\/sessions\/([^/]+)\/pr-review$/, ([, id = ''], b) => postReview(id, b), WEB_API_MS],
  [/^\/api\/sessions\/([^/]+)\/pr-viewed$/, ([, id = ''], b) => setViewed(id, b), WEB_API_MS],
  // 候補や概要や PR の分析は作るたびに API を使うので、他のサイトから GET で起こされないよう POST で受ける。
  [/^\/api\/sessions\/([^/]+)\/suggestions$/, ([, id = '']) => suggestions(id)],
  [/^\/api\/sessions\/([^/]+)\/title-suggestion$/, async ([, id = '']) => ({ title: await suggestTitle(conversation(id)) }), HAIKU_MS],
  [/^\/api\/sessions\/([^/]+)\/summary$/, ([, id = '']) => summarize(conversation(id)), HAIKU_MS],
  [/^\/api\/sessions\/([^/]+)\/glossary$/, ([, id = '']) => glossaryOf(id)],
  [/^\/api\/sessions\/([^/]+)\/diagram$/, ([, id = ''], b) => makeDiagram(sessionMeta(id).id, sessionMeta(id).cwd, chatOf(id), field<string>(b, 'note', 'string')), READER_MS],
  [/^\/api\/sessions\/([^/]+)\/pr-notes$/, ([, id = ''], b) => prNotes(id, b), READER_MS],
  [/^\/api\/attention\/([^/]+)\/answer$/, ([, id = ''], b) => ({ delivery: answerQuestion(id, field<string>(b, 'answer', 'string')) })],
  [/^\/api\/attention\/([^/]+)\/dismiss$/, ([, id = '']) => dismiss(id)],
  [/^\/api\/attention\/([^/]+)\/allow$/, ([, id = '']) => allowPermission(id)],
  [/^\/api\/outbox\/([^/]+)\/(manual|cancel|retry)$/, ([, id = '', action]) => updateOutbox(id, action as 'manual' | 'cancel' | 'retry')],
  [/^\/api\/queue\/([^/]+)\/(edit|save|cancel)$/, ([, id = '', action], b) => updateQueue(id, action as 'edit' | 'save' | 'cancel', b)],
  [/^\/api\/queue\/([^/]+)\/send$/, ([, id = '']) => ({ delivery: sendQueued(findQueued(id)) })],
  [/^\/api\/pr\/comment$/, (_, b) => commentPr(b), WEB_API_MS],
  // 渡されたディレクトリで gh や git を動かすので、他のサイトから GET で起こされないよう POST で受ける。
  [/^\/api\/github\/refs$/, (_, b) => githubRefs(b), WEB_API_MS],
  [/^\/api\/files$/, (_, b) => fileCandidates(completionCwd(b), field<string>(b, 'q', 'string'), bump)],
  [/^\/api\/jira\/refresh$/, () => refreshJira()],
  [/^\/api\/jira\/issues\/([^/]+)\/comment$/, ([, key = ''], b) => commentJira(key, b), WEB_API_MS],
  [/^\/api\/jira\/issues\/([^/]+)\/transitions$/, ([, key = ''], b) => transitionIssue(key, b), WEB_API_MS],
  [/^\/api\/feedback$/, (_, b) => feedback(b)],
  [/^\/api\/proofread\/revise$/, (_, b) => reviseProof(b), READER_MS],
  [/^\/api\/terminal\/([\w-]+)\/input$/, ([, id = ''], b) => terminalInput(id, b)],
  [/^\/api\/terminal\/([\w-]+)\/resize$/, ([, id = ''], b) => terminalResize(id, b)],
  [/^\/api\/proofread\/files\/([\w-]{1,64})$/, ([, id = ''], b) => writeProofFile(proofFileOf(id, b))],
  [/^\/api\/errors$/, (_, b) => clientError(b)],
  [/^\/api\/local-file$/, (_, b, url) => saveLocalFile(url, b)],
  // 作るたびに Jev を使うので、他のサイトから GET で起こされないよう POST で受ける。
  [/^\/api\/minesweeper\/hint$/, (_, b) => minesHintOf(b), WEB_API_MS],
  [/^\/api\/assistant$/, async (_, b) => askDolphin(await logsReady.then(buildSnapshot), toMessages(b), await focusOf(b)), HAIKU_MS],
  [/^\/api\/improve$/, (_, b) => updateImprove(improveSettings(b))],
  [/^\/api\/auto-continue$/, (_, b) => setAutoContinue(field<boolean>(b, 'enabled', 'boolean'))],
  [/^\/api\/auth\/([^/]+)$/, ([, id = ''], b) => answerAuth(id, field<boolean>(b, 'ok', 'boolean'))],
  // ブラウザでのログインは数分かかるので、終わるのを待たずに返す。進み具合と失敗は、スナップショットで知らせる。
  [/^\/api\/claude\/login$/, () => void loginClaude().catch(() => {})],
  [/^\/api\/hook$/, (_, b, __, req) => handleHook(b as HookInput, Number(req.headers['x-claude-pid']) || 0)],
];

const memoryNow = latestMemory(() => platform.readMemory());

const GET_ROUTES: Route[] = [
  [/^\/api\/snapshot$/, () => buildSnapshot()],
  [/^\/api\/ie\/last-frame$/, () => lastFrame],
  [/^\/api\/search$/, (_, __, url) => searchSessions(url.searchParams.get('q') ?? '')],
  [/^\/api\/jira$/, (_, __, url) => jiraSearch(url), WEB_API_MS],
  [/^\/api\/jira\/refs$/, (_, __, url) => viaGh('JIRA の課題を読めません', () => jiraRefs(wantsFresh(url))), WEB_API_MS],
  [/^\/api\/github\/inbox$/, (_, __, url) => viaGh('GitHub の PR を探せません', () => githubInbox(wantsFresh(url))), WEB_API_MS],
  [/^\/api\/jira\/issues\/([^/]+)$/, ([, key = ''], _, url) => viaGh('課題を読めません', () => jiraDetail(jiraKeyOf(key), wantsFresh(url))), WEB_API_MS],
  [/^\/api\/notion\/pages\/([0-9a-f]{32})$/, ([, id = ''], _, url) => viaGh('ページを読めません', () => notionPage(id, wantsFresh(url))), WEB_API_MS],
  [/^\/api\/notion\/pages\/([0-9a-f]{32})\/title$/, ([, id = '']) => viaGh('ページを読めません', async () => ({ title: await notionTitle(id) })), WEB_API_MS],
  [/^\/api\/pr$/, (_, __, url) => {
    const pr = prUrlOf(url.searchParams.get('url'));
    return viaGh('PR を読めません', () => prDetail(pr, wantsFresh(url)));
  }, WEB_API_MS],
  [/^\/api\/memory$/, () => memoryNow()],
  [/^\/api\/proofread\/files$/, () => recentProofFiles()],
  // 無いファイルは、画面が新しく書き始めるので null を返す。読めないファイルと見分けるため、404 にはしない。
  [/^\/api\/proofread\/files\/([\w-]{1,64})$/, ([, id = '']) => readProofFile(id)],
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
  [/^\/api\/sessions\/([^/]+)\/skills$/, ([, id = '']) => skillsOf({ sessionId: id })],
  [/^\/api\/skills$/, (_, __, url) => skillsOf({ cwd: url.searchParams.get('cwd') || undefined })],
  [/^\/api\/sessions\/([^/]+)\/pr-threads$/, ([, id = ''], _, url) => threadsOf(openPr(id, url.searchParams.get('pr')).pr, wantsFresh(url)), WEB_API_MS],
  [/^\/api\/sessions\/([^/]+)\/pr-viewed$/, ([, id = ''], _, url) => viaGh('Viewed を取得できません', () => prViewed(openPr(id).pr, wantsFresh(url))), WEB_API_MS],
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
  if (req.method === 'POST' && url.pathname === '/api/proofread') return proofreadStream(req, res);
  // 人間が許可するか、Claude Code が確認を終えるまで応答しないので、遅い応答として数えない。
  if (req.method === 'POST' && url.pathname === '/api/hook/permission') return holdPermission(res, (await readBody(req)) as HookInput);
  if (req.method === 'GET' && url.pathname === '/api/favicon') return sendFavicon(req, res, url);
  if (req.method === 'GET' && url.pathname === '/api/github/attachment') return sendGithubAttachment(req, res, url);
  // 端末でコマンドを動かすので、ほかのサイトの img などから GET で起こされないよう、ダッシュボードの画面からの接続に限る。
  if (req.method === 'GET' && url.pathname === '/api/terminal') {
    const fetchSite = req.headers['sec-fetch-site'];
    if (fetchSite && fetchSite !== 'same-origin') throw new HttpError(403, 'forbidden');
    return streamTerminal(res, url.searchParams.get('cwd') || os.homedir());
  }
  if (req.method === 'GET' && url.pathname === GOOGLE_HOME_PATH) return sendRetro(res, Promise.resolve(OLD_GOOGLE_HOME), url);
  if (req.method === 'GET' && url.pathname === GOOGLE_SEARCH_PATH) return sendGoogle(req, res, url);
  if (req.method === 'GET' && url.pathname === WIKIPEDIA_PATH) return sendWikipedia(req, res, url);
  if (req.method === 'GET' && url.pathname === RETRO_PATH) return sendRetroSite(req, res, url);
  if (req.method === 'GET' && url.pathname === YAHOO_HOME_PATH) return sendRetro(res, fetchYahooHome(req.headers['accept-language']), url);
  if (req.method === 'GET' && url.pathname === YAHOO_SEARCH_PATH) return sendRetro(res, fetchYahooSearch(url.search, req.headers['accept-language']), url);
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
  // 要約を作っている間もサーバーはほかの要求に応え、画面は前に受け取ったスナップショットを出しているので、待った時間は応答の見込みに数えない。
  if (req.method === 'GET' && url.pathname === '/api/snapshot') await logsReady;
  for (const [pattern, handler, expectedMs = OWN_MS] of routes) {
    const m = pattern.exec(url.pathname);
    if (!m) continue;
    const body = req.method === 'POST' ? await readBody(req) : undefined;
    const started = performance.now();
    const elu = performance.eventLoopUtilization();
    try {
      const result = await handler(m.map((s) => decodeURIComponent(s)), body, url, req);
      if (result === undefined) return void res.writeHead(204).end();
      return json(req, res, result);
    } finally {
      const ms = performance.now() - started;
      if (ms > expectedMs) {
        // 外部を待っていたのか、ほかの処理でサーバーが塞がっていたのかを見分けられるよう、JavaScript を動かしていた時間を添える。
        const busy = `サーバーが JavaScript を動かしていた時間: ${(performance.eventLoopUtilization(elu).active / 1000).toFixed(1)} 秒（ほかの API や裏の処理も含む）。残りは、外部のコマンドや API、ファイルを待っていた時間。`;
        // 同じ API は経路でまとめる。経路の正規表現は、区切りの \/ を / に戻して読みやすくする。
        recordSlow('server', `${req.method} ${pattern.source.replaceAll('\\/', '/')} の応答（見込み ${expectedMs / 1000} 秒）`, ms, `${req.method} ${url.pathname}`, busy);
      }
    }
  }
  throw new HttpError(404, 'not found');
};

const server = http.createServer(async (req, res) => {
  if (!(await allowRequest(req.headers.host, req.headers.origin, PORT))) return void res.writeHead(403).end('forbidden');
  route(req, res).catch((e: unknown) => {
    // 入力の誤り（4xx）と、セッションの状態で断った操作（409）と、claude の失敗（502）は、ダッシュボードの不具合ではないので集めない。
    // 送り手が本文を送り終える前に切ったリクエストも集めない。フックの curl は、送っている途中でも 2 秒で打ち切る。
    const status = e instanceof HttpError ? e.status : e instanceof SessionStateError ? 409 : e instanceof UpstreamError ? 502 : 500;
    if (status === 500 && e !== req.errored) {
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
// 保留したメッセージは、セッションが入力待ちになったときと、質問への回答や前のメッセージの配信で状態が変わったときに送る。
onChange(flushQueue);
// アーカイブ待ちのセッションは、入力待ちか停止になったときにアーカイブする。作業中かどうかはセッションのファイルに出る。
onChange(flushArchives);
// 使用率の推移は statusline が変わるたびに記録する。起動時にも記録し、次の更新まで推移が空にならないようにする。
recordRateLimits();
watch(SESSIONS_DIR, false, () => {
  scheduleBroadcast();
  flushQueue();
  flushArchives();
});
watch(STATUSLINE_DIR, false, () => {
  recordRateLimits();
  scheduleBroadcast();
});
watch(TASKS_DIR, true, scheduleBroadcast);
watch(PROJECTS_DIR, true, (file) => {
  scheduleBroadcast();
  flushQueue();
  if (file) onTranscriptChange(file);
});
// 画面のファイルが作り直されたら、開いている画面に新しい更新時刻を配って読み込み直させる。
watch(path.join(ROOT, 'public'), false, scheduleBroadcast);
// 起動したときにも読む。次に書かれるまで待つと、前から残っていた状態まで、そのとき書かれたものとみなしてしまう。
mergeClaudePrCache(path.join(CLAUDE_DIR, PR_CACHE_FILE));
watch(CLAUDE_DIR, false, (file) => file === PR_CACHE_FILE && mergeClaudePrCache(path.join(CLAUDE_DIR, PR_CACHE_FILE)));
ensureImprover();
redeliverInterrupted();
flushQueue();
flushArchives();
// 起動したときは、一覧から消している途中でサーバーが止まったものも拾うので、消せなかったものが無くても消し直す。
retryRemovals();
setInterval(() => Object.values(state.sessions).some((m) => m.archived && m.removeError) && retryRemovals(), REMOVE_RETRY_MS);
checkLimits();

// 落ちる前に記録して改善要望にし、これまでどおり止まる。コードが直れば node --watch が起動し直す。
for (const event of ['uncaughtException', 'unhandledRejection'] as const) {
  process.on(event, (e: unknown) => {
    console.error(e);
    recordError('server', e instanceof Error ? e.message : String(e), e instanceof Error ? e.stack : undefined, event);
    save();
    saveCache();
    process.exit(1);
  });
}

for (const signal of ['SIGINT', 'SIGTERM'] as const) {
  process.on(signal, () => {
    save();
    saveCache();
    saveLogSummaries();
    process.exit(0);
  });
}

server.on('error', (e: NodeJS.ErrnoException) => {
  process.stderr.write(e.code === 'EADDRINUSE' ? `ポート ${PORT} は使用中です。DURUN_DASHBOARD_PORT で変更できます。\n` : `${e.message}\n`);
  process.exit(1);
});
server.listen(PORT, '127.0.0.1', () => process.stdout.write(`durun-dashboard: http://localhost:${PORT}\n`));
