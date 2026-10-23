// 人間から Claude へのメッセージを届ける。
// MCP のツールで待っている Claude には直接返し、止まっているセッションと入力待ちのバックグラウンドのセッションは再開して渡す。
// 作業中のバックグラウンドのセッションは supervisor に返信を頼んで渡し、ほかの動いているセッションには受信ソケットへ送る。
// 人間が画面から新しいセッションを始めるときは、最初の依頼を渡して起動する。アーカイブしたバックグラウンドのセッションは、Claude Code の一覧から消す。
// 作業中にアーカイブしたセッションは、止めずに入力待ちか停止になるのを待ってからアーカイブする。
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { setImmediate } from 'node:timers/promises';
import { promisify } from 'node:util';
import type { Delivery, OutboxMessage, ReviewComment } from '../shared/types.ts';
import { CLAUDE_DIR, HUMAN_LABEL, SESSIONS_DIR, findTranscript, isAlive, readJson, readLiveSessions, summarizeLog, type LiveSession, type LogSummary } from './claude.ts';
import { UpstreamError } from './haiku.ts';
import { claudeCommand, platform } from './platform.ts';
import { ensureSession, moveSession, newId, state, touch, type SessionMeta } from './store.ts';

// バックグラウンドのセッションを動かす supervisor の名簿。動いているジョブだけが workers に載る。
const ROSTER = path.join(CLAUDE_DIR, 'daemon', 'roster.json');
// supervisor に頼むときに名乗る鍵。Claude Code が作り、OS のユーザーだけが読める。
const CONTROL_KEY = path.join(CLAUDE_DIR, 'daemon', 'control.key');
const SUPERVISOR_TIMEOUT_MS = 5000;
const STOP_TIMEOUT_MS = 15_000;
// 認証を拒まれた接続は数ミリ秒で閉じられ、受け付けられた接続は開いたままになる。これだけ待てば見分けられる。
const AUTH_GRACE_MS = 200;
const ROSTER_POLL_MS = 500;
const EXIT_POLL_MS = 200;

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
export const formatMessages = (msgs: OutboxMessage[]): string => `${HUMAN_LABEL}${joinTexts(msgs)}`;

/**
 * claude --bg はセッションを裏で動かしてすぐに終わるので、終了を待てば成功したかが分かる。成功したら、claude の出力を返す。
 * claude が断った理由は UpstreamError で返す。ホームのように信頼していない作業ディレクトリで断られるのは、ダッシュボードの不具合ではないため。
 * started には、起動した claude の pid を渡す。
 */
const claudeOutput = (args: string[], cwd: string, env = process.env, started?: (pid: number) => void): Promise<string> =>
  new Promise((resolve, reject) => {
    const claude = claudeCommand(args, env);
    const child = spawn(claude.file, claude.args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
    if (child.pid) started?.(child.pid);
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk: Buffer) => (stdout += chunk.toString()));
    child.stderr.on('data', (chunk: Buffer) => (stderr += chunk.toString()));
    child.on('error', reject);
    child.on('exit', (code) => (code === 0 ? resolve(`${stdout}\n${stderr}`) : reject(new UpstreamError(stderr.trim() || `exit ${code}`))));
  });

const runClaude = (args: string[], cwd: string, env = process.env): Promise<void> => claudeOutput(args, cwd, env).then(() => undefined);

/** 起動に渡すモデルと effort。空なら渡さず、Claude Code の設定に従わせる。 */
const modelFlags = (model?: string, effort?: string): string[] => [...(model ? ['--model', model] : []), ...(effort ? ['--effort', effort] : [])];

/**
 * 人間が画面から始めるセッション。依頼が - で始まる箇条書きでもオプションと解釈されないよう、-- の後に置く。
 * model と effort が空なら Claude Code の設定に従う。
 * theme を渡すと、起動したセッションに付ける。
 */
export const startSession = async (cwd: string, prompt: string, model?: string, effort?: string, theme?: string): Promise<void> => {
  const output = await claudeOutput(['--bg', ...modelFlags(model, effort), '--', prompt], cwd);
  // 空のテーマを付けると、リポジトリ名にならず未分類になる。
  if (!theme) return;
  // 起動は済んでいるので、テーマを付けられなくても失敗にしない。
  try {
    const sessionId = await backgroundedSession(output);
    if (!sessionId) throw new Error('セッションの id を引けません');
    ensureSession(sessionId, cwd).theme = theme;
    touch();
  } catch (e) {
    console.error(`起動したセッションにテーマを付けられません: ${(e as Error).message}`);
  }
};

/**
 * claude stop は止める指示を出すとすぐに戻り、supervisor が止まったと記録するのは 1、2 秒後になる。
 * 記録される前に --resume すると、動いているとみなされて複製ができるので、名簿から消えるまで待つ。
 * claude agents --json の一覧からは記録より先に消えるので、待つ目安にならない。
 */
const waitForStop = (jobId: string): Promise<void> =>
  new Promise((resolve, reject) => {
    const finish = (error?: Error): void => {
      watcher.close();
      clearInterval(poll);
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
    // 名簿の置き換えを fs.watch が知らせないことがあるので、間を置いて読み直しもする。
    const poll = setInterval(check, ROSTER_POLL_MS);
    const timer = setTimeout(() => finish(new Error('セッションを止められません')), STOP_TIMEOUT_MS);
    check();
  });

/**
 * --resume で渡した本文は、人間が打った発言として届くので、前置きを付けない。
 * 動いているバックグラウンドのセッションは先に止める。止めたセッションは、起動したときのオプション（--model や --settings）で起こし直される。
 * rewindTo を渡すと、そのエントリまで巻き戻して送る。--bg は --resume-session-at を無視するので、このターンだけ -p で動かす。
 * -p では権限の確認に答えられず、確認の要るツールは拒否される。次のメッセージからは、巻き戻した分岐の続きを --bg で再開する。
 * -p のセッションは端末のセッションと同じに見えるので、stopSession が止められるよう pid を覚える。
 * flags は switchSession が渡す。claude の出力を返す。
 */
const relaunch = async (sessionId: string, cwd: string, text: string, jobId?: string, rewindTo?: string, flags: string[] = []): Promise<string> => {
  // 再開したセッションは、渡した依頼を読み始めるまでの数秒、入力待ちに見える。その間に前の発言を返信待ちとして通知し直さないよう、既読にする。
  const meta = state.sessions[sessionId];
  if (meta) meta.replySeenAt = Date.now();
  touch();
  // cmux の端末から起動すると、cmux の claude のラッパーが --settings を足す。
  // フラグを付けて --resume すると、バックグラウンドのセッションは起動したときのオプションを守るために複製を作るので、ラッパーに足させない。
  const env = { ...process.env, CMUX_CLAUDE_HOOKS_DISABLED: '1' };
  // claude は起動し終えるまでサーバーを止め、マシンが混むと 1 秒を超える。送信の API を待たせないよう、応答を返してから起動する。
  await setImmediate();
  if (jobId) {
    await runClaude(['stop', jobId], cwd, env);
    await waitForStop(jobId);
  }
  if (!rewindTo) return claudeOutput(['--bg', '--resume', sessionId, ...flags, '--', text], cwd, env);
  return claudeOutput(['-p', '--resume', sessionId, '--resume-session-at', rewindTo, '--', text], cwd, env, (pid) => {
    if (meta) meta.printPid = pid;
    touch();
  });
};

const resume = (sessionId: string, cwd: string, text: string, jobId?: string, rewindTo?: string): Promise<void> =>
  relaunch(sessionId, cwd, text, jobId, rewindTo).then(() => undefined);

/** 巻き戻して送ったターンの claude -p を、端末で Ctrl+C を押したときと同じ SIGINT で止め、終わるのを待つ。 */
const interrupt = (pid: number): Promise<void> =>
  new Promise((resolve, reject) => {
    process.kill(pid, 'SIGINT');
    const deadline = Date.now() + STOP_TIMEOUT_MS;
    const poll = setInterval(() => {
      if (isAlive(pid) && Date.now() < deadline) return;
      clearInterval(poll);
      if (isAlive(pid)) reject(new Error('セッションを止められません'));
      else resolve();
    }, EXIT_POLL_MS);
  });

/**
 * 作業中でも、バックグラウンドのセッションと、巻き戻して送ったターンを止める。会話ログは残るので、次に送ったメッセージで再開する。
 * 端末のセッションは人間の端末で動いているので、ダッシュボードからは止めない。
 */
export const stopSession = async (sessionId: string): Promise<void> => {
  const live = readLiveSessions().get(sessionId);
  if (!live) throw new Error('このセッションは動いていません');
  if (live.pid === state.sessions[sessionId]?.printPid) return interrupt(live.pid);
  if (live.kind !== 'bg' || !live.jobId) throw new Error('端末で開いているセッションは、ダッシュボードからは止められません');
  await runClaude(['stop', live.jobId], os.homedir());
  await waitForStop(live.jobId);
};

type Job = { id?: string; sessionId?: string; kind?: string; pid?: number };

const listJobs = async (): Promise<Job[]> => {
  const claude = claudeCommand(['agents', '--json', '--all']);
  const { stdout } = await promisify(execFile)(claude.file, claude.args);
  return JSON.parse(stdout) as Job[];
};

/** claude --bg は「backgrounded · ジョブの id」と書く。そのジョブのセッションの id を返す。 */
const backgroundedSession = async (output: string): Promise<string | undefined> => {
  const jobId = /backgrounded\W+(\w+)/.exec(output)?.[1];
  return jobId ? (await listJobs()).find((j) => j.id === jobId)?.sessionId : undefined;
};

/**
 * バックグラウンドのセッションを claude rm で一覧から消す。会話ログは残り、claude --resume で開ける。
 * 動いているものは、作業中でも先に止める。端末のセッションと、一覧に無いセッションには何もしない。
 * claude rm は、作業ツリーに未コミットや未 push の変更が残るセッションを消さずに失敗するので、その理由で reject する。
 */
export const removeSession = async (sessionId: string): Promise<void> => {
  const job = (await listJobs()).find((j) => j.sessionId === sessionId && j.kind === 'background');
  if (!job?.id) return;
  // 作業ディレクトリは消えていることがあるので、id だけで動く stop と rm はホームで実行する。
  if (job.pid) {
    await runClaude(['stop', job.id], os.homedir());
    await waitForStop(job.id);
  }
  await runClaude(['rm', job.id], os.homedir());
};

/**
 * アーカイブし、バックグラウンドのセッションは Claude Code の一覧からも消す。消したセッションには claude attach できないので、そのコマンドも出さない。
 * 消すには claude を何度も動かして数秒かかるので、終わるのを待たずに返る。消せなくてもアーカイブは保ち、消せなかった理由を removeError に残す。
 */
const archive = (meta: SessionMeta): void => {
  Object.assign(meta, { archived: true, archiveWhenIdle: undefined, removeError: undefined });
  touch();
  removeSession(meta.id)
    .then(() => (meta.jobId = undefined), (e: Error) => (meta.removeError = e.message))
    .finally(touch);
};

/** 作業中のセッションは、止めずにアーカイブ待ちにして true を返す。入力待ちか停止になったら、flushArchives がアーカイブする。 */
export const archiveSession = (meta: SessionMeta): boolean => {
  if (readLiveSessions().get(meta.id)?.status !== 'busy') {
    archive(meta);
    return false;
  }
  meta.archiveWhenIdle = true;
  touch();
  return true;
};

/** アーカイブ待ちのセッションのうち、入力待ちか停止になったものをアーカイブする。状態が変わるたびに呼ぶ。 */
export const flushArchives = (): void => {
  const pending = Object.values(state.sessions).filter((m) => m.archiveWhenIdle);
  if (!pending.length) return;
  const live = readLiveSessions();
  for (const meta of pending) {
    // アーカイブで状態が変わるとここがまた呼ばれるので、そこでアーカイブしたものは飛ばす。
    if (!meta.archiveWhenIdle || live.get(meta.id)?.status === 'busy') continue;
    archive(meta);
  }
};

const awaitingPermission = (sessionId: string): boolean => state.attention.some((a) => a.sessionId === sessionId && a.kind === 'permission');

/**
 * 止めて再開しても作業が途切れない、入力待ちのバックグラウンドのセッション。
 * 端末のセッションはダッシュボードから止められず、作業中や権限の確認待ちに止めると作業が途切れる。
 */
const restartable = (live: LiveSession): boolean => live.kind === 'bg' && live.status === 'idle' && !!live.jobId && !awaitingPermission(live.sessionId);

const summaryOf = (sessionId: string): LogSummary | undefined => {
  const transcript = state.sessions[sessionId]?.transcriptPath ?? findTranscript(sessionId);
  return transcript ? summarizeLog(transcript) : undefined;
};

/**
 * 本体のターンを終え、裏で動くサブエージェントを待っているセッション。Claude Code はこの間も作業中と記録するので、会話ログで見分ける。
 * 再開したばかりのセッションの会話ログには、前のプロセスで終えたターンが残っているので、このプロセスが始まった後に終えたターンに限る。
 */
const delegating = (live: LiveSession): boolean => live.status === 'busy' && (summaryOf(live.sessionId)?.turnEndedAt ?? 0) > (live.startedAt ?? Infinity);

/**
 * 権限の確認待ちのバックグラウンドのセッション。入力待ちになるまで保留し、flushQueue に再開して渡させる。
 * 受信ソケットで送ると、Claude Code が別のセッションからの発言として包む。supervisor に返信を頼むと、Claude Code は確認や質問で止まっているジョブへの返信を、その答えとして使うことがある。
 * readyForQueue が送ってよいとする状態とは重ならないので、flushQueue が送ったものをまた保留することはない。
 */
const mustWait = (live: LiveSession | undefined): boolean => live?.kind === 'bg' && awaitingPermission(live.sessionId);

/**
 * supervisor の制御ソケットへ 1 行の JSON で、バックグラウンドのジョブへの返信を頼む。Claude Code の agent view が返信に使う経路。
 * 人間が打った発言として届き、セッションを止めない。Claude が作業中なら、Claude Code が受け付けて順に読む。
 * ponytail: この頼み方は Claude Code の文書に無い。2.1.301 で確かめた。変わったら断られた理由が、送信したメッセージに出る。
 */
const replyToJob = (jobId: string, text: string): Promise<void> =>
  new Promise((resolve, reject) => {
    const request = { proto: 1, op: 'reply', short: jobId, text, auth: fs.readFileSync(CONTROL_KEY, 'utf8').trim() };
    const socket = net.connect(platform.supervisorSocket(CLAUDE_DIR), () => socket.write(`${JSON.stringify(request)}\n`));
    let buf = '';
    socket.setEncoding('utf8');
    socket.setTimeout(SUPERVISOR_TIMEOUT_MS, () => socket.destroy(new Error('supervisor が応えません')));
    socket.on('data', (chunk: string) => {
      buf += chunk;
      const end = buf.indexOf('\n');
      if (end < 0) return;
      socket.end();
      let res: { ok?: boolean; error?: string } = {};
      try {
        res = JSON.parse(buf.slice(0, end)) as typeof res;
      } catch {
        // 読めない返事は、断られたものとして下で扱う。
      }
      if (res.ok) resolve();
      else reject(new Error(res.error ?? 'supervisor の返事を読めません'));
    });
    // 失敗したときは close より先に error が来るので、reject で決まる。返事を受けた後の close では何も変わらない。
    socket.on('error', reject);
    socket.on('close', () => reject(new Error('supervisor が返事をせずに接続を閉じました')));
  });

/**
 * Claude Code が各セッションに開く受信ソケット（cross-session messaging）へ、1 行の JSON で送る。
 * 作業中のセッションはツール実行の合間に読み、入力待ちのセッションは新しいターンを始めて読む。
 */
export const postToInbox = (socketPath: string | undefined, text: string, token?: string): Promise<void> =>
  new Promise((resolve, reject) => {
    if (!socketPath) return reject(new Error('このセッションには受信ソケットがありません'));
    const line = `${JSON.stringify({ type: 'user', message: { role: 'user', content: text } })}\n`;
    let accepted = !token;
    const socket = net.connect(socketPath, () => {
      if (!token) return socket.end(line);
      // 認証を拒んだ受信ソケットは、何も返さずにすぐ接続を閉じる。閉じられないのを確かめてから本文を送り、届いたと偽らない。
      socket.write(`${JSON.stringify({ type: 'auth', token })}\n`);
      setTimeout(() => {
        accepted = true;
        if (!socket.destroyed) socket.end(line);
      }, AUTH_GRACE_MS);
    });
    // 失敗したときは close より先に error が来るので、reject で決まる。
    socket.on('error', reject);
    socket.on('close', () => (accepted ? resolve() : reject(new Error('受信ソケットに認証を拒まれました'))));
  });

/** rewindTo を渡すと、会話をそのエントリまで巻き戻して送る。巻き戻すには止めて再開するので、作業中のセッションと端末のセッションでは失敗する。 */
export const sendMessage = (sessionId: string, text: string, rewindTo?: string): { message: OutboxMessage; delivery: Delivery } => {
  const meta = state.sessions[sessionId];
  if (!meta) throw new Error(`unknown session: ${sessionId}`);
  const live = readLiveSessions().get(sessionId);
  if (rewindTo && live && !restartable(live)) throw new Error('作業中のセッションと端末で開いているセッションは巻き戻せません');
  const message: OutboxMessage = { id: newId(), sessionId, text, createdAt: Date.now() };
  state.outbox.push(message);
  // 前に届けられなかったものも一緒に送る。
  // 巻き戻すときは、捨てる分岐に宛てた未配信のものを混ぜない。
  return { message, delivery: deliver(sessionId, meta.cwd, live, rewindTo ? [message] : undelivered(sessionId), rewindTo) };
};

/** 届けられなかったメッセージを、送り直す操作で届け直す。同じセッションの未配信のものもまとめて送る。 */
export const redeliver = (sessionId: string): Delivery => {
  const meta = state.sessions[sessionId];
  if (!meta) throw new Error(`unknown session: ${sessionId}`);
  return deliver(sessionId, meta.cwd, readLiveSessions().get(sessionId), undelivered(sessionId));
};

const undelivered = (sessionId: string): OutboxMessage[] => state.outbox.filter((m) => m.sessionId === sessionId && !m.deliveredAt);

/** 届け方を返す。権限の確認待ちのバックグラウンドのセッションへは送らず、送信の記録から保留へ移す。 */
const deliver = (sessionId: string, cwd: string, live: LiveSession | undefined, pending: OutboxMessage[], rewindTo?: string): Delivery => {
  if (mustWait(live)) {
    state.outbox = state.outbox.filter((m) => !pending.includes(m));
    state.queue.push(...pending.map(({ id, text, createdAt }) => ({ id, sessionId, text, createdAt })));
    touch();
    return 'queued';
  }
  // 受信ソケットで送ったものは承認として扱われないので、再開して渡せるセッションと、supervisor に返信を頼めるセッションには使わない。
  // 作業中のセッションは、止めると作業もサブエージェントも途切れるので、再開せずに supervisor に返信を頼む。
  const restart = !live || restartable(live);
  const job = !restart && live.kind === 'bg' ? live.jobId : undefined;
  // 続けて送られても二重に届かないよう、送る前に配信済みにする。届け終えるまでは sending を付けておき、途中でサーバーが止まったら次の起動で届け直せるようにする。
  // 巻き戻しは、届け直すともう一度巻き戻すので、sending を付けない。
  const via = restart ? 'resume' : job ? 'reply' : 'socket';
  for (const m of pending) Object.assign(m, { deliveredAt: Date.now(), via, error: undefined, sending: rewindTo ? undefined : true });
  touch();
  // 鍵を読めないときも、届けられなかったものとして下で未配信に戻す。
  const toInbox = async (to: LiveSession): Promise<void> => postToInbox(to.messagingSocketPath, formatMessages(pending), platform.inboxToken(to.pid, SESSIONS_DIR));
  const sent = restart ? resume(sessionId, cwd, joinTexts(pending), live?.jobId, rewindTo) : job ? replyToJob(job, joinTexts(pending)) : toInbox(live);
  sent.then(
    () => {
      for (const m of pending) m.sending = undefined;
      touch();
    },
    // 届けられなかったら未配信に戻し、理由を残す。
    // 巻き戻しの -p はターンを終えるまで動くので、止めたときや途中で失敗したときも、依頼が会話ログに載っていれば届いている。
    (e: Error) => {
      const done = rewindTo ? arrived(sessionId, pending) : new Set<OutboxMessage>();
      const lost = pending.filter((m) => !done.has(m));
      if (!lost.length) return;
      for (const m of lost) Object.assign(m, { deliveredAt: undefined, via: undefined, error: e.message, sending: undefined });
      touch();
      console.error(`${sessionId} にメッセージを届けられません: ${e.message}`);
    },
  );
  return restart ? 'resumed' : 'sent';
};

const ASK_KINDS = new Set(['ask', 'question', 'permission']);

/**
 * 保留したメッセージを送ってよいか。Claude が入力待ちで、質問や権限の確認で止まっていないときに限る。サブエージェントを待っているだけのセッションも、入力待ちとみなす。
 * 質問や権限の確認は、フックで記録されるより先に会話ログに現れることがあるので、答えを待っているツールの呼び出しでも見分ける。
 * 送った直後のセッションは、読み始めるまで入力待ちに見えるので、前に送ったメッセージに Claude が返事をするまでは送らない。
 * アーカイブしたセッションと、アーカイブ待ちのセッションへは送らない。アーカイブ待ちのセッションは、入力待ちになるとすぐにアーカイブして止めるため。
 */
const readyForQueue = (sessionId: string, live: LiveSession | undefined): boolean => {
  const meta = state.sessions[sessionId];
  if (!meta || meta.archived || meta.archiveWhenIdle || !live || !(live.status === 'idle' || delegating(live))) return false;
  if (state.attention.some((a) => a.sessionId === sessionId && ASK_KINDS.has(a.kind))) return false;
  const sent = state.outbox.filter((m) => m.sessionId === sessionId);
  if (sent.some((m) => !m.deliveredAt || m.sending)) return false;
  const log = summaryOf(sessionId);
  if (log?.lastToolUse) return false;
  return !sent.length || (log?.lastMessageAt ?? 0) > Math.max(...sent.map((m) => m.createdAt));
};

/**
 * 送れるようになったセッションへ、保留したメッセージを古い順に 1 つ送る。状態が変わるたびに呼ぶ。
 * 先頭のメッセージを書き直している間は、順番を変えないよう、そのセッションへは何も送らない。
 */
export const flushQueue = (): void => {
  if (!state.queue.length) return;
  const live = readLiveSessions();
  for (const sessionId of new Set(state.queue.map((m) => m.sessionId))) {
    const head = state.queue.find((m) => m.sessionId === sessionId);
    if (!head || head.editing || !readyForQueue(sessionId, live.get(sessionId))) continue;
    // 送ってから外す。送る途中で状態が変わってここが呼ばれても、送っている間は readyForQueue が止める。
    sendMessage(sessionId, head.text);
    state.queue = state.queue.filter((m) => m !== head);
    touch();
  }
};

type LogEntry = { timestamp?: string; content?: unknown; message?: { content?: unknown } };

/**
 * 会話ログに、送った時刻より後の発言として本文が残っているメッセージ。
 * 再開して渡した本文は人間の発言の message.content に、受信ソケットで渡した本文は受信の記録の content に、どちらも文字列で残る。
 */
const arrived = (sessionId: string, msgs: OutboxMessage[]): Set<OutboxMessage> => {
  const found = new Set<OutboxMessage>();
  const transcript = findTranscript(sessionId);
  if (!transcript) return found;
  for (const line of fs.readFileSync(transcript, 'utf8').split('\n')) {
    // 行ごとに JSON を読むのは重いので、本文を含まない行は読まない。
    const candidates = msgs.filter((m) => !found.has(m) && line.includes(JSON.stringify(m.text).slice(1, -1)));
    if (!candidates.length) continue;
    let entry: LogEntry;
    try {
      entry = JSON.parse(line) as LogEntry;
    } catch {
      continue;
    }
    const text = [entry.message?.content, entry.content].find((c) => typeof c === 'string');
    const at = Date.parse(entry.timestamp ?? '');
    for (const m of candidates) if (text?.includes(m.text) && at >= m.createdAt) found.add(m);
  }
  return found;
};

/**
 * 起動したときに呼ぶ。前のサーバーが届けている途中で止まったメッセージを、未配信に戻して届け直す。
 * コードを変えるたびに node --watch がサーバーを止めるので、セッションを止めてから再開するまでの数秒に重なると、配信済みの記録だけが残って本文が消える。
 * 渡し終えてから記録する前に止まった分は、会話ログに本文が残っているので、二重に届けない。
 */
export const redeliverInterrupted = (): void => {
  const cut = state.outbox.filter((m) => m.sending);
  if (!cut.length) return;
  for (const sessionId of new Set(cut.map((m) => m.sessionId))) {
    const mine = cut.filter((m) => m.sessionId === sessionId);
    const done = arrived(sessionId, mine);
    for (const m of mine) Object.assign(m, done.has(m) ? { sending: undefined } : { deliveredAt: undefined, via: undefined, sending: undefined });
  }
  touch();
  for (const sessionId of new Set(cut.filter((m) => !m.deliveredAt).map((m) => m.sessionId))) {
    const meta = state.sessions[sessionId];
    if (meta) deliver(sessionId, meta.cwd, readLiveSessions().get(sessionId), undelivered(sessionId));
  }
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

/** 使用制限が明けたときと、通信が戻ったときに渡す依頼。人間が続きを頼むときと同じ言い方にし、止まったことには触れない。 */
export const CONTINUE_PROMPT = '続けて';

/**
 * 使用制限か通信の途切れで止まったセッションに、続きを頼む。人間からの発言ではないので、前置きを付けない。
 * 動いていないセッションは、人間が閉じたものなので起こさない。
 */
export const continueStopped = async (sessionId: string, cwd: string): Promise<void> => {
  const live = readLiveSessions().get(sessionId);
  if (!live) return;
  if (restartable(live)) return resume(sessionId, cwd, CONTINUE_PROMPT, live.jobId);
  return postToInbox(live.messagingSocketPath, CONTINUE_PROMPT, platform.inboxToken(live.pid, SESSIONS_DIR));
};

/**
 * モデルと effort を変えて、依頼を渡す。会話が続くセッションの id を返す。
 * バックグラウンドのセッションは起動したときのオプションを保存していて、後から変えられない。/model を再開で渡しても、次の再開で元に戻る。
 * フラグを付けて --resume すると、Claude Code は会話を引き継いだ複製を別の id で起動するので、ダッシュボードの設定を複製へ移し、元のセッションはアーカイブする。
 * 端末で始めて終わったセッションは保存したオプションを持たないので、同じ id のまま続く。
 * 止めて再開するので、作業中のセッションと端末で開いているセッションでは失敗する。
 */
export const switchSession = async (sessionId: string, text: string, model?: string, effort?: string): Promise<string> => {
  const meta = state.sessions[sessionId];
  if (!meta) throw new Error(`unknown session: ${sessionId}`);
  const live = readLiveSessions().get(sessionId);
  if (live && !restartable(live)) throw new Error('作業中のセッションと端末で開いているセッションは、モデルと effort を変えられません');
  state.outbox.push({ id: newId(), sessionId, text, createdAt: Date.now() });
  const pending = undelivered(sessionId);
  // 届け直すと複製がもう 1 つできるので、巻き戻しと同じく sending を付けない。
  for (const m of pending) Object.assign(m, { deliveredAt: Date.now(), via: 'resume', error: undefined });
  touch();
  let output: string;
  try {
    output = await relaunch(sessionId, meta.cwd, joinTexts(pending), live?.jobId, undefined, modelFlags(model, effort));
  } catch (e) {
    for (const m of pending) Object.assign(m, { deliveredAt: undefined, via: undefined, error: (e as Error).message });
    touch();
    throw e;
  }
  // 複製なら、元とは別のジョブの id になる。
  const next = (await backgroundedSession(output)) || sessionId;
  if (next === sessionId) return sessionId;
  moveSession(sessionId, next);
  // 元のセッションは複製に引き継いだので、Claude Code の一覧からも消す。消せなくても、ダッシュボードではアーカイブ済みになる。
  await removeSession(sessionId).catch((e: Error) => console.error(`${sessionId} を一覧から消せません: ${e.message}`));
  return next;
};

/**
 * 会話を引き継いだ別のセッションをバックグラウンドで起動して、依頼を渡す。起動したセッションの id を返す。
 * 元のセッションは止めずに残すので、作業中のセッションと端末で開いているセッションからも分岐できる。
 * テーマと、set_task で付けた JIRA の課題と、人間が登録したり外したりしたバッジは、分岐先にも付ける。名前は、ほかの新しいセッションと同じく、最初の応答の後に haiku に付けさせる。
 */
export const forkSession = async (sessionId: string, text: string, model?: string, effort?: string): Promise<string> => {
  const meta = state.sessions[sessionId];
  if (!meta) throw new Error(`unknown session: ${sessionId}`);
  const output = await claudeOutput(['--bg', '--resume', sessionId, '--fork-session', ...modelFlags(model, effort), '--', text], meta.cwd);
  const fork = await backgroundedSession(output);
  if (!fork) throw new Error('分岐したセッションの id を引けません');
  Object.assign(ensureSession(fork, meta.cwd), { theme: meta.theme, jira: [...meta.jira], badges: meta.badges, hiddenBadges: meta.hiddenBadges });
  state.outbox.push({ id: newId(), sessionId: fork, text, createdAt: Date.now(), deliveredAt: Date.now(), via: 'resume' });
  touch();
  return fork;
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
  const item = state.attention.find((a) => a.id === attentionId);
  // 分けた質問への回答には、どの質問への答えかが書いてある。
  return resolveAttention(attentionId, answer, item?.questions ? '質問への回答:' : `質問「${item?.body ?? ''}」への回答:`);
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
