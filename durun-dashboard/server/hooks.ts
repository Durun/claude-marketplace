// Claude Code のフックを受ける。要対応を記録し、名前の無いセッションに名前を付け、改善のセッションのウェーブを区切り、worktree に入る権限を決める。
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { ASK_TOOL, type LiveSession, questionDialog, readCustomTitle, readLiveSessions, summarizeLog, toQuestions } from './claude.ts';
import { dropStaleRequests } from './deliver.ts';
import { isHelperSession } from './haiku.ts';
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
  tool_input?: { command?: unknown; questions?: unknown; name?: unknown; path?: unknown };
};

/** フックへの応答。PreToolUse で、ツールを使ってよいかを Claude Code に返す。 */
export type HookOutput = { hookSpecificOutput: { hookEventName: 'PreToolUse'; permissionDecision: 'allow' | 'deny'; permissionDecisionReason: string } };

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

const toDialog = (q: ReturnType<typeof questionDialog>): Dialog | undefined => q && { kind: 'question', title: '質問', ...q };

/** PreToolUse を送らない、フックを足す前に始まったセッションの質問は、会話ログから読む。 */
const questionOf = (meta: SessionMeta): Dialog | undefined => {
  const log = meta.transcriptPath ? summarizeLog(meta.transcriptPath) : undefined;
  return log?.lastToolUse === ASK_TOOL ? toDialog(questionDialog(log.lastQuestions ?? [])) : undefined;
};

const pushDialog = (meta: SessionMeta, dialog: Dialog, replaces: (a: StoredAttention) => boolean): void => {
  state.attention = state.attention.filter((a) => !replaces(a));
  state.attention.push({ id: newId(), sessionId: meta.id, ...dialog, createdAt: Date.now() });
  touch();
};

/**
 * AskUserQuestion の質問は、ツールを呼ぶ前のフックが渡す入力から記録する。
 * Claude Code は、呼び出しを会話ログに書くのを回答の後まで遅らせることがあるので、止まった時点の会話ログからは読めない。
 */
const recordQuestion = (meta: SessionMeta, input: HookInput): void => {
  const dialog = toDialog(questionDialog(toQuestions(input.tool_input?.questions)));
  if (dialog) pushDialog(meta, dialog, (a) => isDialog(a, meta.id));
};

/**
 * Claude Code は AskUserQuestion の質問で止まったときも、権限の確認として通知する。記録済みの質問があれば、その通知なので質問を残す。
 * サブエージェントの権限の確認は、質問と並べて記録する。
 */
const recordNotification = (meta: SessionMeta, input: HookInput): void => {
  const title = NEEDS_HUMAN[input.notification_type ?? ''];
  if (!title) return;
  if (input.notification_type === 'permission_prompt') {
    if (state.attention.some((a) => isDialog(a, meta.id) && a.kind === 'question')) return;
    const question = questionOf(meta);
    if (question) return pushDialog(meta, question, (a) => isDialog(a, meta.id));
  }
  pushDialog(meta, { kind: 'permission', title, body: input.message }, (a) => isDialog(a, meta.id) && a.kind === 'permission');
};

/** 質問はセッションの本体が尋ねるので、サブエージェントが先へ進んでも残す。 */
const clearDialogs = (sessionId: string, fromSubagent: boolean): void => {
  const before = state.attention.length;
  state.attention = state.attention.filter((a) => !isDialog(a, sessionId) || (fromSubagent && a.kind === 'question'));
  if (state.attention.length !== before) touch();
};

/** stack の PR は、ほかのブランチに居ながらまとめて push や作成をするので、居たことのあるブランチの PR をすべて取り直す。 */
const refreshPrFor = (meta: SessionMeta): void => {
  if (!meta.transcriptPath) return;
  for (const branch of summarizeLog(meta.transcriptPath).branches) void refreshPr(meta.cwd, branch);
};

const observe = (meta: SessionMeta, event: string | undefined, input: HookInput): void => {
  if (input.transcript_path && meta.transcriptPath !== input.transcript_path) {
    meta.transcriptPath = input.transcript_path;
    touch();
  }
  if (event === 'Notification') return recordNotification(meta, input);
  if (event === 'PreToolUse' && input.tool_name === ASK_TOOL && !input.agent_id) return recordQuestion(meta, input);
  // 通知以外のイベントが来たら、権限の確認や質問は済んで先へ進んでいる。
  clearDialogs(meta.id, !!input.agent_id);
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

const ENTER_WORKTREE = 'EnterWorktree';
// worktree を作る場所。.claude/worktrees は Claude Code の既定で、.worktrees は WorktreeCreate のフックで作ることの多い場所。
const WORKTREE_DIRS = [path.join('.claude', 'worktrees'), '.worktrees'];
// 許可してから、入った先がセッションファイルの cwd に出るまで待つ時間。WorktreeCreate のフックが依存を入れると数分かかる。
const ENTERING_MS = 5 * 60 * 1000;
/** EnterWorktree を許したセッションごとの、入る先の候補と許した時刻。cwd が移る前に、ほかのセッションが同じ先へ入るのを断るために持つ。 */
const entering = new Map<string, { targets: string[]; at: number }>();

/** cwd を含むリポジトリの元の作業ツリー。worktree の中からでも元の場所を返す。git の外なら undefined。 */
const repoRoot = (cwd: string): string | undefined => {
  try {
    return path.dirname(execFileSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim());
  } catch {
    return undefined;
  }
};

/**
 * EnterWorktree で入る先の候補。path はその場所で、name は Claude Code が作るか、同じ名前があれば入り直す場所。
 * どちらも無ければ新しい名前で作るので、候補は無い。git の外で name を渡されたときは、場所を決められないので undefined。
 */
const enterTargets = (cwd: string, input: HookInput['tool_input']): string[] | undefined => {
  if (typeof input?.path === 'string') return [path.resolve(cwd, input.path)];
  const name = input?.name;
  if (typeof name !== 'string') return [];
  const root = repoRoot(cwd);
  return root ? WORKTREE_DIRS.map((dir) => path.join(root, dir, name)) : undefined;
};

const isInside = (p: string, dir: string): boolean => {
  const rel = path.relative(dir, p);
  return !rel.startsWith('..') && !path.isAbsolute(rel);
};

/** targets の中にいるか、targets へ入ると決めたばかりの、ほかのセッション。読み取りだけで終わる補助のセッションは数えない。 */
const occupantOf = (self: string, targets: string[]): LiveSession | undefined => {
  const now = Date.now();
  return [...readLiveSessions().values()].find((s) => {
    if (s.sessionId === self || isHelperSession(s.sessionId)) return false;
    const claim = entering.get(s.sessionId);
    const places = [s.cwd, ...(claim && now - claim.at < ENTERING_MS ? claim.targets : [])];
    return places.some((p) => targets.some((t) => isInside(p, t)));
  });
};

const decide = (permissionDecision: 'allow' | 'deny', permissionDecisionReason: string): HookOutput => ({
  hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision, permissionDecisionReason },
});

/**
 * EnterWorktree の権限の確認を、ほかのセッションがいない worktree なら許可し、いれば断る。1 つの作業ツリーを複数のセッションが同時に書き換えないようにするため。
 * 入る先を決められないときは、Claude Code の権限の確認に任せる。
 */
const enterWorktree = (sessionId: string, cwd: string, input: HookInput['tool_input']): HookOutput | undefined => {
  const targets = enterTargets(cwd, input);
  if (!targets) return undefined;
  const other = occupantOf(sessionId, targets);
  if (other) {
    const name = state.sessions[other.sessionId]?.title ?? other.name ?? other.sessionId;
    return decide('deny', `別のセッション「${name}」が ${other.cwd} にいるか、入ろうとしています。1 つの worktree に複数のセッションが同時に入らないよう、durun-dashboard が止めました。別の worktree を使うか、人間に相談してください。`);
  }
  entering.set(sessionId, { targets, at: Date.now() });
  return decide('allow', 'ほかのセッションがいない worktree なので、durun-dashboard が許可しました。');
};

/** EnterWorktree の PreToolUse には、使ってよいかを返す。 */
export const handleHook = (input: HookInput, pid: number): HookOutput | undefined => {
  if (!input.session_id || !input.cwd) return;
  if (pid) pidSessions.set(pid, input.session_id);
  const event = input.hook_event_name;
  const meta = ensureSession(input.session_id, input.cwd);
  observe(meta, event, input);
  // サブエージェントが止まっても、応答は終わっておらず、改善のセッションのウェーブも続いている。
  const responded = !input.agent_id && event === 'Stop';
  nameUntitled(meta, input, responded);
  if (responded) nextImprovement(input.session_id, input.last_assistant_message);
  if (event === 'PreToolUse' && input.tool_name === ENTER_WORKTREE) return enterWorktree(input.session_id, input.cwd, input.tool_input);
};
