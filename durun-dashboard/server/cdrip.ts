// アプリ「CD取り込み」の中枢。設定とキューを読み書きし、状態を SSE でウィンドウへ流し、API の入口になる。
// 取り込み・情報収集・書き出しの中身は cdrip*.ts に分け、ここはそれらを状態機械につなぐ。
import fs from 'node:fs';
import type { ServerResponse } from 'node:http';
import path from 'node:path';
import type { CdFeatures, CdToolName, CdLogEntry, CdRipItem, CdRipSettings, CdRipView, CdTool } from '../shared/types.ts';
import { applyList, parseList, recover } from './cdripQueue.ts';
import { detectTools, installTool, knownOffset, TOOL_NAMES } from './cdripTools.ts';
import { platform } from './platform.ts';
import { DATA_DIR } from './store.ts';

/** 入力の誤り（400）や、今はできない操作（409）を API の応答の状態に写す。 */
export class CdRipError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

const DIR = path.join(DATA_DIR, 'cdrip');
const SETTINGS_FILE = path.join(DIR, 'settings.json');
const QUEUE_FILE = path.join(DIR, 'queue.json');
const LOG_FILE = path.join(DIR, 'log.jsonl');
const SAVE_DELAY_MS = 200;
const KEEPALIVE_MS = 20_000;
const LOG_SHOWN = 200;

export const DEFAULT_FEATURES: CdFeatures = {
  flacImage: true,
  mp3: true,
  mp3Bitrate: '320k',
  embedCover: true,
  mp3Gain: false,
  ai: true,
  aiVideo: false,
  autoExport: false,
  rekordbox: true,
  verify: 0,
};

type Queue = { accepting: boolean; items: CdRipItem[] };

let settings: CdRipSettings | undefined;
let queue: Queue = { accepting: false, items: [] };
let log: CdLogEntry[] = [];
let tools: CdTool[] = [];
let drives: CdRipView['drives'] = [];
let media: CdRipView['drive']['media'] = 'unknown';
let saveTimer: NodeJS.Timeout | undefined;
const windows = new Set<ServerResponse>();

const readJson = <T,>(file: string): T | undefined => {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8')) as T;
  } catch {
    // 無いか壊れていれば、無いものとして作り直す。
    return undefined;
  }
};

const writeJson = (file: string, value: unknown): void => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(value, null, 2));
  fs.renameSync(tmp, file);
};

export const saveCdRip = (): void => {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  if (settings) writeJson(QUEUE_FILE, queue);
};

const scheduleSave = (): void => {
  saveTimer ??= setTimeout(saveCdRip, SAVE_DELAY_MS).unref();
};

/** 設定の書き込み値を、型が合うものだけ取り込んで current に重ねる。 */
export const mergeSettings = (current: CdRipSettings, patch: unknown): CdRipSettings => {
  if (typeof patch !== 'object' || patch === null) throw new CdRipError(400, '設定はオブジェクトで渡してください');
  const p = patch as Record<string, unknown>;
  const str = (key: string): string | undefined => {
    const v = p[key];
    if (v === undefined) return undefined;
    if (typeof v !== 'string') throw new CdRipError(400, `${key} は文字列で渡してください`);
    return v;
  };
  const num = (key: string, min: number, max: number): number | undefined => {
    const v = p[key];
    if (v === undefined) return undefined;
    if (typeof v !== 'number' || !Number.isInteger(v) || v < min || v > max) throw new CdRipError(400, `${key} は ${min} から ${max} の整数で渡してください`);
    return v;
  };
  const dest = (p.dest ?? {}) as Record<string, unknown>;
  for (const k of ['cd', 'club', 'music'] as const) if (dest[k] !== undefined && (typeof dest[k] !== 'string' || !dest[k])) throw new CdRipError(400, `dest.${k} は文字列で渡してください`);
  return {
    drive: str('drive') ?? current.drive,
    driveModel: str('driveModel') ?? current.driveModel,
    offset: num('offset', -3000, 3000) ?? current.offset,
    paranoia: num('paranoia', 0, 3) ?? current.paranoia,
    musicDir: str('musicDir') ?? current.musicDir,
    stageDir: str('stageDir') ?? current.stageDir,
    dest: { cd: (dest.cd as string | undefined) ?? current.dest.cd, club: (dest.club as string | undefined) ?? current.dest.club, music: (dest.music as string | undefined) ?? current.dest.music },
    features: mergeFeatures(current.features, p.features),
  };
};

const BITRATES = ['320k', '256k', 'V0'];
const FLAGS = ['flacImage', 'mp3', 'embedCover', 'mp3Gain', 'ai', 'aiVideo', 'autoExport', 'rekordbox'] as const;

const mergeFeatures = (current: CdFeatures, patch: unknown): CdFeatures => {
  if (patch === undefined) return current;
  if (typeof patch !== 'object' || patch === null) throw new CdRipError(400, 'features はオブジェクトで渡してください');
  const p = patch as Record<string, unknown>;
  const next = { ...current };
  for (const k of FLAGS) {
    if (p[k] === undefined) continue;
    if (typeof p[k] !== 'boolean') throw new CdRipError(400, `features.${k} は真偽値で渡してください`);
    next[k] = p[k];
  }
  if (p.mp3Bitrate !== undefined) {
    if (typeof p.mp3Bitrate !== 'string' || !BITRATES.includes(p.mp3Bitrate)) throw new CdRipError(400, `features.mp3Bitrate は ${BITRATES.join(' / ')} のどれかで渡してください`);
    next.mp3Bitrate = p.mp3Bitrate as CdFeatures['mp3Bitrate'];
  }
  if (p.verify !== undefined) {
    if (typeof p.verify !== 'number' || !Number.isInteger(p.verify) || p.verify < 0 || p.verify > 10) throw new CdRipError(400, 'features.verify は 0 から 10 の整数で渡してください');
    next.verify = p.verify;
  }
  return next;
};

/** 保存した設定が無い PC のために、OS からドライブと音楽フォルダーを調べて初期値を作る。 */
const inferSettings = async (): Promise<CdRipSettings> => {
  const [found, musicDir] = await Promise.all([platform.opticalDrives(), platform.musicDir()]);
  drives = found;
  const first = found[0];
  const cd = path.join(musicDir, 'CD');
  return {
    drive: first?.id ?? '',
    driveModel: first?.model ?? '',
    offset: first ? (knownOffset(first.model) ?? 0) : 0,
    paranoia: 3,
    musicDir,
    stageDir: path.join(cd, '_rip'),
    dest: { cd, club: path.join(musicDir, 'CLUB'), music: path.join(musicDir, 'Music') },
    features: { ...DEFAULT_FEATURES },
  };
};

const readLog = (): CdLogEntry[] => {
  try {
    return fs
      .readFileSync(LOG_FILE, 'utf8')
      .split('\n')
      .filter(Boolean)
      .slice(-LOG_SHOWN)
      .map((l) => JSON.parse(l) as CdLogEntry);
  } catch {
    return [];
  }
};

/** ログ 1 件を足す。ファイルには全部残し、画面には末尾の LOG_SHOWN 件だけ送る。 */
export const logCdRip = (kind: string, text: string, itemId?: string): void => {
  const entry: CdLogEntry = { at: Date.now(), kind, text, ...(itemId ? { itemId } : {}) };
  log = [...log, entry].slice(-LOG_SHOWN);
  try {
    fs.mkdirSync(DIR, { recursive: true });
    fs.appendFileSync(LOG_FILE, `${JSON.stringify(entry)}\n`);
  } catch (e) {
    console.error(`CD取り込みのログを書けません: ${(e as Error).message}`);
  }
  broadcast();
};

let ready: Promise<void> | undefined;

/** 設定とキューを読む。止まったまま残った行は、取り込み先の中身で判定し直す。最初の 1 回だけ動く。 */
export const initCdRip = (): Promise<void> => {
  ready ??= (async () => {
    settings = await inferSettings();
    const saved = readJson<Partial<CdRipSettings>>(SETTINGS_FILE);
    if (saved) settings = mergeSettings(settings, saved);
    else writeJson(SETTINGS_FILE, settings);
    log = readLog();
    const storedQueue = readJson<Queue>(QUEUE_FILE);
    if (storedQueue) queue = { accepting: !!storedQueue.accepting, items: recover(storedQueue.items ?? [], probeStage, Date.now()) };
    await refreshTools();
  })();
  return ready;
};

const current = (): CdRipSettings => {
  if (!settings) throw new CdRipError(503, 'CD取り込みの準備中です');
  return settings;
};

/** 取り込み先に残った FLAC の数と、取り込みの最後に書かれる rip.log の有無。 */
const probeStage = (item: CdRipItem): { flacs: number; logged: boolean } => {
  const disc = path.join(current().stageDir, item.id, 'disc');
  try {
    return { flacs: fs.readdirSync(disc).filter((f) => f.endsWith('.flac')).length, logged: fs.existsSync(path.join(disc, 'rip.log')) };
  } catch {
    return { flacs: 0, logged: false };
  }
};

const refreshTools = async (): Promise<void> => {
  const next = await detectTools(current().features.aiVideo);
  if (JSON.stringify(next) !== JSON.stringify(tools)) {
    tools = next;
    broadcast();
  }
};

const driveMessage = (s: CdRipSettings): string | undefined => {
  if (!s.drive) return '光学ドライブが見つかりません。設定でドライブを選んでください';
  if (s.offset === 0 && knownOffset(s.driveModel) === undefined) return 'このドライブの読み取りオフセットが表に無いので 0 で取り込みます。AccurateRip の表で調べて、設定に書いてください';
  return undefined;
};

export const view = (): CdRipView => {
  const s = current();
  return { tools, drive: { id: s.drive, model: s.driveModel, offset: s.offset, media, message: driveMessage(s) }, accepting: queue.accepting, settings: s, items: queue.items, log, drives };
};

let broadcastTimer: NodeJS.Timeout | undefined;

/** 続けて変わっても 1 回にまとめて、ウィンドウへ状態を送る。 */
export const broadcast = (): void => {
  if (!settings || broadcastTimer) return;
  broadcastTimer = setTimeout(() => {
    broadcastTimer = undefined;
    const data = `event: view\ndata: ${JSON.stringify(view())}\n\n`;
    for (const res of windows) res.write(data);
  }, 30);
  broadcastTimer.unref();
};

/** 状態を更新して、保存と配信を予約する。 */
export const mutateItems = (fn: (items: CdRipItem[]) => CdRipItem[]): void => {
  queue = { ...queue, items: fn(queue.items) };
  scheduleSave();
  broadcast();
};

export const itemsNow = (): CdRipItem[] => queue.items;
export const settingsNow = (): CdRipSettings => current();
export const setMedia = (next: CdRipView['drive']['media']): void => {
  if (media === next) return;
  media = next;
  broadcast();
};

/** ウィンドウからの接続。最後のウィンドウが切れても、受付中の見張りは止めない。 */
export const streamCdRip = async (res: ServerResponse): Promise<void> => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  windows.add(res);
  const keepalive = setInterval(() => res.write(': keepalive\n\n'), KEEPALIVE_MS);
  res.on('close', () => {
    clearInterval(keepalive);
    windows.delete(res);
  });
  await initCdRip();
  res.write(`event: view\ndata: ${JSON.stringify(view())}\n\n`);
};

let lastId = 0;
const makeId = (): string => {
  const d = new Date();
  const p = (n: number, w = 2): string => String(n).padStart(w, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}-${(lastId = (lastId + 1) % 100)}`;
};

export const setCdRipList = async (text: string): Promise<void> => {
  await initCdRip();
  mutateItems((items) => applyList(items, parseList(text), queue.accepting, Date.now(), makeId));
};

export const setCdRipAccepting = async (on: boolean): Promise<void> => {
  await initCdRip();
  if (on && !current().drive) throw new CdRipError(409, '光学ドライブが見つかりません。設定でドライブを選んでください');
  queue = { ...queue, accepting: on };
  logCdRip('accepting', on ? '受付を開始しました' : '受付を停止しました');
  scheduleSave();
  broadcast();
};

export const saveCdRipSettings = async (patch: unknown): Promise<void> => {
  await initCdRip();
  settings = mergeSettings(current(), patch);
  writeJson(SETTINGS_FILE, settings);
  await refreshTools();
  broadcast();
};

/** 画面の導入ボタンから、依存ツールをパッケージ管理で入れる。管理者権限が要るものは、実行せずコマンドを返す。 */
export const installCdTool = async (name: string): Promise<{ message: string }> => {
  await initCdRip();
  if (!TOOL_NAMES.includes(name as CdToolName)) throw new CdRipError(400, `知らないツールです: ${name}`);
  try {
    const { message, ran } = await installTool(name as CdToolName);
    logCdRip('tool', ran ? `${name} を入れました` : `${name} は手で入れてください`);
    return { message };
  } catch (e) {
    throw new CdRipError(502, `${name} を入れられませんでした: ${(e as Error).message.split('\n')[0]}`);
  } finally {
    await refreshTools();
  }
};
