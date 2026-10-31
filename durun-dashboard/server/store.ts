// ダッシュボード自身が持つ状態。セッションのテーマ付けと、人間と Claude の間でやり取りした内容を保存する。
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { type AskedQuestion, type Bookmark, type BranchView, type DiffRange, type DocFormat, type ImproveSettings, MEDIA_TYPES, type OutboxMessage, type PrThreadNote, type QueuedMessage, type RatePoint, type SavedDoc, type SavedFile, type SavedLink, type SavedProof, type SavedTerm } from '../shared/types.ts';

// 利用者ごとのデータはツールのリポジトリと分けてホームに置く。テストでは一時ディレクトリに向けて、実際の状態に触れないようにする。
export const DATA_DIR = process.env.DURUN_DASHBOARD_DATA_DIR ?? path.join(os.homedir(), '.durun-dashboard');
const STATE_FILE = path.join(DATA_DIR, 'state.json');
const SAVE_DELAY_MS = 200;
const KEEP_DELIVERED = 200;

export type SessionMeta = {
  id: string;
  cwd: string;
  transcriptPath?: string;
  theme?: string;
  title?: string;
  jira: string[];
  archived: boolean;
  /** 人間が作業中にアーカイブした。入力待ちか停止になったら、アーカイブする */
  archiveWhenIdle?: boolean;
  /** アーカイブしたが、Claude Code の一覧から消せなかった理由。残っている間は、後で消し直す */
  removeError?: string;
  /** 人間がピン留めした */
  pinned?: boolean;
  /** 人間がセッションのウィジェットでバッジに登録した、PR、Issue、JIRA の課題の URL */
  badges?: string[];
  /** 人間が外したバッジの URL。ブランチや set_task から見つけたものも、ここにあれば出さない */
  hiddenBadges?: string[];
  /** 人間が資料に足したリンク */
  links?: SavedLink[];
  /** 人間がエクスプローラーから資料に足したファイル */
  files?: SavedFile[];
  /** Claude が MCP の proofread で校正した、文章校正アプリのファイル */
  proofFiles?: SavedProof[];
  /** 人間がほかのセッションの資料から足した present の資料 */
  sharedDocs?: SavedDoc[];
  /** 人間が資料のウィジェットで並べ替えた順と、外した資料。present の資料は id、リンクは URL で持つ */
  docOrder?: string[];
  hiddenDocs?: string[];
  /** 人間が資料のウィジェットで付け直した題名。キーは docOrder と同じ */
  docTitles?: Record<string, string>;
  /** 人間が資料のウィジェットでピン留めした資料。キーは docOrder と同じ */
  pinnedDocs?: string[];
  /** 人間が用語集に足した語 */
  terms?: SavedTerm[];
  /** 人間が用語集から外した語。termKey の形で持ち、やり取りから拾い直しても出さない */
  hiddenTerms?: string[];
  /** 人間がチャットでブックマークした発言 */
  bookmarks?: Bookmark[];
  firstSeen: number;
  /** この時刻までの Claude の発言は人間が確認済み */
  replySeenAt: number;
  /** Claude が show_branches で並べた、コードの表示の選択肢 */
  branchViews?: BranchView[];
  // 以下は生きていたときの情報。終了後も一覧に名前やリンクを出すために残す。
  name?: string;
  kind?: string;
  entrypoint?: string;
  remoteUrl?: string;
  jobId?: string;
  startedAt?: number;
  /** 改善要望を実装するためにダッシュボードが起動したセッション */
  improver?: boolean;
  /** 巻き戻して送ったターンを動かす claude -p の pid。端末のセッションと同じに見えるが、ダッシュボードが起動したものなので止められる。 */
  printPid?: number;
  /** 使用制限か通信の途切れで止まったのを続けさせたか、続けないと人間が答えたときの、止まった時刻。同じもので 2 度続けさせず、2 度尋ねないために残す。 */
  limitResumedAt?: number;
};

export type StoredAttention = {
  id: string;
  kind: 'ask' | 'review' | 'command' | 'permission' | 'question';
  sessionId: string;
  title: string;
  body?: string;
  options?: string[];
  descriptions?: string[];
  questions?: AskedQuestion[];
  /** 権限の確認では Claude Code が使おうとしているツールの入力、コマンドの実行の依頼では実行するコマンド */
  detail?: string;
  /** レビュー依頼で、Claude が見てほしいコミットの範囲 */
  range?: DiffRange;
  createdAt: number;
};

export type StoredDoc = {
  id: string;
  sessionId: string;
  title: string;
  format: DocFormat;
  path?: string;
  content?: string;
  createdAt: number;
  seen: boolean;
};

export type ImproveRun = {
  sessionId: string;
  pid: number;
  /** このウェーブで計画を頼んだ要望のファイル名 */
  items?: string[];
  /** 画面に出す、対応中のウェーブの説明 */
  summary?: string;
  startedAt: number;
};

/** ダッシュボード自身で起きた想定外のエラー。同じエラーは 1 件にまとめて数える。item は起票した改善要望のファイル名。 */
export type ErrorRecord = {
  source: 'server' | 'client';
  message: string;
  stack?: string;
  /** サーバーなら API の経路、画面なら URL */
  where?: string;
  /** 遅い処理なら、かかった時間の最長（ミリ秒） */
  slowestMs?: number;
  /** 最長だった回のサーバーの、直近 1 分の load average。測れない OS では無い */
  slowestLoad?: number;
  count: number;
  firstAt: number;
  lastAt: number;
  /** 起票した回数 */
  filed: number;
  item?: string;
};

/** 今の枠の使用率の推移。resetsAt は枠の終わり（epoch ミリ秒）で、変わったら新しい枠なので点を捨てる。 */
export type RateWindow = { resetsAt: number; points: RatePoint[] };

type State = {
  sessions: Record<string, SessionMeta>;
  attention: StoredAttention[];
  // ponytail: 資料は削除しないので溜まり続ける。重くなったら古いものを消す。
  docs: StoredDoc[];
  outbox: OutboxMessage[];
  /** 人間が「キュー」で保留したメッセージ。送ったら outbox に移す。 */
  queue: QueuedMessage[];
  /** 既読にした派生の要対応（PR の状態など）の id */
  dismissed: string[];
  /** 改善要望の自動実装。model と effort は空なら Claude Code の設定に従う。error があれば待機する。 */
  improve: ImproveSettings & { run?: ImproveRun; error?: string };
  /** 自動継続モード。スリープや通信の途切れで止まったセッションを、人間に尋ねずに続けさせる。Claude のログインが切れて止まったセッションも、ログインが戻ったら続けさせる。 */
  autoContinue?: boolean;
  /** 人間がダッシュボードから Claude にログインし直した時刻。これより前にログインが切れて止まったセッションは、もうログインを尋ねない。 */
  claudeLoginAt?: number;
  /** エラーをまとめる鍵ごとの記録 */
  errors: Record<string, ErrorRecord>;
  /** PR のレビュースレッドに添えた分析。古いものから消す。 */
  prNotes: Record<string, PrThreadNote>;
  /** 人間がホームで並べ替えたセッションの順 */
  sessionOrder: string[];
  /** 人間がホームで並べ替えたテーマの順 */
  themeOrder: string[];
  /** 5 時間と 7 日の使用制限それぞれの、今の枠の使用率の推移 */
  rateWindows: { fiveHour?: RateWindow; sevenDay?: RateWindow };
};

const empty = (): State => ({ sessions: {}, attention: [], docs: [], outbox: [], queue: [], dismissed: [], improve: { enabled: false }, errors: {}, prNotes: {}, sessionOrder: [], themeOrder: [], rateWindows: {} });

const DOCS_DIR = path.resolve(DATA_DIR, 'docs');
// 画像と動画の資料はファイルを指すだけで、本文を書かない。
const DOC_EXT: Record<DocFormat, string> = { markdown: '.md', html: '.html', mermaid: '.mmd', svg: '.svg', text: '.txt', image: '', video: '' };

/** 資料の本文をファイルに書き、その絶対パスを返す。資料を添付されたメッセージを受け取った Claude が、そのパスから本文を読めるようにするため。 */
export const writeDocFile = (id: string, format: DocFormat, content: string): string => {
  const file = path.join(DOCS_DIR, `${id}${DOC_EXT[format]}`);
  fs.mkdirSync(DOCS_DIR, { recursive: true });
  fs.writeFileSync(file, content);
  return file;
};

/** 資料の形式を、ファイルの拡張子から決める。 */
export const inferFormat = (file: string | undefined): DocFormat => {
  const ext = file ? path.extname(file).toLowerCase() : '';
  const media = MEDIA_TYPES[ext];
  if (media) return media.startsWith('video/') ? 'video' : 'image';
  if (ext === '.html' || ext === '.htm') return 'html';
  if (ext === '.mmd' || ext === '.mermaid') return 'mermaid';
  if (ext === '.svg') return 'svg';
  if (ext === '.md' || ext === '.markdown' || !ext) return 'markdown';
  return 'text';
};

// 壊れたファイルを空の状態で上書きしないよう、読めないときは起動を止める。
// 本文を state.json に持つ資料は、本文をファイルに書き出し、そのファイルを指す資料にする。
const load = (): State => {
  if (!fs.existsSync(STATE_FILE)) return empty();
  const loaded: State = { ...empty(), ...JSON.parse(fs.readFileSync(STATE_FILE, 'utf8')) };
  for (const doc of loaded.docs) {
    if (doc.path || doc.content === undefined) continue;
    doc.path = writeDocFile(doc.id, doc.format, doc.content);
    delete doc.content;
  }
  return loaded;
};

export const state = load();

const listeners = new Set<() => void>();
let saveTimer: NodeJS.Timeout | undefined;

export const save = (): void => {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  const delivered = state.outbox.filter((m) => m.deliveredAt);
  if (delivered.length > KEEP_DELIVERED) {
    const drop = new Set(delivered.slice(0, delivered.length - KEEP_DELIVERED));
    state.outbox = state.outbox.filter((m) => !drop.has(m));
  }
  fs.mkdirSync(DATA_DIR, { recursive: true });
  const tmp = `${STATE_FILE}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(state, null, 2));
  fs.renameSync(tmp, STATE_FILE);
};

/** 表示に関わる情報が変わったことを知らせる。 */
export const changed = (): void => {
  for (const fn of listeners) fn();
};

/** 画面の更新を伴わない変更を保存する。 */
export const persist = (): void => {
  saveTimer ??= setTimeout(save, SAVE_DELAY_MS);
};

/** 状態を変えたら呼ぶ。保存を予約し、画面の更新を促す。 */
export const touch = (): void => {
  persist();
  changed();
};

export const onChange = (fn: () => void): void => {
  listeners.add(fn);
};

export const newId = (): string => randomUUID().slice(0, 8);

/**
 * 人間がセッションに付けた設定と、やり取りした内容を、会話を引き継いだ複製のセッションへ移す。元のセッションはアーカイブする。
 * モデルと effort を変えるときに、Claude Code が複製を別の id で起動するので使う。
 */
export const moveSession = (from: string, to: string): void => {
  const old = state.sessions[from];
  if (!old || from === to) return;
  const { theme, title, jira, pinned, links, files, proofFiles, sharedDocs, docOrder, hiddenDocs, docTitles, pinnedDocs, terms, hiddenTerms, replySeenAt, branchViews } = old;
  Object.assign(ensureSession(to, old.cwd), { theme, title, jira, pinned, links, files, proofFiles, sharedDocs, docOrder, hiddenDocs, docTitles, pinnedDocs, terms, hiddenTerms, replySeenAt, branchViews });
  Object.assign(old, { archived: true, pinned: undefined, jobId: undefined });
  for (const items of [state.attention, state.docs, state.outbox]) {
    for (const item of items) if (item.sessionId === from) item.sessionId = to;
  }
  state.sessionOrder = state.sessionOrder.map((id) => (id === from ? to : id));
  touch();
};

export const ensureSession = (id: string, cwd: string): SessionMeta => {
  const existing = state.sessions[id];
  if (existing) return existing;
  const meta: SessionMeta = { id, cwd, jira: [], archived: false, firstSeen: Date.now(), replySeenAt: 0 };
  state.sessions[id] = meta;
  persist();
  return meta;
};
