// アプリ「ムービーメーカー」の中枢。ウィンドウがつないでいる間だけ、movie-maker の mm-serve につなぎ、変更の通知を受け、
// ウィンドウでつないだ 1 つのセッションに mm_ ツールを開く。人間の操作はフックで届け、ログはそのセッションの資料に足したファイルにも書く。
// 設計は docs/movie-maker-connect-design.md。
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import type { ServerResponse } from 'node:http';
import path from 'node:path';
import type { MmChange, MmLogEntry, MovieMakerView, MmLinkStatus } from '../shared/types.ts';
import { appendLog, logFilePath } from './flLogFile.ts';
import { type Installer, installedApp, installerFor, installPaths } from './movieMakerInstall.ts';
import { CHANGE_FORMAT, changeLine, findLink, hookText, isHuman, MM_DIR, MmClient, projectSummary, STORE_FILE, type ToolInfo, type ToolResult } from './movieMakerLink.ts';
import { platform } from './platform.ts';
import { DATA_DIR, newId, state, touch } from './store.ts';

const LOG_DIR = path.join(DATA_DIR, 'moviemaker');
const LOG_TITLE = 'ムービーメーカー';
const KEEPALIVE_MS = 20_000;
const VIEW_THROTTLE_MS = 150;
/** 覚えておく変更とログの数。 */
const MAX_RECORDS = 500;
const VIEW_LOG = 200;
const HOOK_LIMIT = 20;
const CHANGES_ALL = 50;
const OUTPUT_CHARS = 4000;

type Win = { res: ServerResponse; sessionId?: string };
type ChangeRecord = { seq: number; at: number; change: MmChange };

const windows = new Set<Win>();
let boundSession: string | undefined;
let projectDir: string | undefined;
let installer: Installer | undefined;
let watchers: fs.FSWatcher[] = [];
let client: MmClient | undefined;
let linkAbort: AbortController | undefined;
let linkStatus: MmLinkStatus = 'no-project';
let linkMessage: string | undefined;
let version: number | undefined;
let undoDepth: number | undefined;
let tools: ToolInfo[] | undefined;
let records: ChangeRecord[] = [];
let seq = 0;
/** ウィンドウに見せるログ。人間の操作とエージェントのツールの呼び出しを届いた順に並べる。 */
let log: MmLogEntry[] = [];
/** セッションごとに、どの変更まで届けたか。フックと mm_changes で共有する。 */
const cursors = new Map<string, number>();

const sessionName = (id: string): string => state.sessions[id]?.title ?? id.slice(0, 8);
const getInstaller = (): Installer => {
  if (!installer) {
    installer = installerFor(DATA_DIR);
    installer.onChange(broadcast);
  }
  return installer;
};

// ─── ウィンドウ ─────────────────────────────────────────────────────────

const youEntry = (r: ChangeRecord): MmLogEntry => ({ id: `r${r.seq}`, at: r.at, who: 'you', change: r.change });

const logFileOf = (sessionId: string | undefined): string | undefined => {
  const file = sessionId && logFilePath(LOG_DIR, sessionId, Date.now(), LOG_TITLE);
  return file && fs.existsSync(file) ? platform.displayPath(file) : undefined;
};

const hasStore = (): boolean => !!projectDir && fs.existsSync(path.join(projectDir, STORE_FILE));

const view = (): MovieMakerView => ({
  sessionId: boundSession,
  install: getInstaller().view(),
  project: projectDir,
  hasStore: hasStore(),
  link: { status: linkStatus, message: linkMessage, port: client?.port, version, undoDepth },
  log,
  logFile: logFileOf(boundSession),
});

let viewTimer: NodeJS.Timeout | undefined;
const sendView = (win: Win, body: string): void => {
  if (!win.res.writableEnded) win.res.write(`event: view\ndata: ${body}\n\n`);
};

function broadcast(): void {
  if (viewTimer || !windows.size) return;
  viewTimer = setTimeout(() => {
    viewTimer = undefined;
    const body = JSON.stringify(view());
    for (const win of windows) sendView(win, body);
  }, VIEW_THROTTLE_MS);
}

/** ログを、つないだセッションのその日のファイルに書き足す。新しく作ったファイルは、そのセッションの資料に足す。 */
const writeLog = (item: MmLogEntry): void => {
  if (!boundSession) return;
  let created: string[];
  try {
    created = appendLog(LOG_DIR, boundSession, [item], LOG_TITLE);
  } catch (e) {
    console.error(`[moviemaker] ログのファイルに書けません: ${(e as Error).message}`);
    return;
  }
  const meta = state.sessions[boundSession];
  const fresh = created.filter((file) => meta && !meta.files?.some((f) => f.path === file) && !meta.hiddenDocs?.includes(file));
  if (!meta || !fresh.length) return;
  meta.files = [...(meta.files ?? []), ...fresh.map((file) => ({ path: file, at: Date.now() }))];
  touch();
};

const bind = (sessionId: string | undefined): void => {
  if (sessionId === boundSession) return;
  boundSession = sessionId;
  // つないだときより前の変更は、そのセッションに届けない。
  if (sessionId && !cursors.has(sessionId)) cursors.set(sessionId, seq);
  broadcast();
};

// ─── movie-maker とのつながり ─────────────────────────────────────────────

const setLink = (status: MmLinkStatus, message?: string): void => {
  linkStatus = status;
  linkMessage = message;
  broadcast();
};

const unlink = (): void => {
  linkAbort?.abort();
  linkAbort = undefined;
  client = undefined;
  tools = undefined;
  version = undefined;
  undoDepth = undefined;
};

/** ログに足し、ファイルにも書く。届いた順に並べる。 */
const addLog = (entry: MmLogEntry): void => {
  log = [...log, entry].slice(-VIEW_LOG);
  writeLog(entry);
};

const onChange = (change: MmChange): void => {
  version = change.version;
  const record = { seq: ++seq, at: Date.now(), change };
  records = [...records, record].slice(-MAX_RECORDS);
  // mcp の変更は mm_tool の呼び出しの 1 件としてログに書くので、ここでは人間と mm の CLI の変更だけを書く。
  if (isHuman(change)) addLog(youEntry(record));
  else if (change.actor !== 'mcp') addLog({ id: newId(), at: record.at, who: 'agent', tool: `mm ${change.kind}`, args: { name: change.name }, ok: true, output: changeLine(change) });
  broadcast();
};

/** mm-serve の GET /events を購読する。流れが切れたら、lock.info を見直してつなぎ直すか、待つ。 */
const subscribe = (c: MmClient, abort: AbortController): void => {
  c.events((event, data) => {
    if (event === 'change') onChange(data as MmChange);
    if (event === 'hello' || event === 'resync') {
      version = (data as { version?: number }).version;
      broadcast();
    }
  }, abort.signal)
    .catch(() => undefined)
    .finally(() => {
      if (abort.signal.aborted || client !== c) return;
      unlink();
      tryLink();
    });
};

/** 動いている movie-maker を探してつなぐ。lock.info が変わるたびに呼ぶ。 */
function tryLink(): void {
  if (!windows.size) return;
  if (!projectDir) return setLink('no-project', 'プロジェクトのフォルダーを選んでください。');
  const found = findLink(projectDir, platform.movieMakerTokenDir(process.env));
  if ('reason' in found) {
    if (client) unlink();
    return setLink('waiting', found.reason);
  }
  if (client?.port === found.port) return;
  unlink();
  const c = new MmClient(found.port, found.token);
  const abort = new AbortController();
  client = c;
  linkAbort = abort;
  c.project(abort.signal)
    .then((p) => {
      if (client !== c) return;
      version = p.version;
      undoDepth = p.undo_depth;
      setLink('connected');
      subscribe(c, abort);
    })
    .catch((e: Error) => {
      if (client !== c) return;
      unlink();
      setLink('waiting', `movie-maker につなげません: ${e.message}`);
    });
}

const unwatch = (): void => {
  watchers.forEach((w) => w.close());
  watchers = [];
};

/** プロジェクトのフォルダーと .mm を見張り、lock.info が変わったらつなぎ直す。時間で読み直さない。 */
const watch = (): void => {
  unwatch();
  if (!projectDir) return;
  for (const dir of [projectDir, path.join(projectDir, MM_DIR)]) {
    try {
      const w = fs.watch(dir, () => {
        // .mm を後から作ったときは、その中も見張る。
        if (watchers.length < 2 && fs.existsSync(path.join(projectDir ?? '', MM_DIR))) watch();
        tryLink();
      });
      w.on('error', () => undefined);
      watchers.push(w);
    } catch {
      // フォルダーがまだ無い。
    }
  }
};

const start = (): void => {
  watch();
  tryLink();
};

const stop = (): void => {
  unwatch();
  unlink();
  setLink(projectDir ? 'waiting' : 'no-project');
};

/** ウィンドウからの接続。最初のウィンドウがつないだら movie-maker を探し、最後のウィンドウが切れたら離れる。 */
export const streamMovieMaker = (res: ServerResponse, sessionId: string | undefined, project: string | undefined): void => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  const win: Win = { res, sessionId };
  windows.add(win);
  if (sessionId) bind(sessionId);
  if (project && !projectDir) projectDir = path.resolve(project);
  if (windows.size === 1) start();
  sendView(win, JSON.stringify(view()));
  const keepalive = setInterval(() => res.write(': keepalive\n\n'), KEEPALIVE_MS);
  res.on('close', () => {
    clearInterval(keepalive);
    windows.delete(win);
    if (!windows.size) stop();
  });
};

export const setMmSession = (sessionId: string | undefined): void => bind(sessionId);

export const setMmProject = (project: string | undefined): void => {
  const next = project?.trim() ? path.resolve(project.trim()) : undefined;
  if (next === projectDir) return;
  projectDir = next;
  unlink();
  if (windows.size) start();
  broadcast();
};

export const installMovieMaker = async (): Promise<{ message: string }> => {
  void getInstaller().install();
  return { message: '入れ始めました。進み具合はウィンドウに出ます。' };
};

const runCli = (cli: string, args: string[]): Promise<string> =>
  new Promise((resolve, reject) => {
    const child = spawn(cli, args, { windowsHide: true });
    let out = '';
    child.stdout.on('data', (c: Buffer) => (out += c.toString()));
    child.stderr.on('data', (c: Buffer) => (out += c.toString()));
    child.on('error', reject);
    child.on('close', (code) => (code === 0 ? resolve(out) : reject(new Error(`mm ${args.join(' ')} が失敗しました: ${out.trim()}`))));
  });

/** movie-maker を起動する。project.mmdb が無いフォルダーは mm new で作ってから開く。 */
export const launchMovieMaker = async (): Promise<{ message: string }> => {
  if (!projectDir) return { message: 'プロジェクトのフォルダーを選んでください。' };
  const exe = installedApp(installPaths(DATA_DIR).bin);
  if (!exe) return { message: 'movie-maker を入れてください。' };
  if (linkStatus === 'connected') return { message: 'movie-maker はこのプロジェクトを開いています。' };
  const created = !hasStore();
  if (created) await runCli(exe.cli, ['new', projectDir]);
  const child = spawn(exe.app, [projectDir], { cwd: path.dirname(exe.app), detached: true, stdio: 'ignore' });
  child.on('error', (e) => setLink('waiting', `movie-maker を起動できません: ${e.message}`));
  child.unref();
  watch();
  broadcast();
  return { message: created ? `プロジェクトを作り、movie-maker を起動しました: ${projectDir}` : 'movie-maker を起動しました。' };
};

// ─── ツール ────────────────────────────────────────────────────────────

export type MmToolResult = ToolResult;

const text = (t: string, isError = false): MmToolResult => ({ content: [{ type: 'text', text: t }], ...(isError ? { isError } : {}) });

const refusal = (sessionId: string): string | undefined => {
  if (!windows.size) return 'ムービーメーカーが動いていません。人間にダッシュボードでムービーメーカーのウィンドウを開いてもらってください。ウィンドウを開いている間だけ、movie-maker とつながります。';
  if (!boundSession) return 'ムービーメーカーのウィンドウで、つなぐセッションが選ばれていません。人間に、ウィンドウでこのセッションを選んでもらってください。';
  if (boundSession !== sessionId) return `ムービーメーカーは、別のセッション「${sessionName(boundSession)}」につないでいます。人間にムービーメーカーのウィンドウでこのセッションに切り替えてもらってください。`;
  if (!client) return `movie-maker とつながっていません。${linkMessage ?? ''}`.trim();
  return undefined;
};

const mmState = async (c: MmClient): Promise<MmToolResult> => {
  const p = await c.project();
  version = p.version;
  undoDepth = p.undo_depth;
  broadcast();
  return text([`プロジェクト: ${projectDir ?? '?'}`, projectSummary(p), '値を変えるときは mm_tool を使う。tool を省くとツールの一覧が返る。'].join('\n'));
};

const clipOutput = (r: MmToolResult): string =>
  r.content
    .map((c) => (c.type === 'text' ? (c.text ?? '') : `[${c.type} ${c.mimeType ?? ''}]`))
    .join('\n')
    .slice(0, OUTPUT_CHARS);

const mmTool = async (c: MmClient, args: { tool?: unknown; arguments?: unknown }): Promise<MmToolResult> => {
  tools ??= await c.tools();
  const name = typeof args.tool === 'string' ? args.tool.trim() : '';
  if (!name) return text([`movie-maker のツール（${tools.length} 個）。arguments を省くと、そのツールの引数のスキーマを返す。`, ...tools.map((t) => `${t.name}: ${(t.description ?? '').split('\n')[0]}`)].join('\n'));
  const info = tools.find((t) => t.name === name);
  if (!info) return text(`movie-maker に ${name} というツールはありません。tool を省くと一覧が返ります。`, true);
  const hasParams = Object.keys(info.inputSchema?.properties ?? {}).length > 0;
  if (args.arguments === undefined && hasParams) return text(JSON.stringify(info.inputSchema, null, 2));
  const toolArgs = (args.arguments && typeof args.arguments === 'object' ? args.arguments : {}) as Record<string, unknown>;
  const result = await c.callTool(name, toolArgs);
  const entry = { id: newId(), at: Date.now(), who: 'agent' as const, tool: name, args: toolArgs, ok: !result.isError, output: clipOutput(result) };
  addLog(entry);
  broadcast();
  return result;
};

const mmChanges = (sessionId: string, all: boolean): MmToolResult => {
  const since = all ? 0 : (cursors.get(sessionId) ?? seq);
  cursors.set(sessionId, seq);
  const list = records.filter((r) => r.seq > since).slice(all ? -CHANGES_ALL : undefined);
  if (!list.length) return text('前に読んでから、変わったものはありません。');
  return text([`${CHANGE_FORMAT}。`, ...list.map((r) => `${isHuman(r.change) ? 'human' : 'agent'} ${changeLine(r.change)}`)].join('\n'));
};

export const MM_TOOL_NAMES = new Set(['mm_state', 'mm_tool', 'mm_changes']);

export const callMmTool = async (name: string, sessionId: string, args: Record<string, unknown>): Promise<MmToolResult> => {
  const why = refusal(sessionId);
  if (why || !client) return text(why ?? 'movie-maker とつながっていません。', true);
  try {
    switch (name) {
      case 'mm_state':
        return await mmState(client);
      case 'mm_tool':
        return await mmTool(client, args);
      case 'mm_changes':
        return mmChanges(sessionId, args.all === true);
      default:
        return text(`知らないツールです: ${name}`, true);
    }
  } catch (e) {
    return text(`ムービーメーカーで失敗しました: ${(e as Error).message}`, true);
  }
};

/**
 * フックの応答に入れる、前に届けてから人間が movie-maker でした操作。つないだセッションにだけ、ウィンドウが開いている間だけ返す。
 * エージェント自身の変更は、ツールの結果で知っているので入れない。
 */
export const mmHookContext = (sessionId: string): string | undefined => {
  if (!windows.size || boundSession !== sessionId) return undefined;
  const since = cursors.get(sessionId) ?? seq;
  cursors.set(sessionId, seq);
  return hookText(
    records.filter((r) => r.seq > since && isHuman(r.change)).map((r) => r.change),
    HOOK_LIMIT,
  );
};
