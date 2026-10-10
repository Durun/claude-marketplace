// Claude Code のフックを受ける。要対応を記録し、名前の無いセッションに名前を付け、改善のセッションのウェーブを区切る。
import { ASK_TOOL, questionDialog, readCustomTitle, summarizeLog } from './claude.ts';
import { dropStaleRequests } from './deliver.ts';
import { nextImprovement } from './improve.ts';
import { refreshPr } from './integrations.ts';
import { conversationOf } from './snapshot.ts';
import { ensureSession, newId, state, touch, type SessionMeta, type StoredAttention } from './store.ts';
import { suggestTitle } from './suggest.ts';

export type HookInput = {
  hook_event_name?: string;
  session_id?: string;
  transcript_path?: string;
  cwd?: string;
  agent_id?: string;
  /** Stop で渡される、止まる直前の Claude の発言 */
  last_assistant_message?: string;
  notification_type?: string;
  message?: string;
  tool_name?: string;
  tool_input?: { command?: unknown };
};

/**
 * 人間の操作を待って Claude が止まっている通知。idle_prompt は入力待ちの一覧（reply）で扱う。
 * agent_needs_input は、一覧で見ている別のセッションが止まったことを知らせるもの。止まったセッション自身も通知するので記録しない。
 */
const NEEDS_HUMAN: Record<string, string> = {
  permission_prompt: '権限の確認',
  worker_permission_prompt: '権限の確認（サブエージェント）',
  elicitation_dialog: '入力の要求',
  elicitation_url_dialog: '入力の要求',
};

// push や PR 操作の直後は PR の状態が変わるので取り直す。
const PR_COMMAND = /\b(git\s+push|gh\s+pr\s+(create|ready|merge|close|reopen))\b/;

// フックは送り元の claude プロセスの PID を添えてくるので、それで MCP の呼び出し元を引ける。
// セッションファイルを書かない -p 実行や、/clear で id が変わった直後もこれで追える。
const pidSessions = new Map<number, string>();

export const sessionIdForPid = (pid: number): string | undefined => pidSessions.get(pid);

/** Claude Code の画面で人間の操作を待っている要対応。セッションごとに最後の 1 件だけを持つ。 */
const isDialog = (a: StoredAttention, sessionId: string): boolean => a.sessionId === sessionId && (a.kind === 'permission' || a.kind === 'question');

type Dialog = Pick<StoredAttention, 'kind' | 'title' | 'body' | 'options'>;

/** Claude Code は AskUserQuestion の質問で止まったときも、権限の確認として通知する。そのときは質問として記録する。 */
const questionOf = (meta: SessionMeta): Dialog | undefined => {
  const log = meta.transcriptPath ? summarizeLog(meta.transcriptPath) : undefined;
  const q = log?.lastToolUse === ASK_TOOL ? questionDialog(log.lastQuestions ?? []) : undefined;
  return q && { kind: 'question', title: '質問', ...q };
};

const recordNotification = (meta: SessionMeta, input: HookInput): void => {
  const title = NEEDS_HUMAN[input.notification_type ?? ''];
  if (!title) return;
  const dialog: Dialog = (input.notification_type === 'permission_prompt' && questionOf(meta)) || { kind: 'permission', title, body: input.message };
  state.attention = state.attention.filter((a) => !isDialog(a, meta.id));
  state.attention.push({ id: newId(), sessionId: meta.id, ...dialog, createdAt: Date.now() });
  touch();
};

const clearDialogs = (sessionId: string): void => {
  const before = state.attention.length;
  state.attention = state.attention.filter((a) => !isDialog(a, sessionId));
  if (state.attention.length !== before) touch();
};

const refreshPrFor = (meta: SessionMeta): void => {
  const branch = meta.transcriptPath ? summarizeLog(meta.transcriptPath).branch : undefined;
  if (branch) void refreshPr(meta.cwd, branch);
};

const observe = (meta: SessionMeta, event: string | undefined, input: HookInput): void => {
  if (input.transcript_path && meta.transcriptPath !== input.transcript_path) {
    meta.transcriptPath = input.transcript_path;
    touch();
  }
  if (event === 'Notification') return recordNotification(meta, input);
  // 通知以外のイベントが来たら、権限の確認や質問は済んで先へ進んでいる。
  clearDialogs(meta.id);
  if (event === 'UserPromptSubmit') {
    meta.replySeenAt = Date.now();
    dropStaleRequests(meta.id);
    touch();
  }
  if (event === 'PostToolUse' && input.tool_name === 'Bash' && PR_COMMAND.test(String(input.tool_input?.command ?? ''))) refreshPrFor(meta);
};

/** 応答を返し終える前でも、作業が始まってこの時間が経てば名前を付ける。 */
const AUTO_TITLE_AFTER_MS = 5 * 60 * 1000;
/** 名前を頼んだセッション。失敗しても、サーバーが起動し直すまでは頼み直さない。 */
const titling = new Set<string>();

/**
 * 人が見るセッションに、人間も set_task も名前を付けていなければ、Claude の最初の応答が終わったときか、始まって 5 分経ったときに haiku が名前を付ける。
 * Claude Code が自動で付ける名前は作業の中身が分かりにくいので、付いていないものとして扱う。
 * Stop の時点では、最後の発言が会話ログにまだ書かれていないことがあるので、フックが渡すものを使う。
 */
const nameUntitled = (meta: SessionMeta, input: HookInput, responded: boolean): void => {
  if (meta.title || titling.has(meta.id) || meta.archived || !meta.kind || meta.entrypoint === 'sdk-cli') return;
  if (!responded && Date.now() - (meta.startedAt ?? meta.firstSeen) < AUTO_TITLE_AFTER_MS) return;
  if (meta.transcriptPath && readCustomTitle(meta.transcriptPath)) return;
  const c = conversationOf(meta.id);
  if (!c) return;
  titling.add(meta.id);
  suggestTitle({ ...c, lastMessage: (responded && input.last_assistant_message) || c.lastMessage }).then(
    (title) => {
      // 考えている間に人間が付けた名前は残す。
      if (meta.title) return;
      meta.title = title;
      touch();
    },
    (e: Error) => console.error(`${meta.id} に名前を付けられません: ${e.message}`),
  );
};

export const handleHook = (input: HookInput, pid: number): void => {
  if (!input.session_id || !input.cwd) return;
  if (pid) pidSessions.set(pid, input.session_id);
  const event = input.hook_event_name;
  const meta = ensureSession(input.session_id, input.cwd);
  observe(meta, event, input);
  // サブエージェントが止まっても、応答は終わっておらず、改善のセッションのウェーブも続いている。
  const responded = !input.agent_id && event === 'Stop';
  nameUntitled(meta, input, responded);
  if (responded) nextImprovement(input.session_id, input.last_assistant_message);
};
