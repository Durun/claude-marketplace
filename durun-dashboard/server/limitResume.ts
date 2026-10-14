// 使用制限で止まったセッションを、制限が明けたら続けさせる。
// 制限で止まったことは伝えず、人間が続きを頼むときと同じ短い依頼だけを渡して、制限が無かったかのように作業を続けさせる。
import path from 'node:path';
import { findTranscript, readLiveSessions, summarizeLog } from './claude.ts';
import { continueAfterLimit } from './deliver.ts';
import { resumeImprover } from './improve.ts';
import { state, touch } from './store.ts';

// リセットの時刻ちょうどに頼むと、まだ断られることがあるので、少し待ってから続けさせる。
const AFTER_RESET_MS = 60_000;
const timers = new Map<string, NodeJS.Timeout>();

/**
 * 会話ログが使用制限で終わっていれば、制限が明けるのを待ってから続けさせる。
 * 続けさせた制限は止まった時刻で覚え、続きの依頼が会話ログに載るまでの間に呼ばれても、サーバーが再起動しても、同じ制限で 2 度頼まない。
 */
export const checkLimit = (sessionId: string): void => {
  clearTimeout(timers.get(sessionId));
  timers.delete(sessionId);
  const meta = state.sessions[sessionId];
  const transcript = meta && !meta.archived ? (meta.transcriptPath ?? findTranscript(sessionId)) : undefined;
  const limit = transcript ? summarizeLog(transcript).limit : undefined;
  if (!meta || !limit || meta.limitResumedAt === limit.at) return;
  const wait = limit.resetsAt + AFTER_RESET_MS - Date.now();
  if (wait > 0) return void timers.set(sessionId, setTimeout(() => checkLimit(sessionId), wait));
  meta.limitResumedAt = limit.at;
  touch();
  if (state.improve.run?.sessionId === sessionId) return resumeImprover();
  continueAfterLimit(sessionId, meta.cwd).catch((e: Error) => console.error(`${sessionId} に続きを頼めません: ${e.message}`));
};

/** 起動したときに呼ぶ。サーバーが止まっている間に制限で止まったセッションと、制限が明けたセッションを拾う。 */
export const checkLimits = (): void => {
  for (const id of readLiveSessions().keys()) checkLimit(id);
  if (state.improve.run) checkLimit(state.improve.run.sessionId);
};

/** projects の下で変わったファイルのうち、セッションの会話ログ（プロジェクトのディレクトリの直下の .jsonl）だけを見る。 */
export const onTranscriptChange = (file: string): void => {
  if (path.extname(file) === '.jsonl' && path.dirname(path.dirname(file)) === '.') checkLimit(path.basename(file, '.jsonl'));
};
