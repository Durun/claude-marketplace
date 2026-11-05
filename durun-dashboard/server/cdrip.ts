// アプリ「CD取り込み」の中枢。設定とキューを読み書きし、状態を SSE でウィンドウへ流し、API の入口になる。
// 取り込み・情報収集・書き出しの中身は cdrip*.ts に分け、ここはそれらを状態機械につなぐ。
import fs from 'node:fs';
import type { ServerResponse } from 'node:http';
import path from 'node:path';
import type { CdFeatures, CdMeta, CdToolName, CdLogEntry, CdRipItem, CdRipSettings, CdRipView, CdTool } from '../shared/types.ts';
import { appendAdded, type ExportTools, exportDisc } from './cdripExport.ts';
import { fetchCover, matchTrackCount, parseResearch, researchPrompt, researchSchema, skeletonMeta, validateMeta } from './cdripResearch.ts';
import { rekordboxEntries, rekordboxXml } from './cdripRekordbox.ts';
import { askResearcher } from './haiku.ts';
import { applyList, nextWaiting, parseList, recover, transition } from './cdripQueue.ts';
import { cyanripArgs, listFlacs, type ParsedRip, type RipResult, startRip, trackSamples } from './cdripRip.ts';
import { detectTools, installTool, knownOffset, resolveTool, TOOL_NAMES } from './cdripTools.ts';
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
    // 情報収集の途中で止まった行は、頼み直す。
    for (const i of queue.items) if (i.status === 'researching') startResearch(i.id);
    if (queue.accepting) ensureWatching();
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
  return { tools, drive: { id: s.drive, model: s.driveModel, offset: s.offset, media, message: note ?? driveMessage(s) }, accepting: queue.accepting, settings: s, items: queue.items, log, drives };
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
  ensureWatching();
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
  if (on) ensureWatching();
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

// ─── 取り込み ───────────────────────────────────────────────────────────

const WATCH_MS = 3000;
const CD_SAMPLE_RATE = 44100;

let note: string | undefined;
let watchTimer: NodeJS.Timeout | undefined;
let ticking = false;
let active: { itemId: string; abort: () => void } | undefined;
// 排出したか中断したディスクが抜かれるまでは、次の取り込みを始めない。同じ CD を二重に取り込まないため。
let untilRemoved = false;

const setNote = (next: string | undefined): void => {
  if (note === next) return;
  note = next;
  broadcast();
};

const updateItem = (id: string, fn: (i: CdRipItem) => CdRipItem): void => mutateItems((items) => items.map((i) => (i.id === id ? fn(i) : i)));

const findItem = (id: string): CdRipItem => {
  const item = queue.items.find((i) => i.id === id);
  if (!item) throw new CdRipError(404, `行がありません: ${id}`);
  return item;
};

const eject = async (): Promise<void> => {
  await platform.ejectDisc(current().drive);
  untilRemoved = true;
};

/** 取り込みが終わった後の始末。曲数どおりの FLAC がそろっていれば取り込み済み、そうでなければ失敗にする。 */
const finishRip = async (item: CdRipItem, dir: string, result: RipResult): Promise<void> => {
  if (result.aborted) {
    updateItem(item.id, (i) => transition(i, { type: 'abort' }, Date.now()));
    logCdRip('rip', '取り込みを中断しました。ディスクを出して入れ直すと、次の取り込みを始めます', item.id);
    untilRemoved = true;
    return;
  }
  const discDir = path.join(dir, 'disc');
  const flacs = fs.existsSync(discDir) ? listFlacs(discDir) : [];
  const tracks = result.parsed.tracks ?? 0;
  const failure = result.parsed.error ?? (result.code !== 0 ? `cyanrip が終了コード ${result.code} で終わりました（${path.join(dir, 'cyanrip.out')} を見てください）` : flacs.length !== tracks || !tracks ? `${tracks} 曲のはずが ${flacs.length} 曲しか取り込めていません` : undefined);
  if (failure) {
    updateItem(item.id, (i) => transition(i, { type: 'ripFailed', error: failure }, Date.now()));
    logCdRip('rip', `取り込みに失敗しました: ${failure}`, item.id);
  } else {
    // 曲の長さの確定値は、取り込んだ FLAC のサンプル数から求める。
    const metaflac = resolveTool('metaflac');
    const durations = metaflac ? (await trackSamples(metaflac, flacs)).map((n) => Math.round((n / CD_SAMPLE_RATE) * 1000) / 1000) : result.parsed.durations;
    const ai = current().features.ai;
    updateItem(item.id, (i) => {
      const disc = { ...(i.disc ?? { tracks }), tracks, durations };
      const meta = i.meta ?? skeletonMeta(disc);
      const next = transition({ ...i, disc, meta }, { type: 'ripped', researched: !ai || !!i.research?.finishedAt }, Date.now());
      // 曲数の不一致と、取り込み中に届いた情報収集の注意は、状態が変わっても残す。
      const attention = matchTrackCount(meta, tracks) ?? i.attention;
      return attention ? { ...next, attention } : next;
    });
    if (ai && !queue.items.find((i) => i.id === item.id)?.research?.startedAt) startResearch(item.id);
    logCdRip('rip', `取り込みを終えました（${tracks} 曲）`, item.id);
  }
  await eject();
};

/** cyanrip が読み取った情報を、行の disc と progress に写す。変わらなければ配信しない。 */
const applyParsed = (id: string, p: ParsedRip): void => {
  updateItem(id, (i) => {
    const disc = p.tracks === undefined ? i.disc : { tracks: p.tracks, durations: p.durations, totalSeconds: p.totalSeconds, cdText: p.cdText, tocId: p.tocId };
    const progress = p.tracks !== undefined && p.progress ? { track: p.progress.track, total: p.tracks } : i.progress;
    if (JSON.stringify([disc, progress]) === JSON.stringify([i.disc, i.progress])) return i;
    return { ...i, disc, progress, updatedAt: Date.now() };
  });
  // 曲数が分かった時点で、取り込みを待たずに情報収集を始める。
  if (p.tracks !== undefined && current().features.ai && !queue.items.find((i) => i.id === id)?.research) startResearch(id);
};

const ripNext = async (item: CdRipItem): Promise<void> => {
  const s = current();
  const cyanrip = resolveTool('cyanrip');
  if (!cyanrip) return setNote('cyanrip が見つかりません。上の導入ボタンで入れてください');
  setNote(undefined);
  const dir = path.join(s.stageDir, item.id);
  fs.rmSync(path.join(dir, 'disc'), { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  updateItem(item.id, (i) => transition(i, { type: 'start' }, Date.now()));
  logCdRip('rip', `取り込みを始めます: ${item.hint}`, item.id);
  const rip = startRip(cyanrip, cyanripArgs(s, await platform.cdDevice(s.drive)), dir, (p) => applyParsed(item.id, p));
  active = { itemId: item.id, abort: rip.abort };
  try {
    await finishRip(item, dir, await rip.done);
  } catch (e) {
    updateItem(item.id, (i) => (i.status === 'ripping' ? transition(i, { type: 'ripFailed', error: (e as Error).message }, Date.now()) : i));
    logCdRip('rip', `取り込みの後始末に失敗しました: ${(e as Error).message}`, item.id);
  } finally {
    active = undefined;
  }
};

/** 3 秒ごとにディスクを見て、受付中で待機中の行があれば取り込む。 */
const tick = async (): Promise<void> => {
  if (ticking || !settings) return;
  if (!queue.accepting && !windows.size && !active) return;
  ticking = true;
  try {
    const loaded = await platform.mediaLoaded(settings.drive);
    setMedia(loaded === undefined ? 'unknown' : loaded ? 'loaded' : 'none');
    if (loaded === false) untilRemoved = false;
    if (active || !queue.accepting || loaded !== true || untilRemoved) return;
    const next = nextWaiting(queue.items);
    setNote(next ? undefined : 'ディスクが入っていますが、待機中の行がありません。リストに足してください');
    if (next) await ripNext(next);
  } catch (e) {
    console.error(`CD取り込みの見張りが失敗しました: ${(e as Error).message}`);
  } finally {
    ticking = false;
  }
};

/** 見張りを始める。最後のウィンドウが閉じても止めない。サーバーが止まるまで 1 本だけ動く。 */
const ensureWatching = (): void => {
  watchTimer ??= setInterval(() => void tick(), WATCH_MS).unref();
};

export const skipCdItem = async (id: string): Promise<void> => {
  await initCdRip();
  findItem(id);
  updateItem(id, (i) => transition(i, { type: 'skip' }, Date.now()));
};

/** 取り込みをやり直す。取り込み済みの FLAC は消して、待機中に戻す。 */
export const retryCdItem = async (id: string): Promise<void> => {
  await initCdRip();
  const item = findItem(id);
  if (!['review', 'failed'].includes(item.status)) throw new CdRipError(409, `${item.status} の行はやり直せません`);
  fs.rmSync(path.join(current().stageDir, id, 'disc'), { recursive: true, force: true });
  updateItem(id, (i) => transition(i, { type: 'retry' }, Date.now()));
};

export const abortCdItem = async (id: string): Promise<void> => {
  await initCdRip();
  if (active?.itemId !== id) throw new CdRipError(409, 'この行は取り込み中ではありません');
  active.abort();
};

export const ejectCd = async (): Promise<void> => {
  await initCdRip();
  if (active) throw new CdRipError(409, '取り込み中は排出できません');
  await platform.ejectDisc(current().drive);
};

// ─── 確認と書き出し ─────────────────────────────────────────────────────

/** 行のフォルダー。書き出し済みの行は _done に移してあるので、なければそちらを見る。 */
const itemDir = (id: string): string => {
  const live = path.join(current().stageDir, id);
  return fs.existsSync(live) ? live : path.join(current().stageDir, '_done', id);
};

/** 曲情報を保存する。状態は変えず、曲数が CD と合わなければ attention に残す。 */
export const saveCdMeta = async (id: string, body: unknown): Promise<void> => {
  await initCdRip();
  const item = findItem(id);
  if (!['review', 'researching'].includes(item.status)) throw new CdRipError(409, `${item.status} の行の曲情報は直せません`);
  let meta: CdMeta;
  try {
    meta = validateMeta((body as { meta?: unknown } | undefined)?.meta);
  } catch (e) {
    throw new CdRipError(400, (e as Error).message);
  }
  const mismatch = matchTrackCount(meta, item.disc?.tracks);
  updateItem(id, (i) => {
    const { attention: _old, ...rest } = i;
    return { ...rest, meta, updatedAt: Date.now(), ...(mismatch ? { attention: mismatch } : {}) };
  });
};

const exportTools = (): ExportTools => {
  const found = { flac: resolveTool('flac'), metaflac: resolveTool('metaflac'), ffmpeg: resolveTool('ffmpeg') };
  const missing = Object.entries(found).filter(([, p]) => !p).map(([n]) => n);
  if (missing.length) throw new CdRipError(409, `${missing.join('、')} が見つかりません。上の導入ボタンで入れてください`);
  return found as ExportTools;
};

const runExport = async (item: CdRipItem, meta: CdMeta): Promise<void> => {
  const s = current();
  const dir = path.join(s.stageDir, item.id);
  try {
    const { files, rows } = await exportDisc({ dir, meta, settings: s, tools: exportTools() });
    appendAdded(path.join(s.stageDir, 'added.tsv'), rows);
    // 取り込み途中のフォルダーは、書き出したら _done に移す。移せなくても書き出しは済んでいる。
    try {
      fs.mkdirSync(path.join(s.stageDir, '_done'), { recursive: true });
      fs.renameSync(dir, path.join(s.stageDir, '_done', item.id));
    } catch (e) {
      logCdRip('export', `取り込み先のフォルダーを _done に移せませんでした: ${(e as Error).message}`, item.id);
    }
    updateItem(item.id, (i) => transition(i, { type: 'exported', files }, Date.now()));
    logCdRip('export', `書き出しました: ${meta.circle} / ${meta.album}（${files.length} ファイル）`, item.id);
  } catch (e) {
    const message = (e as Error).message.split('\n')[0] ?? '書き出しに失敗しました';
    updateItem(item.id, (i) => transition(i, { type: 'exportFailed', error: message }, Date.now()));
    logCdRip('export', `書き出しに失敗しました: ${message}`, item.id);
  }
};

/** 確認待ちの行を書き出す。書き出しは裏で進み、結果は行の状態で知らせる。 */
export const exportCdItem = async (id: string): Promise<void> => {
  await initCdRip();
  const item = findItem(id);
  if (item.status !== 'review') throw new CdRipError(409, `${item.status} の行は書き出せません`);
  if (!item.meta) throw new CdRipError(400, '曲情報がありません');
  if (pendingResearch.has(id)) throw new CdRipError(409, 'AI が情報を集めています。終わってから書き出してください');
  const mismatch = matchTrackCount(item.meta, item.disc?.tracks);
  if (mismatch) throw new CdRipError(400, mismatch);
  const { features } = current();
  if (!features.flacImage && !features.mp3) throw new CdRipError(409, '設定で FLAC イメージと mp3 のどちらも切ってあります');
  exportTools();
  const meta = item.meta;
  updateItem(id, (i) => transition(i, { type: 'export' }, Date.now()));
  void runExport(item, meta);
};

/** ジャケットの画像を URL から取り込み、行のフォルダーに cover.<拡張子> として置いて、曲情報に結びつける。 */
export const setCdCover = async (id: string, body: unknown): Promise<void> => {
  await initCdRip();
  const item = findItem(id);
  if (!item.meta) throw new CdRipError(409, '曲情報がまだありません');
  const url = (body as { url?: unknown } | undefined)?.url;
  if (typeof url !== 'string') throw new CdRipError(400, 'url は文字列で渡してください');
  let image: { bytes: Buffer; ext: string };
  try {
    image = await fetchCover(url);
  } catch (e) {
    throw new CdRipError(502, `ジャケットを取り込めません: ${(e as Error).message}`);
  }
  const dir = path.join(current().stageDir, id);
  for (const old of fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => /^cover\./.test(f)) : []) fs.rmSync(path.join(dir, old), { force: true });
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, `cover.${image.ext}`), image.bytes);
  updateItem(id, (i) => (i.meta ? { ...i, meta: { ...i.meta, cover: `cover.${image.ext}`, coverUrl: url }, updatedAt: Date.now() } : i));
};

export const sendCdCover = (res: ServerResponse, id: string): void => {
  const item = findItem(id);
  const file = item.meta?.cover ? path.join(itemDir(id), item.meta.cover) : undefined;
  if (!file || !fs.existsSync(file)) throw new CdRipError(404, 'ジャケットがありません');
  const ext = path.extname(file).slice(1).toLowerCase();
  res.writeHead(200, { 'content-type': `image/${ext === 'jpg' ? 'jpeg' : ext}`, 'cache-control': 'no-cache' }).end(fs.readFileSync(file));
};

// ─── AI の情報収集 ──────────────────────────────────────────────────────

// 同時に動かす情報収集は、行ごとに 1 つ。同じ行を重ねて頼まない。
const pendingResearch = new Map<string, Promise<void>>();

const writeCover = (id: string, image: { bytes: Buffer; ext: string }): string => {
  const dir = path.join(current().stageDir, id);
  fs.mkdirSync(dir, { recursive: true });
  for (const old of fs.readdirSync(dir).filter((f) => /^cover\./.test(f))) fs.rmSync(path.join(dir, old), { force: true });
  fs.writeFileSync(path.join(dir, `cover.${image.ext}`), image.bytes);
  return `cover.${image.ext}`;
};

/** 答えを行に反映する。取り込み中なら状態は変えず、情報収集中なら確認待ちに進める。確信が高く曲数が合えば、設定しだいで自動で書き出す。 */
const applyResearch = (id: string, result: { meta?: CdMeta; error?: string; coverError?: string }): void => {
  const now = Date.now();
  updateItem(id, (i) => {
    const mismatch = result.meta ? matchTrackCount(result.meta, i.disc?.tracks) : undefined;
    const attention = result.error ? `AI の情報収集に失敗しました: ${result.error}` : (mismatch ?? result.coverError);
    const research = { ...(i.research ?? { startedAt: now, sources: [] }), finishedAt: now, confidence: result.meta?.confidence, sources: result.meta?.sources ?? [], notes: result.meta?.notes, error: result.error };
    const next: CdRipItem = { ...i, research, meta: result.meta ?? i.meta, updatedAt: now };
    if (i.status === 'ripping' || i.status === 'researching') return transition(next, { type: 'researched', attention }, now);
    const { attention: _old, ...rest } = next;
    return attention ? { ...rest, attention } : rest;
  });
  const item = queue.items.find((i) => i.id === id);
  if (item?.status === 'review' && !item.attention && item.meta?.confidence === 'high' && current().features.autoExport) {
    exportCdItem(id).catch((e: unknown) => logCdRip('export', `自動の書き出しを始められません: ${(e as Error).message}`, id));
  }
};

/** sonnet に曲情報とジャケットを集めさせる。取り込みと並行して動く。 */
const startResearch = (id: string, retry?: { instruction?: string }): void => {
  if (pendingResearch.has(id)) return;
  const s = current();
  const item = findItem(id);
  const dir = path.join(s.stageDir, id, 'research');
  fs.mkdirSync(dir, { recursive: true });
  updateItem(id, (i) => ({ ...i, research: { startedAt: Date.now(), sources: [], ...(retry?.instruction ? { retryHint: retry.instruction } : {}) }, updatedAt: Date.now() }));
  logCdRip('research', `情報収集を始めます: ${item.hint}`, id);
  const prompt = researchPrompt({ hint: item.hint, group: item.group, disc: item.disc, video: s.features.aiVideo, ...(retry ? { retry: { instruction: retry.instruction, previous: item.meta } } : {}) });
  const task = (async () => {
    try {
      const meta = parseResearch(await askResearcher(dir, prompt, researchSchema(), { video: s.features.aiVideo }));
      let coverError: string | undefined;
      if (meta.coverUrl) {
        try {
          meta.cover = writeCover(id, await fetchCover(meta.coverUrl));
        } catch (e) {
          coverError = `ジャケットを取り込めませんでした: ${(e as Error).message}。URL で差し替えてください`;
        }
      }
      applyResearch(id, { meta, coverError });
      logCdRip('research', `情報収集を終えました: ${meta.circle} / ${meta.album}（確信 ${meta.confidence ?? '不明'}）`, id);
    } catch (e) {
      applyResearch(id, { error: (e as Error).message.split('\n')[0] });
      logCdRip('research', `情報収集に失敗しました: ${(e as Error).message.split('\n')[0]}`, id);
    } finally {
      pendingResearch.delete(id);
    }
  })();
  pendingResearch.set(id, task);
};

/** AI に、人間の指示を添えてやり直させる。 */
export const retryCdResearch = async (id: string, body: unknown): Promise<void> => {
  await initCdRip();
  const item = findItem(id);
  if (!['review', 'researching'].includes(item.status)) throw new CdRipError(409, `${item.status} の行は AI にやり直させられません`);
  if (pendingResearch.has(id)) throw new CdRipError(409, 'AI が情報を集めています');
  const hint = (body as { hint?: unknown } | undefined)?.hint;
  startResearch(id, { instruction: typeof hint === 'string' && hint.trim() ? hint.trim() : undefined });
};

// ─── rekordbox ──────────────────────────────────────────────────────────

/**
 * 書き出した mp3 を rekordbox に取り込む XML として送る。new は rekordbox 用にまだ渡していない行だけで、送ったらその行に渡した時刻を残す。
 */
export const sendCdRekordbox = async (res: ServerResponse, scope: string | null): Promise<void> => {
  await initCdRip();
  if (!current().features.rekordbox) throw new CdRipError(409, '設定で rekordbox への記録を切ってあります');
  if (scope !== 'new' && scope !== 'all') throw new CdRipError(400, 'scope は new か all にしてください');
  const { entries, itemIds } = rekordboxEntries(queue.items, scope);
  if (!entries.length) throw new CdRipError(404, scope === 'new' ? 'rekordbox にまだ渡していない書き出しはありません' : '書き出した mp3 がありません');
  res.writeHead(200, { 'content-type': 'application/xml; charset=utf-8', 'content-disposition': 'attachment; filename=rekordbox_import.xml', 'cache-control': 'no-store' }).end(rekordboxXml(entries));
  const at = Date.now();
  if (scope === 'new') mutateItems((items) => items.map((i) => (itemIds.includes(i.id) && i.exported ? { ...i, exported: { ...i.exported, rekordboxAt: at } } : i)));
  logCdRip('rekordbox', `rekordbox 用の XML を渡しました（${entries.length} 曲）`);
};
