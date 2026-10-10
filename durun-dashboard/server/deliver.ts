// 人間から Claude へのメッセージを届ける。
// MCP のツールで待っている Claude には直接返し、止まっているセッションと入力待ちのバックグラウンドのセッションは再開して渡し、ほかの動いているセッションには受信ソケットへ送る。
// 人間が画面から新しいセッションを始めるときは、最初の依頼を渡して起動する。アーカイブしたバックグラウンドのセッションは、Claude Code の一覧から消す。
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import type { Delivery, OutboxMessage, ReviewComment } from '../shared/types.ts';
import { CLAUDE_DIR, readJson, readLiveSessions, type LiveSession } from './claude.ts';
import { claudeCommand, platform, type Platform } from './platform.ts';
import { newId, state, touch } from './store.ts';

// バックグラウンドのセッションを動かす supervisor の名簿。動いているジョブだけが workers に載る。
const ROSTER = path.join(CLAUDE_DIR, 'daemon', 'roster.json');
const STOP_TIMEOUT_MS = 15_000;

type Waiter = (text: string) => void;
const waiters = new Map<string, Waiter>();

export const waitFor = (attentionId: string, waiter: Waiter): void => {
  waiters.set(attentionId, waiter);
};

/** 接続が切れたときに呼ぶ。同じ質問で再度呼ばれて待ち手が入れ替わっていれば何もしない。 */
export const unwait = (attentionId: string, waiter: Waiter): void => {
  if (waiters.get(attentionId) === waiter) waiters.delete(attentionId);
};

export const isWaiting = (attentionId: string): boolean => waiters.has(attentionId);

const joinTexts = (msgs: OutboxMessage[]): string => msgs.map((m) => m.text).join('\n---\n');

// 受信ソケットで送ると別のセッションからの発言として届くので、人間がダッシュボードから送ったことを前置きで示す。
// 送るたびに Claude のコンテキストに積まれるので、前置きは印と送り手だけにする。
export const formatMessages = (msgs: OutboxMessage[]): string => `[durun-dashboard] 人間から:\n${joinTexts(msgs)}`;

/** claude --bg はセッションを裏で動かしてすぐに終わるので、終了を待てば成功したかが分かる。 */
const runClaude = (args: string[], cwd: string, env = process.env): Promise<void> =>
  new Promise((resolve, reject) => {
    const claude = claudeCommand(args, env);
    const child = spawn(claude.file, claude.args, { cwd, env, stdio: ['ignore', 'ignore', 'pipe'] });
    let stderr = '';
    child.stderr.on('data', (chunk: Buffer) => (stderr += chunk.toString()));
    child.on('error', reject);
    child.on('exit', (code) => (code === 0 ? resolve() : reject(new Error(stderr.trim() || `exit ${code}`))));
  });

/**
 * 人間が画面から始めるセッション。依頼が - で始まる箇条書きでもオプションと解釈されないよう、-- の後に置く。
 * model と effort が空なら Claude Code の設定に従う。
 */
export const startSession = (cwd: string, prompt: string, model?: string, effort?: string): Promise<void> =>
  runClaude(['--bg', ...(model ? ['--model', model] : []), ...(effort ? ['--effort', effort] : []), '--', prompt], cwd);

/**
 * claude stop は止める指示を出すとすぐに戻り、supervisor が止まったと記録するのは 1、2 秒後になる。
 * 記録される前に --resume すると、動いているとみなされて複製ができるので、名簿から消えるまで待つ。
 * claude agents --json の一覧からは記録より先に消えるので、待つ目安にならない。
 */
const waitForStop = (jobId: string): Promise<void> =>
  new Promise((resolve, reject) => {
    const finish = (error?: Error): void => {
      watcher.close();
      clearTimeout(timer);
      if (error) reject(error);
      else resolve();
    };
    // 名簿が無ければ、supervisor ごと止まっている。読めないときは書き換えの途中なので、次の変更を待つ。
    const check = (): void => {
      if (!fs.existsSync(ROSTER)) return finish();
      const workers = readJson<{ workers: Record<string, unknown> }>(ROSTER)?.workers;
      if (workers && !(jobId in workers)) finish();
    };
    const watcher = fs.watch(path.dirname(ROSTER), check);
    const timer = setTimeout(() => finish(new Error('セッションを止められません')), STOP_TIMEOUT_MS);
    check();
  });

/**
 * --resume で渡した本文は、人間が打った発言として届くので、前置きを付けない。
 * 動いているバックグラウンドのセッションは先に止める。止めたセッションは、起動したときのオプション（--model や --settings）で起こし直される。
 * rewindTo を渡すと、そのエントリまで巻き戻して送る。--bg は --resume-session-at を無視するので、このターンだけ -p で動かす。
 * -p では権限の確認に答えられず、確認の要るツールは拒否される。次のメッセージからは、巻き戻した分岐の続きを --bg で再開する。
 */
const resume = async (sessionId: string, cwd: string, text: string, jobId?: string, rewindTo?: string): Promise<void> => {
  // cmux の端末から起動すると、cmux の claude のラッパーが --settings を足す。
  // フラグを付けて --resume すると、バックグラウンドのセッションは起動したときのオプションを守るために複製を作るので、ラッパーに足させない。
  const env = { ...process.env, CMUX_CLAUDE_HOOKS_DISABLED: '1' };
  if (jobId) {
    await runClaude(['stop', jobId], cwd, env);
    await waitForStop(jobId);
  }
  const args = rewindTo ? ['-p', '--resume', sessionId, '--resume-session-at', rewindTo, '--', text] : ['--bg', '--resume', sessionId, '--', text];
  await runClaude(args, cwd, env);
};

type Job = { id?: string; sessionId?: string; kind?: string; pid?: number };

/**
 * バックグラウンドのセッションを claude rm で一覧から消す。会話ログは残り、claude --resume で開ける。
 * 動いているものは、作業中でも先に止める。端末のセッションと、一覧に無いセッションには何もしない。
 * claude rm は、作業ツリーに未コミットや未 push の変更が残るセッションを消さずに失敗するので、その理由で reject する。
 */
export const removeSession = async (sessionId: string): Promise<void> => {
  const claude = claudeCommand(['agents', '--json', '--all']);
  const { stdout } = await promisify(execFile)(claude.file, claude.args);
  const job = (JSON.parse(stdout) as Job[]).find((j) => j.sessionId === sessionId && j.kind === 'background');
  if (!job?.id) return;
  // 作業ディレクトリは消えていることがあるので、id だけで動く stop と rm はホームで実行する。
  if (job.pid) {
    await runClaude(['stop', job.id], os.homedir());
    await waitForStop(job.id);
  }
  await runClaude(['rm', job.id], os.homedir());
};

/**
 * 止めて再開しても作業が途切れない、入力待ちのバックグラウンドのセッション。
 * 端末のセッションはダッシュボードから止められず、作業中や権限の確認待ちに止めると作業が途切れる。
 */
const restartable = (live: LiveSession): boolean =>
  live.kind === 'bg' && live.status === 'idle' && !!live.jobId && !state.attention.some((a) => a.sessionId === live.sessionId && a.kind === 'permission');

/**
 * Claude Code が各セッションに開く受信ソケット（cross-session messaging）へ、1 行の JSON で送る。
 * 作業中のセッションはツール実行の合間に読み、入力待ちのセッションは新しいターンを始めて読む。
 */
export const postToInbox = (socketPath: string | undefined, text: string, { inboxBlocker }: Pick<Platform, 'inboxBlocker'> = platform): Promise<void> =>
  new Promise((resolve, reject) => {
    if (!socketPath) return reject(new Error('このセッションには受信ソケットがありません'));
    if (inboxBlocker) return reject(new Error(inboxBlocker));
    const socket = net.connect(socketPath, () => socket.end(`${JSON.stringify({ type: 'user', message: { role: 'user', content: text } })}\n`));
    // 失敗したときは close より先に error が来るので、reject で決まる。
    socket.on('error', reject);
    socket.on('close', () => resolve());
  });

/** rewindTo を渡すと、会話をそのエントリまで巻き戻して送る。巻き戻すには止めて再開するので、作業中のセッションと端末のセッションでは失敗する。 */
export const sendMessage = (sessionId: string, text: string, rewindTo?: string): { message: OutboxMessage; delivery: Delivery } => {
  const meta = state.sessions[sessionId];
  if (!meta) throw new Error(`unknown session: ${sessionId}`);
  const live = readLiveSessions().get(sessionId);
  // 受信ソケットで送ったものは承認として扱われないので、再開して渡せるセッションには使わない。
  const viaInbox = !!live && !restartable(live);
  if (rewindTo && viaInbox) throw new Error('作業中のセッションと端末で開いているセッションは巻き戻せません');
  const message: OutboxMessage = { id: newId(), sessionId, text, createdAt: Date.now() };
  state.outbox.push(message);
  // 前に届けられなかったものも一緒に送る。続けて送られても二重に届かないよう、送る前に配信済みにする。
  // 巻き戻すときは、捨てる分岐に宛てた未配信のものを混ぜない。-p の依頼は人が打った発言として記録されないので、前置きでダッシュボードからの依頼と分かるようにする。
  const pending = rewindTo ? [message] : state.outbox.filter((m) => m.sessionId === sessionId && !m.deliveredAt);
  for (const m of pending) Object.assign(m, { deliveredAt: Date.now(), via: viaInbox ? 'socket' : 'resume', error: undefined });
  touch();
  const sent = viaInbox ? postToInbox(live?.messagingSocketPath, formatMessages(pending)) : resume(sessionId, meta.cwd, rewindTo ? formatMessages(pending) : joinTexts(pending), live?.jobId, rewindTo);
  // 届けられなかったら未配信に戻し、理由を残す。
  sent.catch((e: Error) => {
    for (const m of pending) Object.assign(m, { deliveredAt: undefined, via: undefined, error: e.message });
    touch();
    console.error(`${sessionId} にメッセージを届けられません: ${e.message}`);
  });
  return { message, delivery: viaInbox ? 'sent' : 'resumed' };
};

/**
 * /compact はスラッシュコマンドとして動かすため、再開して渡す。受信ソケットで送ると、Claude は発言として読むだけで compact しない。
 * そのため、作業中のセッションと端末のセッションでは失敗する。
 */
export const compactSession = (sessionId: string, cwd: string): Promise<void> => {
  const live = readLiveSessions().get(sessionId);
  if (live && !restartable(live)) throw new Error('作業中のセッションと端末で開いているセッションは compact できません');
  return resume(sessionId, cwd, '/compact', live?.jobId);
};

const removeAttention = (attentionId: string): void => {
  state.attention = state.attention.filter((a) => a.id !== attentionId);
  touch();
};

/** 待っている Claude がいれば直接返す。いなければ前置きを付けてメッセージとして送る。 */
export const resolveAttention = (attentionId: string, text: string, fallbackPrefix: string): Delivery => {
  const item = state.attention.find((a) => a.id === attentionId);
  if (!item) throw new Error(`unknown attention: ${attentionId}`);
  removeAttention(attentionId);
  const waiter = waiters.get(attentionId);
  if (waiter) {
    waiters.delete(attentionId);
    waiter(text);
    return 'mcp';
  }
  return sendMessage(item.sessionId, fallbackPrefix ? `${fallbackPrefix}\n${text}` : text).delivery;
};

export const answerQuestion = (attentionId: string, answer: string): Delivery => {
  const question = state.attention.find((a) => a.id === attentionId)?.body ?? '';
  return resolveAttention(attentionId, answer, `質問「${question}」への回答:`);
};

/** branch は、人間がコードでブランチを表示して付けたコメントのとき。作業ツリーのファイルと取り違えないよう、どのブランチの行かを添える。 */
export const formatReview = (comments: ReviewComment[], summary: string, branch?: string): string => {
  if (!comments.length) return summary.trim() || '指摘はありません。';
  const items = comments.map((c, i) => `${i + 1}. ${c.path}:${c.line}${c.side === 'old' ? '（削除された行）' : ''}\n   > ${c.lineText.trim()}\n   ${c.body}`);
  const head = branch ? `ダッシュボードで ${branch} の差分に付けたコメントです。行番号は ${branch} のファイルのものです。` : 'ダッシュボードでのコードレビューの指摘です。各指摘を反映してください。';
  return [head, summary.trim(), ...items].filter(Boolean).join('\n\n');
};

export const submitReview = (sessionId: string, comments: ReviewComment[], summary: string, branch?: string): Delivery => {
  const text = formatReview(comments, summary, branch);
  const request = state.attention.find((a) => a.sessionId === sessionId && a.kind === 'review');
  if (request) return resolveAttention(request.id, text, '');
  return sendMessage(sessionId, text).delivery;
};

/** ターミナルで人間が答えた場合など、待ち手のいない質問やレビュー依頼を片付ける。 */
export const dropStaleRequests = (sessionId: string): void => {
  const before = state.attention.length;
  state.attention = state.attention.filter((a) => !(a.sessionId === sessionId && a.kind !== 'permission' && !waiters.has(a.id)));
  if (state.attention.length !== before) touch();
};
