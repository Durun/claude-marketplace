import type {
  AssistantMessage, AssistantReply, BranchView, ChatEntry, Delivery, DiffFile, DiffRange, DiffResult, Doc, DocMeta, ExplorerDir, FeedbackContext, FeedbackTarget, FileTarget, ImproveSettings, InboxSection, JiraDetail, JiraIssue, MemoryStatus, NotionPage, OutboxMessage, PrDetail, PrNotes, PrThread, ReviewComment, SearchResponse, SessionSummary, SkillInfo,
} from '../shared/types.ts';
import { toast } from './toast.tsx';

export const DELIVERY_TOAST: Record<Delivery, string> = {
  mcp: 'Claude に届けました',
  sent: 'セッションに届けました',
  resumed: 'セッションを再開して届けました',
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

/** 値の無い項目を外して、クエリにする。 */
const query = (params: Record<string, string | undefined>): URLSearchParams =>
  new URLSearchParams(Object.entries(params).filter((e): e is [string, string] => e[1] !== undefined));

export const api = {
  diff: (id: string, full: boolean, range?: DiffRange, view?: BranchView) =>
    getJson<DiffResult>(`${session(id)}/diff?${query({ full: full ? '1' : undefined, ...range, branch: view?.branch, base: view?.base })}`),
  file: (id: string, path: string, branch?: string) => getJson<DiffFile>(`${session(id)}/file?${query({ path, branch })}`),
  localFile: (id: string, path: string) => getJson<{ path: string; content: string }>(`${files(id)}/local-file?${query({ path })}`),
  resolveFile: (id: string, path: string) => getJson<FileTarget>(`${files(id)}/resolve-file?${query({ path })}`),
  mediaUrl: (id: string, path: string) => `${files(id)}/media?${query({ path })}`,
  localFileEventsUrl: (id: string, path: string) => `${files(id)}/local-file/events?${query({ path })}`,
  explorer: (path?: string) => getJson<ExplorerDir>(`/api/explorer?${query({ path })}`),
  chat: (id: string) => getJson<ChatEntry[]>(`${session(id)}/chat`),
  subagentChat: (id: string, agentId: string) => getJson<ChatEntry[]>(`${session(id)}/subagents/${seg(agentId)}/chat`),
  bgOutput: (id: string, taskId: string) => getJson<{ output?: string }>(`${session(id)}/background/${seg(taskId)}/output`),
  skills: (id: string) => getJson<SkillInfo[]>(`${session(id)}/skills`),
  jiraIssue: (key: string) => getJson<JiraDetail>(`/api/jira/issues/${seg(key)}`),
  notionPage: (id: string) => getJson<NotionPage>(`/api/notion/pages/${seg(id)}`),
  notionTitle: (id: string) => getJson<{ title: string }>(`/api/notion/pages/${seg(id)}/title`),
  prDetail: (url: string) => getJson<PrDetail>(`/api/pr?${query({ url })}`),
  prThreads: (id: string) => getJson<PrThread[]>(`${session(id)}/pr-threads`),
  prViewed: (id: string) => getJson<string[]>(`${session(id)}/pr-viewed`),
  doc: (id: string) => getJson<Doc>(`/api/docs/${seg(id)}`),
  search: (q: string) => getJson<SearchResponse>(`/api/search?q=${encodeURIComponent(q)}`),
  githubInbox: () => getJson<InboxSection[]>('/api/github/inbox'),
  jira: (jql: string, max: number) => getJson<JiraIssue[]>(`/api/jira?${new URLSearchParams({ jql, max: String(max) })}`),
  memory: () => getJson<MemoryStatus | null>('/api/memory'),
  theme: (id: string, theme: string) => postJson(`${session(id)}/theme`, { theme }),
  renameTheme: (from: string, to: string) => postJson('/api/themes/rename', { from, to }),
  title: (id: string, title: string) => postJson(`${session(id)}/title`, { title }),
  suggestTitle: (id: string) => postJson<{ title: string }>(`${session(id)}/title-suggestion`),
  /** アーカイブは、Claude Code のセッションを消せなくても済んでいるので、消せなかったことだけを知らせる。 */
  archive: async (id: string, archived: boolean): Promise<void> => {
    const { removeError } = await postJson<{ removeError?: string }>(`${session(id)}/archive`, { archived });
    if (removeError) toast(`Claude Code のセッションは消せませんでした（${removeError}）`, 'error');
  },
  orderSessions: (ids: string[]) => postJson('/api/sessions/order', { ids }),
  pin: (id: string, pinned: boolean) => postJson(`${session(id)}/pin`, { pinned }),
  addLink: (id: string, url: string, title: string) => postJson(`${session(id)}/links`, { url, title }),
  addFile: (id: string, path: string) => postJson(`${session(id)}/files`, { path }),
  orderDocs: (id: string, keys: string[]) => postJson(`${session(id)}/docs/order`, { keys }),
  hideDocs: (id: string, keys: string[]) => postJson(`${session(id)}/docs/hide`, { keys }),
  renameDoc: (id: string, key: string, title: string) => postJson(`${session(id)}/docs/rename`, { key, title }),
  startSession: (cwd: string, prompt: string, model: string, effort: string) => postJson('/api/sessions', { cwd, prompt, model, effort }),
  message: (id: string, text: string) =>
    postJson<{ message: OutboxMessage; delivery: Delivery }>(`${session(id)}/message`, { text }),
  rewind: (id: string, rewindTo: string, text: string) =>
    postJson<{ message: OutboxMessage; delivery: Delivery }>(`${session(id)}/rewind`, { rewindTo, text }),
  suggestions: (id: string) => postJson<string[]>(`${session(id)}/suggestions`),
  summary: (id: string) => postJson<SessionSummary>(`${session(id)}/summary`),
  compact: (id: string) => postJson<void>(`${session(id)}/compact`),
  diagram: (id: string) => postJson<DocMeta>(`${session(id)}/diagram`),
  exportDoc: async (id: string, format: 'pdf' | 'png' | 'html'): Promise<Blob> => {
    const res = await fetch(`/api/docs/${seg(id)}/export/${format}`, { method: 'POST' });
    if (!res.ok) throw new Error(await errorText(res));
    return res.blob();
  },
  prNotes: (id: string) => postJson<PrNotes>(`${session(id)}/pr-notes`),
  review: (id: string, comments: ReviewComment[], summary: string, branch?: string) =>
    postJson<{ delivery: Delivery }>(`${session(id)}/review`, { comments, summary, branch }),
  prReview: (id: string, comments: ReviewComment[], summary: string, commit: string) =>
    postJson<{ url: string }>(`${session(id)}/pr-review`, { comments, summary, commit }),
  setPrViewed: (id: string, path: string, viewed: boolean) => postJson(`${session(id)}/pr-viewed`, { path, viewed }),
  answer: (attentionId: string, answer: string) => postJson(`/api/attention/${seg(attentionId)}/answer`, { answer }),
  dismiss: (attentionId: string) => postJson(`/api/attention/${seg(attentionId)}/dismiss`),
  manual: (outboxId: string) => postJson(`/api/outbox/${seg(outboxId)}/manual`),
  cancel: (outboxId: string) => postJson(`/api/outbox/${seg(outboxId)}/cancel`),
  refreshJira: () => postJson('/api/jira/refresh'),
  jiraComment: (key: string, body: string) => postJson(`/api/jira/issues/${seg(key)}/comment`, { body }),
  prComment: (url: string, body: string) => postJson<{ url: string }>('/api/pr/comment', { url, body }),
  feedback: (text: string, targets: FeedbackTarget[], context: FeedbackContext) =>
    postJson<{ path: string }>('/api/feedback', { text, targets, context }),
  improve: (patch: Partial<ImproveSettings>) => postJson('/api/improve', patch),
  assistant: (messages: AssistantMessage[]) => postJson<AssistantReply>('/api/assistant', { messages }),
  /** 画像は JSON にせず、そのまま本文で送る。 */
  wallpaperFile: async (file: File): Promise<void> => {
    const res = await fetch('/api/wallpaper', { method: 'POST', headers: { 'content-type': file.type }, body: file });
    if (!res.ok) throw new Error(await errorText(res));
  },
  wallpaperUrl: (url: string) => postJson('/api/wallpaper/url', { url }),
  resetWallpaper: () => postJson('/api/wallpaper/reset'),
  reportError: (message: string, stack: string | undefined) => postJson('/api/errors', { message, stack, url: location.href }),
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
