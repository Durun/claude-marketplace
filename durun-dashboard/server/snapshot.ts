// 画面に出す全体像を、Claude Code のファイル、外部の情報源、ダッシュボードの状態から組み立てる。
import fs from 'node:fs';
import path from 'node:path';
import type { Attention, BgTask, Session, SessionStatus, Snapshot, Subagent, Usage } from '../shared/types.ts';
import * as claude from './claude.ts';
import { isHelperSession } from './haiku.ts';
import { improveState } from './improve.ts';
import { cachedPr, findJiraKeys, jiraBase, jiraIssue, jiraRefreshedAt } from './integrations.ts';
import { notionEnabled } from './notion.ts';
import { ensureSession, newId, persist, state, type SessionMeta } from './store.ts';
import type { Conversation } from './suggest.ts';

const DAY_MS = 24 * 60 * 60 * 1000;
/** 終了してからこの期間を過ぎたセッションは会話ログを読まない。 */
const LISTED_AFTER_END_MS = 14 * DAY_MS;
const MAX_TIMELINE_POINTS = 200;
const MAX_MESSAGE_CHARS = 4000;
const HAIKU_WINDOW = 200_000;
const DEFAULT_WINDOW = 1_000_000;
const PUBLIC_DIR = path.join(import.meta.dirname, '..', 'public');

/** worktree（.claude/worktrees/x や .worktrees/x）は元のリポジトリ名でまとめる。 */
export const repoName = (cwd: string): string => path.basename(cwd.replace(/\/(\.claude\/worktrees|\.worktrees)\/.*$/, ''));

/** テーマを付けていないセッションは、リポジトリ名をテーマにする。 */
const themeOf = (meta: SessionMeta): string => meta.theme ?? repoName(meta.cwd);

/**
 * テーマの名前を変える。リポジトリ名のテーマに入っていたセッションにも、新しい名前を付ける。
 * 画面はテーマの前後の空白を除いてまとめるので、比べるときも除く。
 */
export const renameTheme = (sessions: SessionMeta[], from: string, to: string): void => {
  for (const meta of sessions) if (themeOf(meta).trim() === from) meta.theme = to;
};

// ponytail: statusline が無いときの上限はモデル名からの推定。現行は Haiku だけが 200K で、他は 1M。
const defaultWindow = (model: string | undefined): number => (model?.includes('haiku') ? HAIKU_WINDOW : DEFAULT_WINDOW);

const downsample = (points: [number, number][]): [number, number][] => {
  if (points.length <= MAX_TIMELINE_POINTS) return points;
  const step = points.length / MAX_TIMELINE_POINTS;
  const sampled = Array.from({ length: MAX_TIMELINE_POINTS - 1 }, (_, i) => points[Math.floor(i * step)] as [number, number]);
  return [...sampled, points[points.length - 1] as [number, number]];
};

const buildUsage = (sessionId: string, log: claude.LogSummary | undefined, subs: claude.SubagentLog[]): Usage => {
  const sl = claude.readStatusline(sessionId);
  const contextWindow = sl?.windowSize ?? defaultWindow(log?.model);
  const contextTokens = log?.contextTokens ?? 0;
  // statusline はターミナルに描かれている間しか更新されないので、会話ログより新しいときだけ信じる。
  const exact = sl?.usedPercent != null && sl.mtime >= (log?.lastUsageAt ?? 0);
  const totals = { input: 0, output: 0, cacheRead: 0, cacheCreation: 0, ...log?.totals };
  for (const { log: sub } of subs) for (const k of Object.keys(totals) as (keyof typeof totals)[]) totals[k] += sub.totals[k];
  return {
    model: log?.model,
    contextTokens,
    contextWindow,
    contextPercent: exact ? (sl?.usedPercent ?? 0) : (contextTokens / contextWindow) * 100,
    exact,
    totals,
    costUsd: log?.costUsd,
    timeline: downsample(log?.timeline ?? []),
  };
};

/** 画面は発言を Markdown として「もっと読む」で全体を出すので、書式が崩れないよう、収まる最後の改行で切る。1 行が長すぎるときだけ行の途中で切る。 */
const clipMessage = (text: string | undefined): string | undefined => {
  if (!text || text.length <= MAX_MESSAGE_CHARS) return text;
  const end = text.lastIndexOf('\n', MAX_MESSAGE_CHARS);
  return text.slice(0, end > 0 ? end : MAX_MESSAGE_CHARS);
};

// 名前を付けていないセッションは「ディレクトリ名-16進数」か job id になる。
const isDefaultName = (name: string, cwd: string): boolean =>
  /^[0-9a-f]{8}$/.test(name) || (name.startsWith(`${path.basename(cwd)}-`) && /^[0-9a-f]+$/.test(name.slice(path.basename(cwd).length + 1)));

const displayName = (meta: SessionMeta, log: claude.LogSummary | undefined): string => {
  const custom = meta.transcriptPath && claude.readCustomTitle(meta.transcriptPath);
  const named = meta.name && !isDefaultName(meta.name, meta.cwd) ? meta.name : undefined;
  return meta.title ?? custom ?? named ?? log?.title ?? meta.name ?? path.basename(meta.cwd);
};

/** 実行中セッションの情報を、終了後も使えるよう保存する。 */
const rememberLive = (meta: SessionMeta, live: claude.LiveSession): void => {
  const next = {
    name: live.name,
    kind: live.kind,
    entrypoint: live.entrypoint,
    remoteUrl: live.bridgeSessionId ? `https://claude.ai/code/${live.bridgeSessionId}` : meta.remoteUrl,
    jobId: live.jobId,
    startedAt: live.startedAt,
  };
  if (Object.entries(next).some(([k, v]) => meta[k as keyof typeof next] !== v)) {
    Object.assign(meta, next);
    persist();
  }
};

const toSubagent = (sub: claude.SubagentLog, alive: boolean): Subagent => ({
  id: sub.id,
  type: sub.type,
  description: sub.description,
  model: sub.model,
  // サブエージェントは SubagentHandback で結果を返して終わる。それ以外のツール呼び出しで止まっていれば作業中。
  running: alive && (sub.log.lastUsageAt === undefined || (!!sub.log.lastToolUse && sub.log.lastToolUse !== 'SubagentHandback')),
  outputTokens: sub.log.totals.output,
});

const toBgTask = ({ outputFile: _, ...t }: claude.BgTaskLog, alive: boolean): BgTask => ({
  ...t,
  status: t.status === 'running' && !alive ? 'stopped' : t.status,
});

// ponytail: 最後の発言が文章なら返し終えたとみなすので、API エラーなどの合成メッセージで止まったものも done になる。
// 分けるなら、model が <synthetic> の発言を除く。
export const stoppedStatus = (archived: boolean, log: Pick<claude.LogSummary, 'lastMessage' | 'lastToolUse'> | undefined): SessionStatus => {
  if (archived) return 'ended';
  return log?.lastMessage && !log.lastToolUse ? 'done' : 'interrupted';
};

/**
 * バックグラウンドのセッションは、AskUserQuestion で止まっても通知のフックを送らない。
 * そのため、入力待ちで最後のツールが AskUserQuestion なら、会話ログから質問を記録する。記録した質問は、端末のセッションと同じく次のフックで片付く。
 */
const recordBgQuestion = (meta: SessionMeta, log: claude.LogSummary | undefined, live: claude.LiveSession | undefined): void => {
  if (meta.kind !== 'bg' || live?.status !== 'idle' || log?.lastToolUse !== claude.ASK_TOOL) return;
  if (state.attention.some((a) => a.sessionId === meta.id && (a.kind === 'question' || a.kind === 'permission'))) return;
  const q = claude.questionDialog(log.lastQuestions ?? []);
  if (!q) return;
  state.attention.push({ id: newId(), kind: 'question', sessionId: meta.id, title: '質問', ...q, createdAt: Date.now() });
  persist();
};

const buildSession = (meta: SessionMeta, live: claude.LiveSession | undefined): Session => {
  if (!meta.transcriptPath) {
    meta.transcriptPath = claude.findTranscript(meta.id);
    if (meta.transcriptPath) persist();
  }
  const log = meta.transcriptPath ? claude.summarizeLog(meta.transcriptPath) : undefined;
  const subs = meta.transcriptPath ? claude.readSubagents(meta.transcriptPath) : [];
  recordBgQuestion(meta, log, live);
  const name = displayName(meta, log);
  const pr = cachedPr(meta.cwd, log?.branch);
  const jiraKeys = new Set([...meta.jira, ...findJiraKeys([log?.branch, name, pr?.title])]);
  const repo = repoName(meta.cwd);
  const review = state.attention.find((a) => a.sessionId === meta.id && a.kind === 'review');
  return {
    id: meta.id,
    name,
    theme: themeOf(meta),
    repo,
    cwd: meta.cwd,
    branch: log?.branch,
    status: live?.status ?? stoppedStatus(meta.archived, log),
    kind: meta.kind,
    remoteUrl: meta.remoteUrl,
    attachCommand: meta.kind === 'bg' && meta.jobId ? `claude attach ${meta.jobId}` : undefined,
    startedAt: meta.startedAt,
    updatedAt: Math.max(log?.lastActivityAt ?? 0, live?.updatedAt ?? 0, meta.firstSeen),
    lastPrompt: log?.lastPrompt?.slice(0, MAX_MESSAGE_CHARS),
    lastMessage: clipMessage(log?.lastMessage),
    lastMessageAt: log?.lastMessageAt,
    tasks: claude.readTasks(meta.id),
    subagents: subs.map((s) => toSubagent(s, !!live)),
    background: [log, ...subs.map((s) => s.log)].flatMap((l) => [...(l?.bgTasks.values() ?? [])].map((t) => toBgTask(t, !!live))),
    pr,
    jira: [...jiraKeys].map(jiraIssue),
    usage: buildUsage(meta.id, log, subs),
    archived: meta.archived,
    pinned: !!meta.pinned,
    links: meta.links ?? [],
    files: meta.files ?? [],
    docOrder: meta.docOrder ?? [],
    hiddenDocs: meta.hiddenDocs ?? [],
    docTitles: meta.docTitles ?? {},
    pendingMessages: state.outbox.filter((m) => m.sessionId === meta.id && !m.deliveredAt).length,
    awaitingReview: !!review,
    reviewRange: review?.range,
    branchViews: meta.branchViews,
  };
};

// 会話ログの場所が分からない終了済みセッションは、表示できる中身が無いので一覧から外れる。
const mtimeMs = (file: string | undefined): number => {
  try {
    return file ? fs.statSync(file).mtimeMs : 0;
  } catch {
    return 0;
  }
};

/** 対応が済んだ状態に戻った PR の既読印を消し、次に失敗したときにまた知らせる。 */
const prAttention = (s: Session, dismissed: Set<string>): Attention[] => {
  const pr = s.pr;
  if (!pr) return [];
  const items: Attention[] = [];
  const add = (suffix: string, active: boolean, title: string): void => {
    const id = `pr:${pr.url}:${suffix}`;
    if (!active) dismissed.delete(id);
    else items.push({ id, kind: 'pr', sessionId: s.id, title, body: pr.title, url: pr.url, createdAt: s.updatedAt });
  };
  const open = pr.state === 'OPEN' && !s.archived;
  add('ci-failed', open && pr.checks.failed > 0, `PR #${pr.number} の CI が ${pr.checks.failed} 件失敗`);
  add('changes-requested', open && pr.review === 'CHANGES_REQUESTED', `PR #${pr.number} に変更要求`);
  return items;
};

const deriveAttention = (sessions: Session[]): Attention[] => {
  const dismissed = new Set(state.dismissed);
  const items: Attention[] = [...state.attention];
  for (const s of sessions) {
    const meta = state.sessions[s.id];
    const waiting = state.attention.some((a) => a.sessionId === s.id);
    const unseen = (s.lastMessageAt ?? 0) > (meta?.replySeenAt ?? 0);
    if (s.status === 'idle' && s.lastMessage && unseen && !waiting && !s.archived) {
      items.push({ id: `reply:${s.id}`, kind: 'reply', sessionId: s.id, title: '入力待ち', body: s.lastMessage, createdAt: s.lastMessageAt ?? s.updatedAt });
    }
    items.push(...prAttention(s, dismissed));
  }
  for (const d of state.docs) {
    if (!d.seen) items.push({ id: `doc:${d.id}`, kind: 'doc', sessionId: d.sessionId, title: `資料: ${d.title}`, docId: d.id, createdAt: d.createdAt });
  }
  if (dismissed.size !== state.dismissed.length) {
    state.dismissed = [...dismissed];
    persist();
  }
  return items.filter((a) => !dismissed.has(a.id));
};

// claude -p や Agent SDK からの実行は数が多く、人が追う対象ではない。ダッシュボードとやり取りしたものだけ出す。
// 一度も実行中の一覧に現れずフックだけで知ったセッションも、同じく短命な自動実行とみなす。
const isListed = (m: SessionMeta): boolean =>
  (!!m.kind && m.entrypoint !== 'sdk-cli') ||
  !!m.improver ||
  state.docs.some((d) => d.sessionId === m.id) ||
  state.attention.some((a) => a.sessionId === m.id) ||
  state.outbox.some((o) => o.sessionId === m.id);

/**
 * 概要、候補、名前の材料。画面に出している名前とテーマを渡すため、スナップショットのセッションから作る。
 * 作業の全体が分かるよう、最初の依頼も会話ログから読んで渡す。やり取りが無ければ作れない。
 */
export const conversationOf = (id: string): Conversation | undefined => {
  const s = buildSnapshot().sessions.find((x) => x.id === id);
  if (!s?.lastPrompt && !s?.lastMessage) return undefined;
  const transcript = state.sessions[id]?.transcriptPath;
  return { ...s, firstPrompt: transcript ? claude.summarizeLog(transcript).texts.find((t) => t.kind === 'prompt')?.text : undefined };
};

export const buildSnapshot = (): Snapshot => {
  const now = Date.now();
  const live = claude.readLiveSessions();
  // ダッシュボードが補助に動かす claude -p は、一覧に出さないので、状態に記録しない。
  for (const l of live.values()) if (!isHelperSession(l.sessionId)) rememberLive(ensureSession(l.sessionId, l.cwd), l);
  const sessions = Object.values(state.sessions)
    .filter((m) => isListed(m) && (live.has(m.id) || now - mtimeMs(m.transcriptPath) < LISTED_AFTER_END_MS))
    .map((m) => buildSession(m, live.get(m.id)));
  const themes = [...new Set(sessions.flatMap((s) => [s.theme, s.repo]))].sort((a, b) => a.localeCompare(b, 'ja'));
  return {
    now,
    sessions,
    attention: deriveAttention(sessions),
    docs: state.docs.map(({ content: _content, ...meta }) => meta).reverse(),
    outbox: state.outbox,
    rateLimits: claude.readRateLimits(),
    themes,
    sessionOrder: state.sessionOrder,
    improve: improveState(),
    jiraRefreshedAt: jiraRefreshedAt(),
    jiraBase: jiraBase(),
    notion: notionEnabled(),
    assets: { js: mtimeMs(path.join(PUBLIC_DIR, 'app.js')), css: mtimeMs(path.join(PUBLIC_DIR, 'style.css')) },
  };
};
