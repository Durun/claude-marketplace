import type {
  AssistantMessage, AssistantReply, Bookmark, BranchView, ChatEntry, Delivery, DiffFile, DiffRange, DiffResult, Doc, DocEditResult, DocMeta, ExplorerDir, FeedbackContext, FeedbackTarget, FileTarget, Glossary, ImproveSettings, InboxSection, JiraDetail, JiraIssue, MemoryStatus, NotionPage, OutboxMessage, PrDetail, PrNotes, PrThread, ProofEvent, ProofFile, ProofFileMeta, ProofMode, ProofReading, RepoRef, ReviewComment, ReviseNote, SearchResponse, SessionSummary, SkillInfo, TextSaveResult,
} from '../shared/types.ts';
import { toast } from './toast.tsx';

export const DELIVERY_TOAST: Record<Delivery, string> = {
  mcp: 'Claude に届けました',
  sent: 'セッションに届けました',
  resumed: 'セッションを再開して届けました',
  queued: '保留しました。Claude が入力待ちになったら送ります',
};

const errorText = async (res: Response): Promise<string> => {
  const body = await res.text().catch(() => '');
  try {
    const parsed = JSON.parse(body) as { error?: string };
    if (parsed.error) return parsed.error;
  } catch {
    // JSON 以外の本文はそのまま見せる。
  }
  return body.slice(0, 120) || `HTTP ${res.status}`;
};

export const getJson = async <T,>(url: string): Promise<T> => {
  const res = await fetch(url);
  if (!res.ok) throw new Error(await errorText(res));
  return (await res.json()) as T;
};

const postJson = async <T,>(url: string, body: unknown = {}): Promise<T> => {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(await errorText(res));
  return (await res.json().catch(() => ({}))) as T;
};

/** URL の 1 区間をエンコードする。attention の id（doc:xxx）の ':' は、デコード前の URL で照合するサーバーでも一致するよう残す。 */
const seg = (value: string): string => encodeURIComponent(value).replaceAll('%3A', ':');

const session = (id: string): string => `/api/sessions/${seg(id)}`;

/** ファイルを読む API の場所。エクスプローラーからセッションの外で開いたファイルは id が空で、セッションを付けずに読む。 */
const files = (id: string): string => (id ? session(id) : '/api');

/** クエリに載せる真偽値。偽なら項目ごと外す。 */
const flag = (on?: boolean): string | undefined => (on ? '1' : undefined);

/** 値の無い項目を外して、クエリにする。 */
const query = (params: Record<string, string | undefined>): URLSearchParams =>
  new URLSearchParams(Object.entries(params).filter((e): e is [string, string] => e[1] !== undefined));

export const api = {
  /** Internet Explorer の中継が最後に描いたページ。 */
  lastFrame: () => getJson<{ seq: number; url: string; title: string }>('/api/ie/last-frame'),
  diff: (id: string, full: boolean, range?: DiffRange, view?: BranchView) =>
    getJson<DiffResult>(`${session(id)}/diff?${query({ full: full ? '1' : undefined, ...range, branch: view?.branch, base: view?.base })}`),
  file: (id: string, path: string, branch?: string) => getJson<DiffFile>(`${session(id)}/file?${query({ path, branch })}`),
  localFile: (id: string, path: string) => getJson<{ path: string; content: string; modifiedAt: number }>(`${files(id)}/local-file?${query({ path })}`),
  /** メモ帳で編集したファイルを上書きする。modifiedAt は読んだときの更新時刻で、その後にほかで書き換えられていたら、上書きせずに changed を返す。 */
  saveLocalFile: (path: string, content: string, modifiedAt: number) => postJson<TextSaveResult>(`/api/local-file?${query({ path })}`, { content, modifiedAt }),
  resolveFile: (id: string, path: string) => getJson<FileTarget>(`${files(id)}/resolve-file?${query({ path })}`),
  mediaUrl: (id: string, path: string) => `${files(id)}/media?${query({ path })}`,
  localFileEventsUrl: (id: string, path: string) => `${files(id)}/local-file/events?${query({ path })}`,
  explorer: (path?: string) => getJson<ExplorerDir>(`/api/explorer?${query({ path })}`),
  /** 端末の出力を流す SSE。cwd を省くと、サーバーのホームディレクトリで動かす。 */
  terminalEventsUrl: (cwd?: string) => `/api/terminal?${query({ cwd })}`,
  terminalInput: (id: string, data: string) => postJson(`/api/terminal/${seg(id)}/input`, { data }),
  resizeTerminal: (id: string, cols: number, rows: number) => postJson(`/api/terminal/${seg(id)}/resize`, { cols, rows }),
  chat: (id: string) => getJson<ChatEntry[]>(`${session(id)}/chat`),
  subagentChat: (id: string, agentId: string) => getJson<ChatEntry[]>(`${session(id)}/subagents/${seg(agentId)}/chat`),
  bgOutput: (id: string, taskId: string) => getJson<{ output?: string }>(`${session(id)}/background/${seg(taskId)}/output`),
  /** / の補完に出す skills。セッションが無ければ、cwd で動いたセッションのものを借りる。 */
  skills: ({ sessionId, cwd }: { sessionId?: string; cwd?: string }) =>
    getJson<SkillInfo[]>(sessionId ? `${session(sessionId)}/skills` : `/api/skills?${query({ cwd })}`),
  /** # の補完に出す PR と Issue。セッションが無ければ、cwd のリポジトリから探す。 */
  repoRefs: (target: { sessionId?: string; cwd?: string }, fresh?: boolean) => postJson<RepoRef[]>('/api/github/refs', { ...target, fresh }),
  // fresh は、人間が「更新」を押したときに付け、サーバーに覚えた値を使わず読み直させる。
  jiraIssue: (key: string, fresh?: boolean) => getJson<JiraDetail>(`/api/jira/issues/${seg(key)}?${query({ fresh: flag(fresh) })}`),
  notionPage: (id: string, fresh?: boolean) => getJson<NotionPage>(`/api/notion/pages/${seg(id)}?${query({ fresh: flag(fresh) })}`),
  notionTitle: (id: string) => getJson<{ title: string }>(`/api/notion/pages/${seg(id)}/title`),
  prDetail: (url: string, fresh?: boolean) => getJson<PrDetail>(`/api/pr?${query({ url, fresh: flag(fresh) })}`),
  /** pr は PR の URL。無ければ、今のブランチの PR。 */
  prThreads: (id: string, pr?: string, fresh?: boolean) => getJson<PrThread[]>(`${session(id)}/pr-threads?${query({ pr, fresh: flag(fresh) })}`),
  prViewed: (id: string, fresh?: boolean) => getJson<string[]>(`${session(id)}/pr-viewed?${query({ fresh: flag(fresh) })}`),
  doc: (id: string) => getJson<Doc>(`/api/docs/${seg(id)}`),
  search: (q: string) => getJson<SearchResponse>(`/api/search?q=${encodeURIComponent(q)}`),
  githubInbox: (fresh?: boolean) => getJson<InboxSection[]>(`/api/github/inbox?${query({ fresh: flag(fresh) })}`),
  jira: (jql: string, max: number) => getJson<JiraIssue[]>(`/api/jira?${new URLSearchParams({ jql, max: String(max) })}`),
  jiraRefs: (fresh?: boolean) => getJson<JiraIssue[]>(`/api/jira/refs?${query({ fresh: flag(fresh) })}`),
  memory: () => getJson<MemoryStatus | null>('/api/memory'),
  theme: (id: string, theme: string) => postJson(`${session(id)}/theme`, { theme }),
  renameTheme: (from: string, to: string) => postJson('/api/themes/rename', { from, to }),
  orderThemes: (themes: string[]) => postJson('/api/themes/order', { themes }),
  title: (id: string, title: string) => postJson(`${session(id)}/title`, { title }),
  suggestTitle: (id: string) => postJson<{ title: string }>(`${session(id)}/title-suggestion`),
  /** 作業中でアーカイブ待ちにしたら true を返す。Claude Code の一覧から消し終わるのは待たず、消せなかったときは useRemoveFailureToasts が知らせる。 */
  archive: async (id: string, archived: boolean): Promise<boolean> =>
    !!(await postJson<{ deferred?: boolean }>(`${session(id)}/archive`, { archived })).deferred,
  orderSessions: (ids: string[]) => postJson('/api/sessions/order', { ids }),
  pin: (id: string, pinned: boolean) => postJson(`${session(id)}/pin`, { pinned }),
  addLink: (id: string, url: string, title: string) => postJson(`${session(id)}/links`, { url, title }),
  addFile: (id: string, path: string) => postJson(`${session(id)}/files`, { path }),
  shareDoc: (id: string, doc: string) => postJson(`${session(id)}/docs`, { id: doc }),
  addProof: (id: string, proof: string) => postJson(`${session(id)}/proofs`, { id: proof }),
  addBadge: (id: string, url: string) => postJson(`${session(id)}/badges`, { url }),
  removeBadge: (id: string, url: string) => postJson(`${session(id)}/badges/remove`, { url }),
  orderDocs: (id: string, keys: string[]) => postJson(`${session(id)}/docs/order`, { keys }),
  hideDocs: (id: string, keys: string[]) => postJson<DocEditResult>(`${session(id)}/docs/hide`, { keys }),
  renameDoc: (id: string, key: string, title: string) => postJson<DocEditResult>(`${session(id)}/docs/rename`, { key, title }),
  pinDocs: (id: string, keys: string[], pinned: boolean) => postJson<DocEditResult>(`${session(id)}/docs/pin`, { keys, pinned }),
  /** 説明が空なら、サーバーが haiku にやり取りから書かせてから足すので、数秒待つ。 */
  addTerm: (id: string, term: string, description: string) => postJson(`${session(id)}/terms`, { term, description }),
  hideTerm: (id: string, term: string) => postJson(`${session(id)}/terms/hide`, { term }),
  /** on が真ならブックマークに足し、偽なら外す。 */
  bookmark: (id: string, { kind, at, text }: Pick<Bookmark, 'kind' | 'at' | 'text'>, on: boolean) => postJson(`${session(id)}/bookmarks`, { kind, at, text, on }),
  startSession: (cwd: string, prompt: string, model: string, effort: string, theme: string) => postJson('/api/sessions', { cwd, prompt, model, effort, theme }),
  message: (id: string, text: string) =>
    postJson<{ message: OutboxMessage; delivery: Delivery }>(`${session(id)}/message`, { text }),
  /** 「キュー」で保留する。今送れるセッションなら、すぐに送られて queued が false になる。 */
  queue: (id: string, text: string) => postJson<{ queued: boolean }>(`${session(id)}/queue`, { text }),
  rewind: (id: string, rewindTo: string, text: string) =>
    postJson<{ message: OutboxMessage; delivery: Delivery }>(`${session(id)}/rewind`, { rewindTo, text }),
  /** モデルと effort を変えて送る。会話が続くセッションの id が返る。複製に乗り換えたときは、元とは別の id になる。 */
  switchModel: (id: string, text: string, model: string, effort: string) =>
    postJson<{ sessionId: string }>(`${session(id)}/switch`, { text, model, effort }),
  /** 会話を引き継いだ別のセッションを起動して送る。分岐したセッションの id が返る。 */
  fork: (id: string, text: string, model: string, effort: string) =>
    postJson<{ sessionId: string }>(`${session(id)}/fork`, { text, model, effort }),
  suggestions: (id: string) => postJson<string[]>(`${session(id)}/suggestions`),
  summary: (id: string) => postJson<SessionSummary>(`${session(id)}/summary`),
  glossary: (id: string) => postJson<Glossary>(`${session(id)}/glossary`),
  compact: (id: string) => postJson<void>(`${session(id)}/compact`),
  stop: (id: string) => postJson<void>(`${session(id)}/stop`),
  /** スリープや通信の途切れで止まったセッションを続けるか、確認のダイアログで答える。 */
  answerOffline: (id: string, resume: boolean) => postJson<void>(`${session(id)}/offline`, { resume }),
  /** Claude が求めた認証を進めるか、確認のダイアログで答える。 */
  answerAuth: (id: string, ok: boolean) => postJson<void>(`/api/auth/${seg(id)}`, { ok }),
  diagram: (id: string, note: string) => postJson<DocMeta>(`${session(id)}/diagram`, { note }),
  exportDoc: async (id: string, format: 'pdf' | 'png' | 'html'): Promise<Blob> => {
    const res = await fetch(`/api/docs/${seg(id)}/export/${format}`, { method: 'POST' });
    if (!res.ok) throw new Error(await errorText(res));
    return res.blob();
  },
  prNotes: (id: string, pr: string) => postJson<PrNotes>(`${session(id)}/pr-notes`, { pr }),
  review: (id: string, comments: ReviewComment[], summary: string, branch?: string) =>
    postJson<{ delivery: Delivery }>(`${session(id)}/review`, { comments, summary, branch }),
  prReview: (id: string, comments: ReviewComment[], summary: string, commit: string) =>
    postJson<{ url: string }>(`${session(id)}/pr-review`, { comments, summary, commit }),
  setPrViewed: (id: string, path: string, viewed: boolean) => postJson(`${session(id)}/pr-viewed`, { path, viewed }),
  answer: (attentionId: string, answer: string) => postJson(`/api/attention/${seg(attentionId)}/answer`, { answer }),
  dismiss: (attentionId: string) => postJson(`/api/attention/${seg(attentionId)}/dismiss`),
  manual: (outboxId: string) => postJson(`/api/outbox/${seg(outboxId)}/manual`),
  cancel: (outboxId: string) => postJson(`/api/outbox/${seg(outboxId)}/cancel`),
  retry: (outboxId: string) => postJson(`/api/outbox/${seg(outboxId)}/retry`),
  editQueued: (queuedId: string) => postJson(`/api/queue/${seg(queuedId)}/edit`),
  saveQueued: (queuedId: string, text: string) => postJson(`/api/queue/${seg(queuedId)}/save`, { text }),
  cancelQueued: (queuedId: string) => postJson(`/api/queue/${seg(queuedId)}/cancel`),
  refreshJira: () => postJson('/api/jira/refresh'),
  jiraComment: (key: string, body: string) => postJson(`/api/jira/issues/${seg(key)}/comment`, { body }),
  jiraTransition: (key: string, id: string) => postJson(`/api/jira/issues/${seg(key)}/transitions`, { id }),
  prComment: (url: string, body: string) => postJson<{ url: string }>('/api/pr/comment', { url, body }),
  feedback: (text: string, targets: FeedbackTarget[], context: FeedbackContext) =>
    postJson<{ path: string }>('/api/feedback', { text, targets, context }),
  improve: (patch: Partial<ImproveSettings>) => postJson('/api/improve', patch),
  autoContinue: (enabled: boolean) => postJson<void>('/api/auto-continue', { enabled }),
  assistant: (messages: AssistantMessage[], sessionId: string) => postJson<AssistantReply>('/api/assistant', { messages, sessionId }),
  /** 文章校正の進み具合を、届いた順に onEvent に渡す。signal で中止すると、サーバーも残りの文を読ませない。明確化では、readings に答えがある文は読み直さない。 */
  proofread: async (mode: ProofMode, text: string, glossary: string, readings: (ProofReading | null)[], onEvent: (e: ProofEvent) => void, signal: AbortSignal): Promise<void> => {
    const res = await fetch('/api/proofread', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ mode, text, glossary, readings }), signal });
    if (!res.ok || !res.body) throw new Error(await errorText(res));
    const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
    let rest = '';
    for (let chunk = await reader.read(); !chunk.done; chunk = await reader.read()) {
      const lines = (rest + chunk.value).split('\n');
      rest = lines.pop() ?? '';
      for (const line of lines) if (line) onEvent(JSON.parse(line) as ProofEvent);
    }
  },
  proofFiles: () => getJson<ProofFileMeta[]>('/api/proofread/files'),
  /** 無いファイルは null。 */
  proofFile: (id: string) => getJson<ProofFile | null>(`/api/proofread/files/${seg(id)}`),
  saveProofFile: (file: ProofFile) => postJson(`/api/proofread/files/${seg(file.id)}`, file),
  minesHint: (rows: string[], mines: number) => postJson<{ odds: (number | null)[] }>('/api/minesweeper/hint', { rows, mines }),
  reviseProof: (text: string, glossary: string, fixed: number[], notes: ReviseNote[], mode: ProofMode) => postJson<{ text: string }>('/api/proofread/revise', { text, glossary, fixed, notes, mode }),
  /** 画像は JSON にせず、そのまま本文で送る。 */
  wallpaperFile: async (file: File): Promise<void> => {
    const res = await fetch('/api/wallpaper', { method: 'POST', headers: { 'content-type': file.type }, body: file });
    if (!res.ok) throw new Error(await errorText(res));
  },
  wallpaperUrl: (url: string) => postJson('/api/wallpaper/url', { url }),
  resetWallpaper: () => postJson('/api/wallpaper/reset'),
  reportError: (message: string, stack: string | undefined) => postJson('/api/errors', { message, stack, url: location.href }),
  /** 画面が固まった処理。detail は、固まっていた間に動いたスクリプトの内訳。 */
  reportSlow: (message: string, ms: number, detail: string) => postJson('/api/errors', { message, ms, stack: detail, url: location.href }),
};

/** 失敗をトーストで知らせる。成功なら結果を、失敗なら undefined を返す。 */
export const attempt = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
  try {
    return await fn();
  } catch (e) {
    toast(`${label}に失敗しました（${e instanceof Error ? e.message : String(e)}）`, 'error');
    return undefined;
  }
};
