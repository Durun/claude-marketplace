// 使用制限か、スリープや通信の途切れで止まったセッションを続けさせる。使用制限で止まったものは、制限が明けたら続けさせる。
// スリープや通信の途切れで止まったものは、人間が確認のダイアログで OK を押したら続けさせる。自動継続モードなら、人間に尋ねずに、通信が戻ったら続けさせる。
// 止まったことは伝えず、人間が続きを頼むときと同じ短い依頼だけを渡して、何も無かったかのように作業を続けさせる。
import path from 'node:path';
import { findTranscript, readLiveSessions, summarizeLog } from './claude.ts';
import { continueStopped } from './deliver.ts';
import { resumeImprover } from './improve.ts';
import { state, touch } from './store.ts';

// リセットの時刻ちょうどに頼むと、まだ断られることがあるので、少し待ってから続けさせる。
const AFTER_RESET_MS = 60_000;
// スリープから覚めた直後は、まだネットワークにつながっていないことが多い。つながるまで、この間隔で確かめ直す。
const OFFLINE_RETRY_MS = 30_000;
const PROBE_TIMEOUT_MS = 10_000;
const timers = new Map<string, NodeJS.Timeout>();

/** Claude Code が使う API のサーバーに届くか。HTTP で返事があれば、中身は問わない。 */
const reachable = (): Promise<boolean> =>
  fetch(process.env.ANTHROPIC_BASE_URL ?? 'https://api.anthropic.com', { method: 'HEAD', signal: AbortSignal.timeout(PROBE_TIMEOUT_MS) }).then(
    () => true,
    () => false,
  );

const logOf = (sessionId: string) => {
  const meta = state.sessions[sessionId];
  const transcript = meta && !meta.archived ? (meta.transcriptPath ?? findTranscript(sessionId)) : undefined;
  return transcript ? summarizeLog(transcript) : undefined;
};

/**
 * 会話ログが使用制限で終わっていれば、制限が明けるのを待ってから続けさせる。
 * スリープや通信の途切れで終わっていれば、自動継続モードのときだけ、API に届くようになってから続けさせる。自動継続モードでなければ、人間が answerOffline で答えるのを待つ。
 * 続けさせたものは止まった時刻で覚え、続きの依頼が会話ログに載るまでの間に呼ばれても、サーバーが再起動しても、2 度頼まない。
 * online は、API に届くと確かめた直後に渡す。
 */
export const checkLimit = (sessionId: string, online = false): void => {
  clearTimeout(timers.get(sessionId));
  timers.delete(sessionId);
  const meta = state.sessions[sessionId];
  const log = logOf(sessionId);
  const limit = log?.limit;
  const at = limit?.at ?? log?.offlineAt;
  if (!meta || !at || meta.limitResumedAt === at) return;
  const later = (ms: number): void => void timers.set(sessionId, setTimeout(() => checkLimit(sessionId), ms));
  if (limit) {
    const wait = limit.resetsAt + AFTER_RESET_MS - Date.now();
    if (wait > 0) return later(wait);
  } else if (!state.autoContinue) {
    return;
  } else if (!online) {
    // 動いていないセッションは continueStopped が起こさないので、通信が戻るのを待たない。
    if (!readLiveSessions().has(sessionId)) return;
    return void reachable().then((ok) => (ok ? checkLimit(sessionId, true) : later(OFFLINE_RETRY_MS)));
  }
  meta.limitResumedAt = at;
  touch();
  if (state.improve.run?.sessionId === sessionId) return resumeImprover();
  continueStopped(sessionId, meta.cwd).catch((e: Error) => console.error(`${sessionId} に続きを頼めません: ${e.message}`));
};

/** 起動したときに呼ぶ。サーバーが止まっている間に止まったセッションと、制限が明けたり通信が戻ったりしたセッションを拾う。 */
export const checkLimits = (): void => {
  for (const id of readLiveSessions().keys()) checkLimit(id);
  if (state.improve.run) checkLimit(state.improve.run.sessionId);
};

/**
 * スリープや通信の途切れで止まったセッションに、人間が確認のダイアログで答えたときに呼ぶ。resume が真なら続けさせる。
 * 答えた停止は止まった時刻で覚え、続けなかったときも、同じ停止ではもう尋ねない。
 */
export const answerOffline = async (sessionId: string, resume: boolean): Promise<void> => {
  const meta = state.sessions[sessionId];
  const at = logOf(sessionId)?.offlineAt;
  if (!meta || !at || meta.limitResumedAt === at) return;
  meta.limitResumedAt = at;
  // 止まったことは人間がダイアログで見たので、止まったと伝える最後の発言を、回答待ちとして要対応に出し直さない。
  meta.replySeenAt = Date.now();
  touch();
  if (resume) await continueStopped(sessionId, meta.cwd);
};

/** 自動継続モードを切り替える。入れたら、確認のダイアログで答えを待っていたセッションも、通信が戻りしだい続けさせる。 */
export const setAutoContinue = (on: boolean): void => {
  state.autoContinue = on;
  touch();
  if (on) checkLimits();
};

/** projects の下で変わったファイルのうち、セッションの会話ログ（プロジェクトのディレクトリの直下の .jsonl）だけを見る。 */
export const onTranscriptChange = (file: string): void => {
  if (path.extname(file) === '.jsonl' && path.dirname(path.dirname(file)) === '.') checkLimit(path.basename(file, '.jsonl'));
};
