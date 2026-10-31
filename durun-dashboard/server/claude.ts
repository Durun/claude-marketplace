// Claude Code がローカルに残すファイルを読む。実行中セッション、会話ログ、TaskCreate のタスク、statusline の入力。
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Artifact, AskedQuestion, BgTask, ChatEntry, RateLimit, RateLimits, TaskItem, TokenTotals } from '../shared/types.ts';
import { DATA_DIR, type RateWindow, state } from './store.ts';

export const CLAUDE_DIR = process.env.CLAUDE_CONFIG_DIR ?? path.join(os.homedir(), '.claude');
export const SESSIONS_DIR = path.join(CLAUDE_DIR, 'sessions');
export const PROJECTS_DIR = path.join(CLAUDE_DIR, 'projects');
export const TASKS_DIR = path.join(CLAUDE_DIR, 'tasks');
export const STATUSLINE_DIR = path.join(DATA_DIR, 'statusline');

const EDIT_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);
// 補完の候補に添える説明。一覧で見分けられれば足りるので、書き出しだけを持つ。
const SKILL_DESCRIPTION_CHARS = 100;
// 要約はすべてのセッションぶんメモリに持つので、ツールの入力は先頭だけ残す。
const TOOL_DETAIL_CHARS = 2000;
const TOOL_HINT_CHARS = 120;
// ツールの入力のうち、何をしたかが一目で分かる項目。先に見つかったものを使う。
const TOOL_HINT_KEYS = ['description', 'command', 'file_path', 'notebook_path', 'pattern', 'url', 'query', 'skill', 'prompt', 'question'];
const TOTAL_KEYS = ['input', 'output', 'cacheRead', 'cacheCreation'] as const;

export const readJson = <T>(file: string): Partial<T> | undefined => {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8')) as Partial<T>;
  } catch {
    // 書き込み途中のファイルや消えたファイルは、次の変更通知で読み直す。
    return undefined;
  }
};

const mtimeOf = (file: string): number | undefined => {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return undefined;
  }
};

const jsonCache = new Map<string, { mtimeMs: number; size: number; value: unknown }>();
// 更新時刻の刻みが粗いファイルシステムでは、続けて書き直しても更新時刻が変わらないことがある。書かれたばかりのファイルは覚えず、次も読み直す。
const SETTLED_MS = 2000;

/**
 * スナップショットを配るたびに、すべてのセッションの statusline やタスクなどのファイルを読み直す。
 * ファイルを開くのは stat より桁違いに遅いことがあるので、更新時刻と大きさが変わっていなければ前に読んだ値を返す。返した値は共有するので書き換えない。
 */
const readJsonCached = <T>(file: string): Partial<T> | undefined => {
  let stat: fs.Stats;
  try {
    stat = fs.statSync(file);
  } catch {
    jsonCache.delete(file);
    return undefined;
  }
  const hit = jsonCache.get(file);
  if (hit?.mtimeMs === stat.mtimeMs && hit.size === stat.size) return hit.value as Partial<T>;
  const value = readJson<T>(file);
  if (value && Date.now() - stat.mtimeMs > SETTLED_MS) jsonCache.set(file, { mtimeMs: stat.mtimeMs, size: stat.size, value });
  else jsonCache.delete(file);
  return value;
};

export const listDir = (dir: string): string[] => {
  try {
    return fs.readdirSync(dir);
  } catch {
    return [];
  }
};

/** 入れてあるプラグインの場所。id は「プラグイン名@マーケットプレイス名」。入っていなければ undefined。 */
export const pluginDir = (id: string): string | undefined =>
  readJson<{ plugins: Record<string, { installPath?: string }[]> }>(path.join(CLAUDE_DIR, 'plugins', 'installed_plugins.json'))?.plugins?.[id]?.[0]?.installPath;

export type LiveSession = {
  pid: number;
  sessionId: string;
  cwd: string;
  kind?: string;
  /** cli は人が起動したセッション、sdk-cli は claude -p や Agent SDK からの実行 */
  entrypoint?: string;
  name?: string;
  status: 'busy' | 'idle';
  bridgeSessionId?: string;
  /** 外からのメッセージを受ける Unix ソケット。他のセッションやスクリプトが送ると、Claude が読む。 */
  messagingSocketPath?: string;
  jobId?: string;
  startedAt?: number;
  updatedAt?: number;
};

export const isAlive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
};

// `claude agents --json` は Remote Control の id を返さないため、同じ情報源であるセッションファイルを直接読む。
export const readLiveSessions = (): Map<string, LiveSession> => {
  const live = new Map<string, LiveSession>();
  for (const f of listDir(SESSIONS_DIR).filter((name) => name.endsWith('.json'))) {
    const raw = readJsonCached<LiveSession & { status: string }>(path.join(SESSIONS_DIR, f));
    if (!raw?.sessionId || !raw.pid || !raw.cwd || !isAlive(raw.pid)) continue;
    const prev = live.get(raw.sessionId);
    if (prev && (prev.updatedAt ?? 0) > (raw.updatedAt ?? 0)) continue;
    live.set(raw.sessionId, { ...raw, pid: raw.pid, sessionId: raw.sessionId, cwd: raw.cwd, status: raw.status === 'busy' ? 'busy' : 'idle' });
  }
  return live;
};

export type LogSummary = {
  offset: number;
  title?: string;
  cwd?: string;
  branch?: string;
  /** これまでに居たブランチ。最後に居た順に並べるので、今のブランチが末尾になる。 */
  branches: Set<string>;
  model?: string;
  contextTokens: number;
  lastUsageId?: string;
  lastUsage?: TokenTotals;
  lastUsageAt?: number;
  totals: TokenTotals;
  costUsd?: number;
  lastPrompt?: string;
  lastMessage?: string;
  lastMessageAt?: number;
  /** 最後の assistant エントリが呼んだツール。文章だけで終わっていれば undefined。 */
  lastToolUse?: string;
  /** 最後に AskUserQuestion で尋ねた質問と、尋ねた時刻 */
  lastQuestions?: AskedQuestion[];
  lastQuestionsAt?: number;
  lastActivityAt?: number;
  /** 本体のターンを終えた時刻。次の依頼や発言が載ったら消す。 */
  turnEndedAt?: number;
  /** 使用制限で断られて止まったときの、止まった時刻とリセットの時刻（epoch ミリ秒）。続きのやり取りが載ったら消す。 */
  limit?: UsageLimit;
  /** スリープや通信の途切れで API に届かず止まった時刻。続きのやり取りが載ったら消す。 */
  offlineAt?: number;
  /** Claude のログインが切れて API に断られ、止まった時刻。続きのやり取りが載ったら消す。 */
  loginExpiredAt?: number;
  timeline: [number, number][];
  editedFiles: Set<string>;
  /** 人間の依頼と Claude の発言。会話の表示と、過去のセッションの検索に使う。 */
  texts: ChatEntry[];
  /** 依頼の親のエントリの uuid と、その依頼を積む前の texts の数と最後の応答。巻き戻して送った依頼から、捨てられた分岐の発言を外すのに使う。 */
  checkpoints: Map<string, LastReply & { texts: number }>;
  /** Claude Code がこのセッションに見せた skills の名前と説明 */
  skills: Map<string, string>;
  /** バックグラウンドで動かした shell と Monitor。キーはタスクの id。 */
  bgTasks: Map<string, BgTaskLog>;
  /** 結果が届くまでの、バックグラウンドのツール呼び出し。キーは tool_use の id。 */
  pendingBg: Map<string, Pick<BgTaskLog, 'kind' | 'description' | 'command' | 'startedAt'>>;
  /** claude.ai に公開した artifact。キーは URL で、同じ URL に公開し直したら新しいほうを残す。 */
  artifacts: Map<string, Artifact>;
};

/** Claude の最後の応答から拾う値。 */
type LastReply = Pick<LogSummary, 'lastMessage' | 'lastMessageAt' | 'lastToolUse' | 'lastQuestions' | 'lastQuestionsAt'>;

/** ファイルに残した要約では、undefined の値がキーごと消える。戻すときに消えたキーも上書きするよう、キーをそろえて返す。 */
const lastReplyOf = ({ lastMessage, lastMessageAt, lastToolUse, lastQuestions, lastQuestionsAt }: LastReply): LastReply => ({ lastMessage, lastMessageAt, lastToolUse, lastQuestions, lastQuestionsAt });

export type BgTaskLog = {
  id: string;
  kind: BgTask['kind'];
  description: string;
  command: string;
  startedAt?: number;
  status: BgTask['status'];
  summary?: string;
  /** shell はツールの結果、Monitor は最初の通知で分かる。 */
  outputFile?: string;
};

export const ASK_TOOL = 'AskUserQuestion';

type RawQuestion = { question?: unknown; header?: unknown; multiSelect?: unknown; options?: { label?: unknown; description?: unknown }[] };
type ContentBlock = { type?: string; id?: string; tool_use_id?: string; content?: unknown; text?: string; name?: string; input?: { file_path?: unknown; notebook_path?: unknown; questions?: RawQuestion[]; [key: string]: unknown } };
type RawUsage = { input_tokens?: number; output_tokens?: number; cache_read_input_tokens?: number; cache_creation_input_tokens?: number };
type LogEntry = {
  type?: string;
  /** system エントリの種類。turn_duration は、ターンを終えたときに書かれる。 */
  subtype?: string;
  parentUuid?: string | null;
  isSidechain?: boolean;
  origin?: { kind?: string };
  /** claude -p で渡した依頼は sdk になり、origin を持たない。 */
  promptSource?: string;
  timestamp?: string;
  cwd?: string;
  gitBranch?: string;
  aiTitle?: string;
  lastPrompt?: string;
  totalCostUSD?: number;
  message?: { id?: string; model?: string; usage?: RawUsage; content?: ContentBlock[] | string };
  /** queued_command は、作業中に届いた依頼とタスク通知。誰から届いたかを origin に持つ。 */
  attachment?: SkillListing & { prompt?: string; origin?: { kind?: string } };
  /** shell は backgroundTaskId、Monitor は taskId に、動かしたタスクの id を持つ。 */
  toolUseResult?: { backgroundTaskId?: string; taskId?: string };
  /** frame-link エントリが持つ、公開した artifact の URL と題名と元のファイル。 */
  frameUrl?: string;
  /** 使用制限で断られたときに、Claude Code が合成した発言に添える理由と、リセットの時刻（epoch 秒）。 */
  apiError?: string;
  apiErrorParams?: { rate_limit_info?: { resetsAt?: number } };
  /** API のエラーで止まったときに、Claude Code が合成した発言に付ける。 */
  isApiErrorMessage?: boolean;
  /** 合成した発言に添える、エラーの種類。 */
  error?: string;
  title?: string;
  path?: string;
};

/** content は 1 行に 1 つの「- 名前: 説明」。説明が複数行にわたることもある。 */
type SkillListing = { type?: string; names?: string[]; content?: string };

const emptySummary = (): LogSummary => ({
  offset: 0,
  branches: new Set(),
  contextTokens: 0,
  totals: { input: 0, output: 0, cacheRead: 0, cacheCreation: 0 },
  timeline: [],
  editedFiles: new Set(),
  texts: [],
  checkpoints: new Map(),
  skills: new Map(),
  bgTasks: new Map(),
  pendingBg: new Map(),
  artifacts: new Map(),
});

// 同じ API 応答はブロックごとに連続したエントリとして書かれ、同じ usage を持つ。直前と同じ id なら差し替えて二重計上を避ける。
const applyUsage = (s: LogSummary, id: string, u: RawUsage, ts: number | undefined): void => {
  const next: TokenTotals = {
    input: u.input_tokens ?? 0,
    output: u.output_tokens ?? 0,
    cacheRead: u.cache_read_input_tokens ?? 0,
    cacheCreation: u.cache_creation_input_tokens ?? 0,
  };
  const replacing = s.lastUsageId === id;
  const prev = replacing ? s.lastUsage : undefined;
  for (const k of TOTAL_KEYS) s.totals[k] += next[k] - (prev?.[k] ?? 0);
  s.lastUsage = next;
  s.lastUsageId = id;
  s.contextTokens = next.input + next.cacheRead + next.cacheCreation;
  s.lastUsageAt = ts ?? s.lastUsageAt;
  const point: [number, number] = [s.lastUsageAt ?? 0, s.contextTokens];
  if (replacing && s.timeline.length) s.timeline[s.timeline.length - 1] = point;
  else s.timeline.push(point);
};

// ツールの入力はモデルが書いたものなので、形の合わない質問や選択肢は外す。
export const toQuestions = (raw: unknown): AskedQuestion[] =>
  (Array.isArray(raw) ? (raw as RawQuestion[]) : []).flatMap((q) => {
    if (typeof q?.question !== 'string') return [];
    const options = (Array.isArray(q.options) ? q.options : []).flatMap((o) => (typeof o?.label === 'string' ? [{ label: o.label, description: typeof o.description === 'string' ? o.description : '' }] : []));
    return [{
      question: q.question,
      header: typeof q.header === 'string' ? q.header : undefined,
      options: options.map((o) => o.label),
      descriptions: options.map((o) => o.description),
      multiSelect: q.multiSelect === true,
    }];
  });

/**
 * 要対応に出す質問の本文と選択肢。質問が 1 つで、選択肢から 1 つを選ぶときは、選択肢をボタンにする。
 * ほかは質問をタブに分けて出す。本文には、端末のセッションの画面と通知に出すために、質問ごとに選択肢を並べる。
 */
export const questionDialog = (questions: AskedQuestion[]): { body: string; options?: string[]; descriptions?: string[]; questions?: AskedQuestion[] } | undefined => {
  const [first] = questions;
  if (!first) return undefined;
  if (questions.length === 1 && !first.multiSelect) return { body: first.question, options: first.options, descriptions: first.descriptions };
  return { body: questions.map((q) => [q.question, ...q.options.map((o) => `- ${o}`)].join('\n')).join('\n\n'), questions };
};

const toolEntry = (b: ContentBlock, at: number | undefined): ChatEntry => {
  const input = b.input ?? {};
  const hint = TOOL_HINT_KEYS.map((k) => input[k]).find((v) => typeof v === 'string');
  const line = typeof hint === 'string' ? hint.split('\n')[0]?.slice(0, TOOL_HINT_CHARS) : '';
  return { kind: 'tool', text: `${b.name ?? 'tool'}${line ? ` ${line}` : ''}`, at, detail: JSON.stringify(input, null, 2).slice(0, TOOL_DETAIL_CHARS) };
};

const rememberBg = (s: LogSummary, b: ContentBlock, ts: number | undefined): void => {
  const input = b.input ?? {};
  const kind = b.name === 'Monitor' ? 'monitor' : b.name === 'Bash' && input.run_in_background === true ? 'shell' : undefined;
  if (!kind || !b.id) return;
  const str = (v: unknown): string => (typeof v === 'string' ? v : '');
  s.pendingBg.set(b.id, { kind, description: str(input.description), command: str(input.command), startedAt: ts });
};

/** ツールの結果に載るタスクの id を、呼び出したときの入力と結びつける。 */
const applyBgResult = (s: LogSummary, e: LogEntry): void => {
  const id = e.toolUseResult?.backgroundTaskId ?? e.toolUseResult?.taskId;
  const content = e.message?.content;
  if (!id || !Array.isArray(content)) return;
  for (const b of content) {
    const call = b.type === 'tool_result' && b.tool_use_id ? s.pendingBg.get(b.tool_use_id) : undefined;
    if (!call || !b.tool_use_id) continue;
    s.pendingBg.delete(b.tool_use_id);
    const text = typeof b.content === 'string' ? b.content : JSON.stringify(b.content);
    s.bgTasks.set(id, { id, ...call, status: 'running', outputFile: /written to: (\S+?\.output)/.exec(text)?.[1] });
  }
};

const BG_STATUSES = new Set<string>(['completed', 'failed', 'killed', 'stopped']);

/** 終わったときと Monitor のイベントごとに届く通知。イベントの通知には status が無い。 */
const applyNotifications = (s: LogSummary, text: string): void => {
  for (const [, body = ''] of text.matchAll(/<task-notification>([\s\S]*?)<\/task-notification>/g)) {
    const tag = (name: string): string | undefined => new RegExp(`<${name}>([\\s\\S]*?)</${name}>`).exec(body)?.[1];
    const t = s.bgTasks.get(tag('task-id') ?? '');
    if (!t) continue;
    const status = tag('status');
    if (status && BG_STATUSES.has(status)) {
      t.status = status as BgTask['status'];
      t.summary = tag('summary');
    }
    t.outputFile ??= tag('output-file');
  }
};

const applyAssistant = (s: LogSummary, e: LogEntry, ts: number | undefined): void => {
  const m = e.message;
  if (!m) return;
  if (m.model && m.model !== '<synthetic>') s.model = m.model;
  if (m.id && m.usage) applyUsage(s, m.id, m.usage, ts);
  const blocks = Array.isArray(m.content) ? m.content : [];
  // 思考だけのエントリは次の行動の前触れなので、作業中かどうかの判定を変えない。
  const tool = blocks.findLast((b) => b.type === 'tool_use');
  if (tool) s.lastToolUse = tool.name;
  else if (blocks.some((b) => b.type === 'text' && b.text?.trim())) s.lastToolUse = undefined;
  if (tool?.name === ASK_TOOL) Object.assign(s, { lastQuestions: toQuestions(tool.input?.questions), lastQuestionsAt: ts });
  for (const b of blocks) {
    if (b.type === 'text' && b.text?.trim()) {
      s.lastMessage = b.text;
      s.lastMessageAt = ts;
      s.texts.push({ kind: 'message', text: b.text, at: ts });
    }
    if (b.type !== 'tool_use') continue;
    s.texts.push(toolEntry(b, ts));
    rememberBg(s, b, ts);
    const file = b.input?.file_path ?? b.input?.notebook_path;
    if (EDIT_TOOLS.has(b.name ?? '') && typeof file === 'string') s.editedFiles.add(file);
  }
};

// ダッシュボードからの依頼は、フックの結果や別のセッションからのメッセージとして届き、origin が human にならない。依頼の文に付く印で見分ける。
const DASHBOARD_MARK = '[durun-dashboard]';
// 受信ソケットで送る依頼に付けて、人間が送ったことを Claude に伝える前置き。チャットには出さない。
export const HUMAN_LABEL = `${DASHBOARD_MARK} 人間から:\n`;
// Claude Code が、別のセッションから届いたメッセージの後ろに付ける注意書きの書き出し。
const PEER_NOTE = '\n\nThis came from another Claude session';

const withoutLabel = (text?: string): string | undefined => (text?.startsWith(HUMAN_LABEL) ? text.slice(HUMAN_LABEL.length) : text);

const tagText = (text: string, tag: string): string | undefined => text.split(`<${tag}>`)[1]?.split(`</${tag}>`)[0]?.trim();

/** スラッシュコマンドの依頼は <command-name> などのタグで包まれて記録されるので、人間が打ったとおりの「/名前 引数」に戻す。 */
const asTyped = (text: string): string => {
  const name = text.trimStart().startsWith('<command-') ? tagText(text, 'command-name') : undefined;
  if (!name) return text;
  const args = tagText(text, 'command-args');
  return args ? `${name} ${args}` : name;
};

/**
 * 人間の依頼の文。人が打ったものと、claude -p で渡したものと、ダッシュボードから届いたものに限る。タスク通知やスキルの読み込み、ツールの結果は依頼ではない。
 * どの依頼からも HUMAN_LABEL を外す。ダッシュボードから届いた依頼は、前に付くフックや別のセッションの前置きと、後ろの閉じタグや注意書きも外す。
 */
const requestText = (e: LogEntry): string | undefined => {
  const content = e.message?.content;
  const text = typeof content === 'string' ? content : (content ?? []).map((b) => (b.type === 'text' ? (b.text ?? '') : '')).join('\n');
  if (!text.trim()) return undefined;
  if (e.origin?.kind === 'human' || e.promptSource === 'sdk') return withoutLabel(asTyped(text));
  const at = text.indexOf(DASHBOARD_MARK);
  return at < 0 ? undefined : withoutLabel(text.slice(at).split('</agent-message>')[0]?.split(PEER_NOTE)[0]?.trim());
};

// last-prompt エントリを書かない起動経路もあるので、依頼の user エントリからも拾う。
const applyRequest = (s: LogSummary, e: LogEntry, ts: number | undefined): void => {
  const text = requestText(e);
  if (!text) return;
  // 巻き戻して送った依頼は、書き直す前の依頼と同じ親につながる。そこより後の発言は、捨てられた分岐のもの。
  if (e.parentUuid) {
    const kept = s.checkpoints.get(e.parentUuid);
    if (!kept) s.checkpoints.set(e.parentUuid, { texts: s.texts.length, ...lastReplyOf(s) });
    else {
      s.texts.length = Math.min(kept.texts, s.texts.length);
      Object.assign(s, lastReplyOf(kept));
    }
  }
  s.lastPrompt = text;
  s.texts.push({ kind: 'prompt', text, at: ts, rewindTo: e.parentUuid ?? undefined });
};

/**
 * 一覧は、起動時の全体に続けて、増えた分や作り直した全体が届くので、名前ごとに足していく。
 * code-review と code-review:comment-review のように前方が重なる名前があるので、説明の行は「名前: 」まで合わせて探す。
 */
const applySkills = (s: LogSummary, listing: SkillListing): void => {
  const lines = (listing.content ?? '').split('\n');
  for (const name of listing.names ?? []) {
    const line = lines.find((l) => l.startsWith(`- ${name}: `)) ?? '';
    s.skills.set(name, line.slice(name.length + 4).trim().slice(0, SKILL_DESCRIPTION_CHARS));
  }
};

/** skills の一覧を借りる元。at は会話ログが最後に動いた時刻。 */
export type SkillSource = { cwd: string; skills: Map<string, string>; at: number };

/**
 * 新規セッションの欄や、まだ最初の依頼を送っていないセッションには、Claude Code がまだ一覧を見せていない。
 * そのときは同じディレクトリのセッションのうち最近動いたものから、無ければほかのディレクトリのものから借りる。
 */
export const borrowSkills = (cwd: string | undefined, sources: SkillSource[]): Map<string, string> => {
  const listed = sources.filter((s) => s.skills.size).sort((a, b) => b.at - a.at);
  const here = cwd && path.resolve(cwd);
  return (listed.find((s) => path.resolve(s.cwd) === here) ?? listed[0])?.skills ?? new Map();
};

export type UsageLimit = { at: number; resetsAt: number };

const limitOf = (e: LogEntry, ts: number | undefined): UsageLimit | undefined => {
  const resetsAt = e.apiErrorParams?.rate_limit_info?.resetsAt;
  return e.apiError === 'usage_limit_reached' && resetsAt && ts ? { at: ts, resetsAt: resetsAt * 1000 } : undefined;
};

// 応答の途中でスリープしたか接続が切れたとき、API に届かないとき、応答が来ないときに、Claude Code が合成する発言の書き出し。
const OFFLINE_ERROR = /^(API Error: (Your computer went to sleep|Connection (lost|error)|Can't reach the API server)|Request timed out)/;

const offlineOf = (e: LogEntry, ts: number | undefined): number | undefined => {
  const content = e.message?.content;
  const text = Array.isArray(content) ? content.find((b) => b.type === 'text')?.text : undefined;
  return e.isApiErrorMessage && text && OFFLINE_ERROR.test(text) ? ts : undefined;
};

// ログインが切れたときに、Claude Code が合成する発言（Login expired · Please run /login）に添えるエラーの種類。
const loginExpiredOf = (e: LogEntry, ts: number | undefined): number | undefined => (e.isApiErrorMessage && e.error === 'authentication_failed' ? ts : undefined);

const applyLine = (s: LogSummary, line: string, includeSidechain: boolean): void => {
  // API への要求の記録は、要約に使わないうえに会話ログの大きな割合を占めるので、JSON として読まずに飛ばす。
  if (!line || line.startsWith('{"type":"api-request')) return;
  let e: LogEntry;
  try {
    e = JSON.parse(line) as LogEntry;
  } catch {
    return;
  }
  if (e.isSidechain && !includeSidechain) return;
  const ts = e.timestamp ? Date.parse(e.timestamp) : undefined;
  if (e.type === 'ai-title') s.title = e.aiTitle;
  else if (e.type === 'last-prompt' && e.lastPrompt) s.lastPrompt = e.lastPrompt;
  else if (e.type === 'cost-state') s.costUsd = e.totalCostUSD;
  else if (e.type === 'frame-link' && e.frameUrl) s.artifacts.set(e.frameUrl, { url: e.frameUrl, title: e.title ?? '', path: e.path });
  else if (e.type === 'attachment' && e.attachment?.type === 'skill_listing') applySkills(s, e.attachment);
  else if (e.type === 'attachment' && e.attachment?.type === 'queued_command') {
    applyNotifications(s, e.attachment.prompt ?? '');
    // 作業中に届いた依頼は、user のエントリにならず、ターンの途中に差し込まれた記録だけが残る。ターンの途中へは巻き戻させないので、親を渡さない。
    applyRequest(s, { origin: e.attachment.origin, message: { content: e.attachment.prompt } }, ts);
  } else if (e.type === 'system' && e.subtype === 'turn_duration') s.turnEndedAt = ts;
  if (e.type !== 'user' && e.type !== 'assistant') return;
  s.turnEndedAt = undefined;
  if (e.cwd) s.cwd = e.cwd;
  if (e.gitBranch && e.gitBranch !== s.branch) {
    s.branches.delete(e.gitBranch);
    s.branches.add(e.gitBranch);
    s.branch = e.gitBranch;
  }
  if (ts) s.lastActivityAt = ts;
  s.limit = limitOf(e, ts);
  s.offlineAt = offlineOf(e, ts);
  s.loginExpiredAt = loginExpiredOf(e, ts);
  if (e.type === 'assistant') applyAssistant(s, e, ts);
  else {
    applyBgResult(s, e);
    const content = e.message?.content;
    applyNotifications(s, typeof content === 'string' ? content : (content ?? []).map((b) => (b.type === 'text' ? (b.text ?? '') : '')).join('\n'));
    applyRequest(s, e, ts);
  }
};

// 起動し直した直後に会話ログを頭から読み直さないよう、要約を前回の続きとしてファイルに残す。
const SUMMARIES_FILE = path.join(DATA_DIR, 'log-summaries.json');
// 要約の作り方が変わったら残した要約を使わないよう、このファイルの中身のハッシュを添える。
const CODE = createHash('sha1').update(fs.readFileSync(import.meta.filename)).digest('base64url');
// これより前に動きの無かった会話ログは一覧に出ず、読み直すことがまれなので、要約を残さない。
const KEEP_SUMMARY_MS = 14 * 24 * 60 * 60 * 1000;

/** JSON に Map と Set を書けるよう、印を付けた配列にする。 */
const tagged = (_: string, v: unknown): unknown => (v instanceof Map ? { $map: [...v] } : v instanceof Set ? { $set: [...v] } : v);
const untagged = (_: string, v: unknown): unknown => {
  const t = v as { $map?: [unknown, unknown][]; $set?: unknown[] } | null;
  return t?.$map ? new Map(t.$map) : t?.$set ? new Set(t.$set) : v;
};

const loadSummaries = (): Map<string, LogSummary> => {
  try {
    const saved = JSON.parse(fs.readFileSync(SUMMARIES_FILE, 'utf8'), untagged) as { code?: string; logs?: Record<string, LogSummary> };
    return new Map(saved.code === CODE ? Object.entries(saved.logs ?? {}) : []);
  } catch {
    // 無いか壊れていれば、会話ログを頭から読む。
    return new Map();
  }
};

const summaries = loadSummaries();

/** サーバーを止めるときに、会話ログの要約を書き出す。読みかけの例外で止まるときは、要約が途中の状態かもしれないので呼ばない。 */
export const saveLogSummaries = (): void => {
  const since = Date.now() - KEEP_SUMMARY_MS;
  const logs = Object.fromEntries([...summaries].filter(([, s]) => (s.lastActivityAt ?? 0) > since));
  fs.mkdirSync(DATA_DIR, { recursive: true });
  const tmp = `${SUMMARIES_FILE}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify({ code: CODE, logs }, tagged));
  fs.renameSync(tmp, SUMMARIES_FILE);
};

/**
 * 会話ログを前回の続きから読み、要約を更新して返す。
 * includeSidechain はサブエージェント専用ファイルを読むときに true にする。
 */
export const summarizeLog = (file: string, includeSidechain = false): LogSummary => {
  let s = summaries.get(file);
  let size: number;
  try {
    size = fs.statSync(file).size;
  } catch {
    return s ?? emptySummary();
  }
  if (!s || size < s.offset) {
    s = emptySummary();
    summaries.set(file, s);
  }
  if (size === s.offset) return s;
  // ponytail: 差分を一度に読む。数百 MB の会話ログが出てきたら分割して読む。
  const buf = Buffer.alloc(size - s.offset);
  const fd = fs.openSync(file, 'r');
  try {
    fs.readSync(fd, buf, 0, buf.length, s.offset);
  } finally {
    fs.closeSync(fd);
  }
  // 書きかけの最終行は次回に回す。
  const end = buf.lastIndexOf(0x0a);
  if (end < 0) return s;
  for (const line of buf.subarray(0, end).toString('utf8').split('\n')) applyLine(s, line, includeSidechain);
  s.offset += end + 1;
  return s;
};

export const findTranscript = (sessionId: string): string | undefined => {
  for (const dir of listDir(PROJECTS_DIR)) {
    const file = path.join(PROJECTS_DIR, dir, `${sessionId}.jsonl`);
    if (fs.existsSync(file)) return file;
  }
  return undefined;
};

/** ユーザーが /rename などで付けた名前 */
export const readCustomTitle = (transcript: string): string | undefined =>
  readJson<{ customTitle: string }>(path.join(transcript.replace(/\.jsonl$/, ''), 'custom-title.json'))?.customTitle;

export type SubagentLog = { id: string; type: string; description: string; model?: string; log: LogSummary };

export const readSubagents = (transcript: string): SubagentLog[] => {
  const dir = path.join(transcript.replace(/\.jsonl$/, ''), 'subagents');
  return listDir(dir)
    .filter((f) => f.endsWith('.jsonl'))
    .map((f) => {
      const meta = readJsonCached<{ agentType: string; description: string; model: string }>(path.join(dir, f.replace(/\.jsonl$/, '.meta.json')));
      return {
        id: f.replace(/^agent-/, '').replace(/\.jsonl$/, ''),
        type: meta?.agentType ?? 'agent',
        description: meta?.description ?? '',
        model: meta?.model,
        log: summarizeLog(path.join(dir, f), true),
      };
    });
};

const TASK_STATUSES = new Set(['pending', 'in_progress', 'completed']);

export const readTasks = (sessionId: string): TaskItem[] => {
  const dir = path.join(TASKS_DIR, sessionId);
  return listDir(dir)
    .filter((f) => f.endsWith('.json'))
    .map((f) => readJsonCached<TaskItem>(path.join(dir, f)))
    .filter((t): t is Partial<TaskItem> & { id: string; subject: string } => !!t?.id && !!t.subject && TASK_STATUSES.has(t.status ?? ''))
    .map((t) => ({ id: t.id, subject: t.subject, activeForm: t.activeForm, status: t.status ?? 'pending', blockedBy: t.blockedBy ?? [] }))
    .sort((a, b) => Number(a.id) - Number(b.id));
};

type RawLimit = { used_percentage?: number; resets_at?: number };
type RawStatusline = {
  context_window?: { used_percentage?: number; context_window_size?: number };
  rate_limits?: { five_hour?: RawLimit; seven_day?: RawLimit };
  model?: { id?: string };
  effort?: { level?: string };
};

export type StatuslineInfo = { mtime: number; usedPercent?: number; windowSize?: number; model?: string; effort?: string };

/** statusline-tee.sh が保存した statusline の入力。statusline を描かないバックグラウンドセッションには無い。 */
export const readStatusline = (sessionId: string): StatuslineInfo | undefined => {
  const file = path.join(STATUSLINE_DIR, `${sessionId}.json`);
  const mtime = mtimeOf(file);
  const raw = readJsonCached<RawStatusline>(file);
  if (!raw || mtime === undefined) return undefined;
  const cw = raw.context_window;
  return { mtime, usedPercent: cw?.used_percentage, windowSize: cw?.context_window_size, model: raw.model?.id, effort: raw.effort?.level };
};

const toLimit = (l: RawLimit | undefined): RateLimit | undefined =>
  l?.used_percentage == null ? undefined : { usedPercent: l.used_percentage, resetsAt: l.resets_at ? l.resets_at * 1000 : undefined };

// statusline の入力は、そのセッションが最後に API を呼んだ時点の値で、止まっているセッションでは古いまま残る。
// 同じ枠の中で使用率は増える一方なので、リセット時刻が最も遅く、その中で使用率が最も高い値を最新とみなす。
const RATE_LIMIT_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;

const newestLimit = (limits: (RawLimit | undefined)[], now: number): RateLimit | undefined => {
  const current = limits.filter((l): l is RawLimit => l?.used_percentage != null && (l.resets_at ?? Infinity) * 1000 > now);
  current.sort((a, b) => (b.resets_at ?? 0) - (a.resets_at ?? 0) || (b.used_percentage ?? 0) - (a.used_percentage ?? 0));
  return toLimit(current[0]);
};

/** 記録した推移は、同じ枠の値にだけ添える。リセットされた後に、前の枠の推移を見せない。 */
export const withPoints = (limit: RateLimit | undefined, recorded: RateWindow | undefined): RateLimit | undefined =>
  recorded && limit?.resetsAt === recorded.resetsAt ? { ...limit, points: recorded.points } : limit;

/** 記録した推移の最後の使用率。枠が終わっていれば無い。 */
const lastRecorded = (recorded: RateWindow | undefined, now: number): RateLimit | undefined =>
  recorded && recorded.resetsAt > now ? { usedPercent: recorded.points.at(-1)?.[1] ?? 0, resetsAt: recorded.resetsAt } : undefined;

/**
 * レート制限はアカウント単位なので、全セッションの statusline から最新の値を選ぶ。
 * 週の制限をリセットしても週の枠のリセット時刻は変わらず、止まったセッションには前の高い使用率が残る。
 * リセットでは 5 時間の枠が始まり直すので、週の使用率は、今の 5 時間の枠で API を呼んだセッションの値から選ぶ。
 * 5 時間の枠がどれも終わっていれば、その後に API を呼んだセッションは無いので、記録した最後の値を使う。
 * maxAge を渡すと、それより前に描いた statusline を候補から外す。
 */
export const readRateLimits = (maxAge = RATE_LIMIT_MAX_AGE_MS): RateLimits | undefined => {
  const now = Date.now();
  const all = listDir(STATUSLINE_DIR)
    .filter((f) => f.endsWith('.json') && now - (mtimeOf(path.join(STATUSLINE_DIR, f)) ?? 0) < maxAge)
    .map((f) => readJsonCached<RawStatusline>(path.join(STATUSLINE_DIR, f))?.rate_limits);
  const fiveHour = newestLimit(all.map((l) => l?.five_hour), now);
  const fiveHourEnd = fiveHour?.resetsAt;
  const sevenDay =
    fiveHourEnd === undefined
      ? lastRecorded(state.rateWindows.sevenDay, now)
      : newestLimit(all.filter((l) => (l?.five_hour?.resets_at ?? 0) * 1000 === fiveHourEnd).map((l) => l?.seven_day), now);
  return fiveHour || sevenDay
    ? { fiveHour: withPoints(fiveHour, state.rateWindows.fiveHour), sevenDay: withPoints(sevenDay, state.rateWindows.sevenDay), updatedAt: now }
    : undefined;
};
