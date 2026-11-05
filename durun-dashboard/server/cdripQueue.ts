// CD取り込みのリストの解析、入れた順の割り当て、状態遷移、起動時の復旧。ファイルも外部コマンドも触らない純粋な関数だけを置く。
import type { CdRipItem, CdRipStatus } from '../shared/types.ts';

/** リストの 1 行。group は直前の ## 行。 */
export type ListEntry = { hint: string; group?: string };

const GROUP_LINE = /^##\s*(.*)$/;

/** 1 行 1 枚のリストを読む。## で始まる行は、次の ## までの行のグループ名。空行は飛ばす。 */
export const parseList = (text: string): ListEntry[] => {
  const entries: ListEntry[] = [];
  let group: string | undefined;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const g = GROUP_LINE.exec(line);
    if (g) {
      group = g[1]?.trim() || undefined;
      continue;
    }
    entries.push(group ? { hint: line, group } : { hint: line });
  }
  return entries;
};

export const newItem = (entry: ListEntry, id: string, now: number): CdRipItem => ({ id, hint: entry.hint, ...(entry.group ? { group: entry.group } : {}), status: 'waiting', createdAt: now, updatedAt: now });

/**
 * リストを反映する。受付の前は待機中の行を置き換え、受付中は末尾に足すだけにする（取り込み中の行の順番を動かさないため）。
 * 置き換えるときは、同じ位置に同じ手がかりの待機中の行があれば、その行（と id）を残す。
 */
export const applyList = (items: CdRipItem[], entries: ListEntry[], accepting: boolean, now: number, makeId: () => string): CdRipItem[] => {
  if (accepting) return [...items, ...entries.map((e) => newItem(e, makeId(), now))];
  const waiting = items.filter((i) => i.status === 'waiting');
  const kept = items.filter((i) => i.status !== 'waiting');
  const next = entries.map((e, k) => {
    const old = waiting[k];
    return old && old.hint === e.hint && old.group === e.group ? old : newItem(e, makeId(), now);
  });
  return [...kept, ...next];
};

/** 次に取り込む行。リストの上から数えて最初の待機中。 */
export const nextWaiting = (items: CdRipItem[]): CdRipItem | undefined => items.find((i) => i.status === 'waiting');

/** 状態を変える出来事。 */
export type CdRipEvent =
  | { type: 'start' }
  | { type: 'ripped'; researched: boolean }
  | { type: 'ripFailed'; error: string }
  | { type: 'abort' }
  | { type: 'researched'; attention?: string }
  | { type: 'export' }
  | { type: 'exported'; files: string[] }
  | { type: 'exportFailed'; error: string }
  | { type: 'retry' }
  | { type: 'skip' };

// 設計書の状態機械。ripping で研究が終わる出来事（researched）は、状態を変えずに記録だけする。
const ALLOWED: Record<CdRipEvent['type'], CdRipStatus[]> = {
  start: ['waiting'],
  ripped: ['ripping'],
  ripFailed: ['ripping'],
  abort: ['ripping'],
  researched: ['ripping', 'researching'],
  export: ['review'],
  exported: ['exporting'],
  exportFailed: ['exporting'],
  retry: ['review', 'failed'],
  skip: ['waiting', 'researching', 'review', 'failed'],
};

export class TransitionError extends Error {}

/** 出来事を受けた後の行。許されない状態から呼んだら TransitionError。 */
export const transition = (item: CdRipItem, event: CdRipEvent, now: number): CdRipItem => {
  if (!ALLOWED[event.type].includes(item.status)) throw new TransitionError(`${item.status} の行に ${event.type} はできません`);
  const base = { ...item, updatedAt: now };
  const { attention: _drop, ...clean } = base;
  switch (event.type) {
    case 'start':
      return { ...clean, status: 'ripping', progress: undefined };
    case 'ripped':
      return { ...clean, status: event.researched ? 'review' : 'researching', progress: undefined };
    case 'ripFailed':
      return { ...clean, status: 'failed', attention: event.error, progress: undefined };
    case 'abort':
    case 'retry':
      return { ...clean, status: 'waiting', progress: undefined, disc: undefined };
    case 'researched':
      return item.status === 'ripping' ? { ...base, attention: event.attention ?? item.attention } : { ...clean, status: 'review', ...(event.attention ? { attention: event.attention } : {}) };
    case 'export':
      return { ...clean, status: 'exporting' };
    case 'exported':
      return { ...clean, status: 'done', exported: { at: now, files: event.files } };
    case 'exportFailed':
      return { ...clean, status: 'review', attention: event.error };
    case 'skip':
      return { ...clean, status: 'skipped' };
  }
};

/** 起動時に取り込み先を調べた結果。flacs は disc/ の FLAC の数、logged は取り込みの最後に書かれる rip.log があるか。 */
export type StageProbe = { flacs: number; logged: boolean };

const STOPPED_RIP = 'ダッシュボードが止まったため取り込みを中断しました。取り込みをやり直してください';
const STOPPED_EXPORT = 'ダッシュボードが止まったため書き出しを中断しました。書き出し先に途中のファイルが残っていれば消して、もう一度書き出してください';

/**
 * 止まったままの行を、取り込み先の中身で判定し直す。
 * 取り込み中だった行は、曲数どおりの FLAC と rip.log がそろっていれば取り込み済みとして先へ進め、そろっていなければ失敗にする。
 * 書き出し中だった行は、途中のファイルが残りうるので確認待ちに戻して知らせる。
 */
export const recover = (items: CdRipItem[], probe: (item: CdRipItem) => StageProbe, now: number): CdRipItem[] =>
  items.map((item) => {
    if (item.status === 'ripping') {
      const { flacs, logged } = probe(item);
      const complete = logged && !!item.disc?.tracks && flacs === item.disc.tracks;
      if (complete) return { ...item, status: item.research?.finishedAt ? 'review' : 'researching', progress: undefined, updatedAt: now };
      return { ...item, status: 'failed', attention: STOPPED_RIP, progress: undefined, updatedAt: now };
    }
    if (item.status === 'exporting') return { ...item, status: 'review', attention: STOPPED_EXPORT, updatedAt: now };
    return item;
  });
