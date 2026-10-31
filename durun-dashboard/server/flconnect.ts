// アプリ「FL Connect」の中枢。ウィンドウがつないでいる間だけ、FL Studio とやりとりする道を開き、FL の状態と操作の記録を持つ。
// 道は 2 つ。① は FL の MIDI スクリプト（fl/ のスクリプト）と SysEx でやりとりし、② は FL 2026 の Gopher の画面の中の口を呼ぶ。
// ウィンドウからの接続が切れたら道を閉じ、ツールは「FL Connect が動いていません」と返し、操作も記録しない。
// 記録を届け、ツールを使えるのは、ウィンドウでつないだ 1 つのセッションだけ。
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import type { ServerResponse } from 'node:http';
import path from 'node:path';
import type { FlChange, FlConnectView, FlGopherStatus, FlHello, FlLinkStatus, FlLogEntry, FlRecord, FlRemoval, FlValue } from '../shared/types.ts';
import { describeChange, diffPlaylist, diffRows, diffSends, diffSong, followChannels, RecordLog, resolveChannels, restoreLines, type Row, type RowSection, same, type SongState } from './flChanges.ts';
import { GopherError, GopherLink, type GopherTool } from './flGopher.ts';
import { openMidi, type MidiLink } from './flMidi.ts';
import { platform } from './platform.ts';
import { newId, state } from './store.ts';

const SCRIPT_DIR = path.join(import.meta.dirname, '..', 'fl');
const ENTRY = 'device_FLConnect.py';
const CORE = 'flconnect_core.py';
/** FL のユーザーデータの下の、スクリプトを置く場所。FL は Settings/Hardware の下のフォルダーを MIDI スクリプトとして読む。 */
const HARDWARE_DIR = path.join('Settings', 'Hardware', 'FL Connect');

const PORTS = { input: process.env.FL_CONNECT_MIDI_IN || 'FL Connect (B)', output: process.env.FL_CONNECT_MIDI_OUT || 'FL Connect (B)' };
const DEBUG_PORT = Number(process.env.FL_CONNECT_DEBUG_PORT) || 9222;

/** 安い要約を問い合わせる間隔。元に戻す履歴の数が変わっていたら、全部を読み直す。 */
const SUMMARY_MS = 1000;
/** ポートが無いときに開き直す間隔と、② の道を確かめ直す間隔。 */
const RETRY_MS = 3000;
/** この時間 FL が応答しなければ、つながっていないものとして画面に出す。 */
const SILENT_MS = 3000;
/** 変化の印が続けて届いたら、この時間まとめてから読み直す。 */
const READ_DEBOUNCE_MS = 300;
const SUMMARY_TIMEOUT_MS = 1500;
const READ_TIMEOUT_MS = 5000;
/** AI のコードの既定の時間切れと、渡せる上限（秒）。FL の中のスクリプトと同じ値。 */
const EXEC_LIMIT_SEC = 2;
const MAX_EXEC_LIMIT_SEC = 10;
const VIEW_THROTTLE_MS = 150;
const KEEPALIVE_MS = 20_000;
/** 一覧で返す行の数。多いときは start で続きを読ませる。 */
const LIST_LIMIT = 80;
/** フックで渡す記録の数。多いときは fl_changes で読ませる。 */
const HOOK_LIMIT = 20;
const MAX_LOG = 100;
const MAX_AGENT_OPS = 50;

const ROW_SECTIONS: RowSection[] = ['channels', 'mixer', 'playlist', 'patterns'];

/** OnRefresh の印と、読み直す範囲。LED や再生の位置の印は頻繁に届くので、読み直さない。 */
const FLAG_SECTIONS: [number, RowSection[]][] = [
  [1 | 2 | 4, ['mixer']],
  [1024, ['patterns']],
  [2048, ['playlist']],
  // 色と名前の印は、どの範囲が変わったかを持たない。
  [8192 | 16384, ROW_SECTIONS],
  [32768 | 65536, ['channels']],
  // 元に戻す履歴の印（FL 2026 で足された）。何が戻ったかは分からない。
  [131072, ROW_SECTIONS],
];

/** ② の道のツールのうち、FL のプロジェクトを変えないもの。これを呼んだ後は、変化を読み直さない。 */
const isReadTool = (name: string): boolean => /^(get_|list_|show_)/.test(name);

type Summary = {
  playing: boolean;
  recording: boolean;
  tempo: number | null;
  pos: string;
  song: boolean;
  channel: number;
  mixer: number;
  pattern: number;
  focus: [string | null, string];
  undo: [number, number, string];
  counts: number[];
};

type Mirror = { summary?: Summary; channels?: Row[]; mixer?: Row[]; playlist?: Row[]; playlistCount?: number; patterns?: Row[]; sends?: Record<string, number[]> };

/** エージェントの 1 回の操作で変わった値。fl_undo はこれを新しいものから戻す。 */
type AgentOp = { id: string; via: string; at: number; changes: FlChange[] };

type Win = { res: ServerResponse; sessionId?: string };

const windows = new Set<Win>();
let timers: NodeJS.Timeout[] = [];
let link: MidiLink | undefined;
let midiStatus: FlLinkStatus = 'off';
let midiMessage: string | undefined;
let hello: FlHello | undefined;
let lastReplyAt = 0;
let boundSession: string | undefined;
let readOnly = false;
let gopherEnabled = false;
let gopher: GopherLink | undefined;
let gopherStatus: FlGopherStatus = 'off';
let gopherMessage: string | undefined;
let userData: string | undefined;
let installed: string | undefined;
let mirror: Mirror = {};
const dirty = new Set<RowSection>();
let readTimer: NodeJS.Timeout | undefined;
const records = new RecordLog();
const log: FlLogEntry[] = [];
const agentOps: AgentOp[] = [];
/** ② の add_channel で足したチャンネルの、今の番号。② の remove_channels で消すと、そのあとの Ctrl+Z で FL 26.1.7 が落ちるので、消すのを止める。 */
let addedChannels: number[] = [];
let removal: (FlRemoval & { resolve: (allow: boolean) => void }) | undefined;
/** セッションごとの、fl_changes とフックで届けた記録の番号。 */
const cursors = new Map<string, number>();

const versionOf = (file: string): string | undefined => {
  try {
    return /^VERSION = "([^"]+)"/m.exec(fs.readFileSync(file, 'utf8'))?.[1];
  } catch {
    return undefined;
  }
};

const BUNDLED = versionOf(path.join(SCRIPT_DIR, ENTRY)) ?? '?';
const BUNDLED_CORE = versionOf(path.join(SCRIPT_DIR, CORE)) ?? '?';

const sessionName = (id: string): string => state.sessions[id]?.title ?? id.slice(0, 8);

// ─── ウィンドウ ─────────────────────────────────────────────────────────

const view = (): FlConnectView => {
  const s = mirror.summary;
  return {
    sessionId: boundSession,
    midi: { status: midiStatus, message: midiMessage, ports: PORTS, hello },
    gopher: { enabled: gopherEnabled, status: gopherStatus, message: gopherMessage, port: DEBUG_PORT, tools: gopher?.tools?.length },
    readOnly,
    setup: { userData: userData && platform.displayPath(userData), installed, bundled: BUNDLED },
    focus: s && { window: s.focus[0] ?? undefined, caption: s.focus[1] || undefined, channel: s.channel, mixer: s.mixer, pattern: s.pattern, playing: s.playing, tempo: s.tempo ?? undefined, undo: s.undo[2] },
    log: log.slice(-50),
    records: records.records.slice(-80).map((r) => ({ seq: r.seq, at: r.at, who: r.who, via: r.via, text: describeChange(r) })),
    removal: removal && { id: removal.id, sessionId: removal.sessionId, channels: removal.channels },
  };
};

let viewTimer: NodeJS.Timeout | undefined;

const sendView = (win: Win, body: string): void => {
  if (!win.res.writableEnded) win.res.write(`event: view\ndata: ${body}\n\n`);
};

/** 開いているウィンドウに、今の状態を送る。続けて変わったら、まとめて送る。 */
const broadcast = (): void => {
  if (viewTimer || !windows.size) return;
  viewTimer = setTimeout(() => {
    viewTimer = undefined;
    const body = JSON.stringify(view());
    for (const win of windows) sendView(win, body);
  }, VIEW_THROTTLE_MS);
};

const addLog = (entry: Omit<FlLogEntry, 'id' | 'at'>): void => {
  log.push({ id: newId(), at: Date.now(), ...entry });
  if (log.length > MAX_LOG) log.splice(0, log.length - MAX_LOG);
  broadcast();
};

/** つなぐセッションを決める。決めたときより前の記録は、そのセッションに届けない。 */
const bind = (sessionId: string | undefined): void => {
  if (sessionId === boundSession) return;
  boundSession = sessionId;
  if (sessionId && !cursors.has(sessionId)) cursors.set(sessionId, records.last);
  broadcast();
};

/** ウィンドウからの接続。最初のウィンドウがつないだら道を開き、最後のウィンドウが切れたら閉じる。 */
export const streamFlConnect = (res: ServerResponse, sessionId: string | undefined): void => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  const win: Win = { res, sessionId };
  windows.add(win);
  if (sessionId) bind(sessionId);
  if (windows.size === 1) start();
  sendView(win, JSON.stringify(view()));
  const keepalive = setInterval(() => res.write(': keepalive\n\n'), KEEPALIVE_MS);
  res.on('close', () => {
    clearInterval(keepalive);
    windows.delete(win);
    if (!windows.size) stop();
  });
};

export const setFlSession = (sessionId: string | undefined): void => bind(sessionId);

export const setFlSettings = (settings: { readOnly?: boolean; gopher?: boolean }): void => {
  if (settings.readOnly !== undefined) readOnly = settings.readOnly;
  if (settings.gopher !== undefined && settings.gopher !== gopherEnabled) {
    gopherEnabled = settings.gopher;
    if (gopherEnabled) void checkGopher();
    else closeGopher();
  }
  broadcast();
};

/** 人間が、remove_channels で消えるチャンネルを確かめて答える。 */
export const answerFlRemoval = (id: string, allow: boolean): boolean => {
  if (removal?.id !== id) return false;
  removal.resolve(allow);
  return true;
};

// ─── ① の道 ────────────────────────────────────────────────────────────

let queue: Promise<unknown> = Promise.resolve();
let queued = 0;

/**
 * FL への読み書きを 1 つずつ動かす。エージェントの操作（前を読む、書く、後を読む）の途中で、決まった間隔の読み直しが挟まると、
 * エージェントが変えた値を人間の操作と読み違えるため。
 */
const exclusive = <T>(task: () => Promise<T>): Promise<T> => {
  queued += 1;
  const run = queue.then(task, task).finally(() => {
    queued -= 1;
  });
  queue = run.catch(() => undefined);
  return run;
};

const setMidi = (status: FlLinkStatus, message?: string): void => {
  if (status === midiStatus && message === midiMessage) return;
  midiStatus = status;
  midiMessage = message;
  broadcast();
};

const SILENT_HINT =
  'FL が名乗りません。FL の MIDI の設定で、入力の FL Connect (A) を有効にして Controller type に「FL Connect」を選び、出力の FL Connect (A) に同じポートの番号を付けてください。番号がほかの機器と重なると届きません。FL を起動し直すと、この設定が外れることがあります。';

const versionNote = (h: FlHello): string | undefined => {
  if (h.coreError) return `FL の中の flconnect_core.py を読めません。${h.coreError.split('\n').slice(-2).join(' ')}`;
  if (h.script !== BUNDLED) return `FL が動かしているスクリプトの入口は ${h.script} 版で、ダッシュボードの ${BUNDLED} 版と違います。「スクリプトを置く」を押してから、FL の View > Script output で FL Connect の「Reload script」を押すか、FL を起動し直してください。`;
  return undefined;
};

const toHello = (v: Record<string, unknown>): FlHello => ({
  script: String(v.script ?? '?'),
  core: typeof v.core === 'string' ? v.core : undefined,
  coreError: typeof v.core_error === 'string' ? v.core_error : undefined,
  fl: String(v.fl ?? '?'),
  api: Number(v.api ?? 0),
  python: String(v.python ?? '?'),
});

const greeted = (h: FlHello): void => {
  const fresh = hello?.fl !== h.fl || hello?.script !== h.script;
  hello = h;
  lastReplyAt = Date.now();
  // FL を起動し直したか、スクリプトを読み直した。前のプロジェクトの値とは比べない。
  if (fresh) resetProject();
  setMidi('connected', versionNote(h));
  broadcast();
};

/** FL Connect のスクリプトに要求を送る。スクリプトが ok: false を返したら、その理由で例外を投げる。 */
const request = async (payload: Record<string, unknown>, timeoutMs: number): Promise<Record<string, unknown>> => {
  if (!link) throw new Error('MIDI のポートを開いていません');
  const reply = await link.request(payload, timeoutMs);
  lastReplyAt = Date.now();
  if (reply.ok === false) throw new Error(String(reply.error ?? 'FL の中で失敗しました'));
  return reply;
};

const sayHello = async (): Promise<void> => {
  try {
    greeted(toHello(await request({ op: 'hello' }, SUMMARY_TIMEOUT_MS)));
  } catch {
    if (!hello) setMidi('silent', SILENT_HINT);
  }
};

/** 覚えている値を捨て、次に読んだ値を比べる元にする。スクリプトを読み直すと値の形が変わることがあり、その違いを人間の操作と読み違えないため。 */
const forgetValues = (): void => {
  mirror = {};
  for (const s of ROW_SECTIONS) dirty.add(s);
};

/** 別のプロジェクトになった。② で足したチャンネルの元に戻す履歴も消えるので、覚えたチャンネルも捨てる。 */
const resetProject = (): void => {
  forgetValues();
  addedChannels = [];
};

const songOf = (s: Summary): SongState => ({ tempo: s.tempo, playing: s.playing, recording: s.recording });

/** プロジェクトの読み込みの印が届いたら、前のプロジェクトの値と比べないよう、覚えた値を捨てる。 */
let projectLoaded = false;

const onNotice = (value: Record<string, unknown>): void => {
  if (value.hello && typeof value.hello === 'object') greeted(toHello(value.hello as Record<string, unknown>));
  if (value.bye) {
    hello = undefined;
    setMidi('silent', 'FL Connect のスクリプトが止まりました。FL を閉じたか、スクリプトを読み直しています。');
  }
  const d = value.dirty as { items?: unknown[][]; dropped?: number } | undefined;
  if (!d) return;
  for (const [kind, arg] of d.items ?? []) {
    if (kind === 'mixer') dirty.add('mixer');
    else if (kind === 'channel') dirty.add('channels');
    else if (kind === 'project') projectLoaded = true;
    else if (kind === 'refresh') for (const [flag, sections] of FLAG_SECTIONS) if (Number(arg) & flag) for (const s of sections) dirty.add(s);
  }
  if (d.dropped) for (const s of ROW_SECTIONS) dirty.add(s);
  if (dirty.size || projectLoaded) scheduleRead();
};

const scheduleRead = (): void => {
  if (readTimer) return;
  readTimer = setTimeout(() => {
    readTimer = undefined;
    if (!link || midiStatus !== 'connected') return;
    void exclusive(() => readDirty()).catch(() => undefined);
  }, READ_DEBOUNCE_MS);
};

const readDirty = async (): Promise<void> => {
  if (projectLoaded) {
    projectLoaded = false;
    resetProject();
    records.add({ section: 'song', index: 0, target: 'song', field: 'project', before: null, after: null }, 'human');
  }
  if (!dirty.size) return;
  const sections = [...dirty];
  dirty.clear();
  await refresh(sections, 'human');
};

/** ミキサーのトラックのうち、送り先を記録するもの。Master、チャンネルの送り先、名前を変えたトラック。全部を読むと、FL の API を 1 万回以上呼ぶため。 */
const usedTracks = (): number[] => {
  const used = new Set<number>([0]);
  for (const c of mirror.channels ?? []) if (typeof c.mixer === 'number' && c.mixer >= 0) used.add(c.mixer);
  for (const m of mirror.mixer ?? []) if (m.name !== `Insert ${m.i}` && m.name !== '' && m.i > 0) used.add(m.i);
  return [...used].sort((a, b) => a - b);
};

type ReadOptions = { sends?: number[]; slots?: boolean };

/**
 * sections を読み直し、覚えている値と比べて、違いを who の記録にする。返すのは、変わった値と、FL が返したもの。
 * 初めて読む範囲は、比べずに覚えるだけにする。
 */
const refresh = async (sections: RowSection[], who: FlRecord['who'], via?: string, options: ReadOptions = {}): Promise<{ changes: FlChange[]; reply: Record<string, unknown> }> => {
  const reply = await request({ op: 'read', sections, ...(options.sends ? { sends: options.sends } : {}), ...(options.slots ? { slots: true } : {}) }, READ_TIMEOUT_MS);
  const changes: FlChange[] = [];
  const summary = reply.summary as Summary | undefined;
  if (summary) {
    if (mirror.summary) changes.push(...diffSong(songOf(mirror.summary), songOf(summary)));
    mirror.summary = summary;
  }
  for (const section of sections) {
    if (section === 'playlist') {
      const pl = reply.playlist as { items: Row[]; count: number } | undefined;
      if (!pl) continue;
      if (mirror.playlist) changes.push(...diffPlaylist(mirror.playlist, pl.items));
      mirror.playlist = pl.items;
      mirror.playlistCount = pl.count;
      continue;
    }
    const rows = reply[section] as Row[] | undefined;
    if (!rows) continue;
    const prev = mirror[section];
    if (prev) changes.push(...diffRows(section, prev, rows));
    if (section === 'channels' && prev) addedChannels = followChannels(addedChannels, prev, rows);
    mirror[section] = rows;
  }
  const sends = reply.sends as Record<string, number[]> | undefined;
  if (sends) {
    if (mirror.sends) changes.push(...diffSends(mirror.sends, sends, new Map((mirror.mixer ?? []).map((m) => [m.i, m.name]))));
    mirror.sends = { ...mirror.sends, ...sends };
  }
  for (const c of changes) records.add(c, who, via);
  if (changes.length) broadcast();
  return { changes, reply };
};

/** 決まった間隔で安い要約を問い合わせ、FL が生きているかを見る。元に戻す履歴の数か、チャンネルなどの数が変わっていたら、全部を読み直す。 */
const tick = async (): Promise<void> => {
  if (!link) return;
  // エージェントの操作の最中は、その操作が後で読み直す。
  if (queued > 0) return;
  await exclusive(async () => {
    let s: Summary;
    try {
      s = (await request({ op: 'summary' }, SUMMARY_TIMEOUT_MS)) as unknown as Summary;
    } catch {
      if (Date.now() - lastReplyAt > SILENT_MS) {
        hello = undefined;
        setMidi('silent', SILENT_HINT);
      }
      return;
    }
    if (!hello) await sayHello();
    const prev = mirror.summary;
    if (prev) for (const c of diffSong(songOf(prev), songOf(s))) records.add(c, 'human');
    const focusChanged = !prev || JSON.stringify(prev.focus) !== JSON.stringify(s.focus) || prev.channel !== s.channel || prev.mixer !== s.mixer || prev.pattern !== s.pattern || prev.playing !== s.playing || prev.undo[2] !== s.undo[2];
    mirror.summary = s;
    if (!prev || prev.undo[0] !== s.undo[0] || prev.undo[1] !== s.undo[1] || !same(prev.counts, s.counts)) for (const x of ROW_SECTIONS) dirty.add(x);
    for (const x of ROW_SECTIONS) if (!mirror[x]) dirty.add(x);
    if (dirty.size || projectLoaded) await readDirty();
    if (focusChanged) broadcast();
  });
};

const openLink = async (): Promise<void> => {
  try {
    link = await openMidi(PORTS, { onNotice, onHeartbeat: () => (lastReplyAt = Date.now()) });
  } catch (e) {
    link = undefined;
    setMidi('no-port', `${(e as Error).message}。Windows MIDI Services のループバック「FL Connect」を作ってください。`);
    return;
  }
  setMidi('silent', SILENT_HINT);
  await exclusive(sayHello);
};

const start = (): void => {
  void findUserData();
  void openLink();
  timers = [
    setInterval(() => void tick().catch(() => undefined), SUMMARY_MS),
    setInterval(() => {
      if (!link) void openLink();
      if (gopherEnabled) void checkGopher();
    }, RETRY_MS),
  ];
};

const stop = (): void => {
  for (const t of timers) clearInterval(t);
  timers = [];
  if (readTimer) clearTimeout(readTimer);
  readTimer = undefined;
  link?.close();
  link = undefined;
  hello = undefined;
  midiStatus = 'off';
  midiMessage = undefined;
  closeGopher();
  mirror = {};
  dirty.clear();
  removal?.resolve(false);
};

// ─── ② の道 ────────────────────────────────────────────────────────────

const setGopher = (status: FlGopherStatus, message?: string): void => {
  if (status === gopherStatus && message === gopherMessage) return;
  gopherStatus = status;
  gopherMessage = message;
  broadcast();
};

const checkGopher = async (): Promise<void> => {
  if (!gopherEnabled || !windows.size) return;
  gopher ??= new GopherLink(DEBUG_PORT);
  try {
    await gopher.check();
    setGopher('connected');
  } catch (e) {
    const s = e instanceof GopherError ? e.state : 'no-port';
    setGopher(s === 'no-page' || s === 'no-handler' || s === 'busy' ? s : 'no-port', (e as Error).message);
  }
};

const closeGopher = (): void => {
  gopher?.close();
  gopher = undefined;
  gopherStatus = gopherEnabled && windows.size ? 'no-port' : 'off';
  gopherMessage = undefined;
};

// ─── 導入 ──────────────────────────────────────────────────────────────

const hasHardware = (dir: string): boolean => fs.existsSync(path.join(dir, 'Settings', 'Hardware'));

/** FL のユーザーデータのフォルダーを、FL_USER_DATA か、よくある場所から探す。 */
const findUserData = async (): Promise<void> => {
  const given = process.env.FL_USER_DATA;
  const candidates = given ? [given] : platform.flUserDataCandidates(await platform.documentsDir());
  userData = candidates.find(hasHardware);
  installed = userData ? versionOf(path.join(userData, HARDWARE_DIR, ENTRY)) : undefined;
  broadcast();
};

/** ダッシュボードが持つスクリプトを FL のユーザーデータに置く。FL とつながっていれば、中身のスクリプトを読み直させる。 */
export const installFlScripts = async (): Promise<{ message: string }> => {
  await findUserData();
  if (!userData) return { message: 'FL のユーザーデータのフォルダー（Settings/Hardware があるところ）が見つかりません。~/.durun-dashboard/.env.local の FL_USER_DATA に、FL の File settings の「User data folder」を書いてください。' };
  const dest = path.join(userData, HARDWARE_DIR);
  fs.mkdirSync(dest, { recursive: true });
  for (const f of [ENTRY, CORE]) fs.copyFileSync(path.join(SCRIPT_DIR, f), path.join(dest, f));
  await findUserData();
  const placed = `スクリプトを ${platform.displayPath(dest)} に置きました。`;
  if (!link || !hello) return { message: `${placed}FL の MIDI の設定で、入力の FL Connect (A) の Controller type に「FL Connect」を選んでください。` };
  const reloaded = await exclusive(async () => {
    const r = await request({ op: 'reload' }, READ_TIMEOUT_MS);
    forgetValues();
    return r;
  }).catch((e: Error) => ({ ok: false, error: e.message }));
  await exclusive(sayHello);
  if (hello && hello.script !== BUNDLED) return { message: `${placed}入口のスクリプトが変わったので、FL の View > Script output で FL Connect の「Reload script」を押すか、FL を起動し直してください。` };
  return { message: reloaded.ok === false ? `${placed}FL で読み直せませんでした: ${String(reloaded.error)}` : `${placed}FL で中身のスクリプトを読み直しました（${BUNDLED_CORE} 版）。` };
};

/** ② の道のために、デバッグのポートを開く環境変数を付けて FL Studio を起動する。動いている FL には付けられないので、閉じてもらう。 */
export const launchFlStudio = async (): Promise<{ message: string }> => {
  const exe = platform.flStudioPath(process.env);
  if (!exe) return { message: 'この OS では、② の道のための FL の起動はできません（FL 2026 の Windows 版だけが使える道です）。' };
  if (!fs.existsSync(exe)) return { message: `FL Studio が ${exe} にありません。~/.durun-dashboard/.env.local の FL_STUDIO_PATH に FL64.exe の場所を書いてください。` };
  if (await platform.isFlRunning().catch(() => undefined)) return { message: 'FL Studio が動いています。保存して FL を閉じてから、もう一度押してください。動いている FL には、デバッグのポートを開く設定を付けられません。' };
  const child = spawn(exe, [], { cwd: path.dirname(exe), detached: true, stdio: 'ignore', env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${DEBUG_PORT}` } });
  child.unref();
  return { message: `FL Studio を、デバッグのポート ${DEBUG_PORT} を開く設定で起動しました。FL で Gopher を開くとポートが開き、② の道がつながります。このポートが開いている間は、この PC のどのプロセスからも FL を操作できるので、使い終えたら FL をふつうに起動し直してください。` };
};

/** Windows MIDI Services のループバック「FL Connect」を作る。 */
export const createFlLoopback = async (): Promise<{ message: string }> => {
  const command = platform.midiLoopbackCommand(process.env);
  if (!command) return { message: 'この OS では、ループバックを作れません。FL Connect (A) と (B) の名前の MIDI ポートを、ほかの方法で用意してください。' };
  if (!fs.existsSync(command.file)) return { message: `Windows MIDI Services の道具（${command.file}）がありません。microsoft/MIDI のリリースから Tools のパッケージを入れてください。` };
  const out = await new Promise<string>((resolve) => execFile(command.file, command.args, (error, stdout, stderr) => resolve(`${stdout}${stderr}${error ? `\n${error.message}` : ''}`)));
  if (!link) void openLink();
  return { message: `ループバックを作るコマンドを動かしました。\n${out.trim().slice(-1500)}\n「Saved to Config: No」なら、MIDI Settings のアプリで設定を作ってから、もう一度押してください。` };
};

// ─── MCP のツール ─────────────────────────────────────────────────────────

export type FlToolResult = { text: string; isError?: boolean };

const fail = (text: string): FlToolResult => ({ text, isError: true });

/** ツールを使えないときの理由。ウィンドウが無い、別のセッションにつないでいる、など。 */
const refusal = (sessionId: string): string | undefined => {
  if (!windows.size) return 'FL Connect が動いていません。人間にダッシュボードで FL Connect のウィンドウを開いてもらってください。ウィンドウを開いている間だけ、FL とつながります。';
  if (!boundSession) return 'FL Connect のウィンドウで、つなぐセッションが選ばれていません。人間に、ウィンドウでこのセッションを選んでもらってください。';
  if (boundSession !== sessionId) return `FL Connect は、別のセッション「${sessionName(boundSession)}」につないでいます。このセッションから使うには、人間に FL Connect のウィンドウでこのセッションに切り替えてもらってください。`;
  return undefined;
};

const midiRefusal = (): string | undefined => {
  if (midiStatus === 'connected' && link) return undefined;
  return `FL とつながっていません（① の道）。${midiMessage ?? ''}`.trim();
};

const gopherRefusal = (): string | undefined => {
  if (!gopherEnabled) return '② の道（FL 2026 の Gopher の中の口）が入っていません。人間に FL Connect のウィンドウで「② の道を使う」を入れてもらってください。';
  if (gopherStatus !== 'connected' && gopherStatus !== 'busy') return `② の道がつながっていません。${gopherMessage ?? ''}`.trim();
  return undefined;
};

const hex = (v: FlValue | undefined): string => (typeof v === 'number' ? `#${v.toString(16).toUpperCase().padStart(6, '0')}` : '?');

const CHANNEL_TYPES: Record<number, string> = { 0: 'Sampler', 1: 'Hybrid', 2: 'Generator', 3: 'Layer', 4: 'Audio clip', 5: 'Automation clip' };

const flags = (r: Row): string => [r.mute ? 'ミュート' : '', r.solo ? 'ソロ' : ''].filter(Boolean).join('、');

const level = (r: Row): string => `${r.db === null || r.db === undefined ? '-∞' : String(r.db)} dB、パン ${typeof r.pan === 'number' ? (Math.abs(r.pan) < 0.005 ? '中央' : r.pan < 0 ? `左 ${Math.round(-r.pan * 100)}%` : `右 ${Math.round(r.pan * 100)}%`) : '?'}`;

/** 行を並べる。多いときは LIST_LIMIT 行で切り、続きの読み方を添える。 */
const listing = (title: string, rows: string[], start: number): string[] => {
  const page = rows.slice(start, start + LIST_LIMIT);
  const more = rows.length > start + LIST_LIMIT ? [`  …ほかに ${rows.length - start - LIST_LIMIT} 行。続きは fl_state に start: ${start + LIST_LIMIT} を渡すと読める`] : [];
  return [`${title}:`, ...(page.length ? page.map((r) => `- ${r}`) : ['  （無い）']), ...more];
};

/** いちばん多い色。プレイリストとミキサーの、触っていないトラックの色として、一覧から外すのに使う。 */
const commonColor = (rows: Row[]): FlValue | undefined => {
  const count = new Map<FlValue | undefined, number>();
  for (const r of rows) count.set(r.color, (count.get(r.color) ?? 0) + 1);
  return [...count.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
};

const summaryLines = (s: Summary | undefined): string[] => {
  if (!s) return [];
  const where = [s.focus[0] ? `前面のウィンドウ: ${s.focus[0]}${s.focus[1] ? `（${s.focus[1]}）` : ''}` : s.focus[1] ? `前面: ${s.focus[1]}` : '', `選んでいるもの: channel ${s.channel}、mixer ${s.mixer}、pattern ${s.pattern}`].filter(Boolean);
  return [
    `${s.playing ? (s.recording ? '録音している' : '再生している') : '止まっている'}。テンポ ${s.tempo ?? '?'} BPM。位置 ${s.pos}。${s.song ? 'ソング' : 'パターン'}のモード。`,
    `${where.join('。')}。元に戻す履歴: ${s.undo[0]} 件（戻した数 ${s.undo[1]}）。`,
  ];
};

/** fl_state：状態を読む。既定は名前と主な値だけの短い形で、detail で音量、パン、スロット、送り先も返す。 */
const flState = async (args: Record<string, unknown>): Promise<FlToolResult> => {
  const start = Math.max(0, Number(args.start) || 0);
  const plugin = args.plugin as { channel?: number; mixer?: number; slot?: number } | undefined;
  if (plugin && typeof plugin === 'object') {
    const target = typeof plugin.mixer === 'number' ? { index: plugin.mixer, slot: Number(plugin.slot ?? 0) } : { index: Number(plugin.channel ?? 0), slot: -1 };
    const r = await exclusive(() => request({ op: 'params', ...target, start, limit: LIST_LIMIT }, READ_TIMEOUT_MS)).catch((e: Error) => e);
    if (r instanceof Error) return fail(r.message);
    const items = (r.items as { p: number; name: string; value: number; text: string }[]).map((p) => `${p.p}「${p.name}」 ${p.text}（${p.value}）`);
    const next = r.next === null || r.next === undefined ? '' : `\n続きは fl_state の plugin に同じものを、start: ${String(r.next)} を渡すと読める。`;
    return { text: `${String(r.plugin)} のパラメーター（名前の無いものを除く。全部で ${String(r.count)} 個。番号は plugins.getParamValue などの paramIndex）:\n${items.map((x) => `- ${x}`).join('\n')}${next}` };
  }
  const asked = Array.isArray(args.sections) ? args.sections.filter((s): s is RowSection => ROW_SECTIONS.includes(s as RowSection)) : [];
  const sections = asked.length ? asked : ROW_SECTIONS;
  const detail = args.detail === true;
  const read = await exclusive(() => refresh(sections, 'human', undefined, detail ? { slots: sections.includes('mixer'), sends: usedTracks() } : {})).catch((e: Error) => e);
  if (read instanceof Error) return fail(read.message);
  const { reply } = read;
  const h = hello;
  const lines = [`FL Studio ${h?.fl ?? '?'}（MIDI スクリプトの API ${h?.api ?? '?'}、FL Connect のスクリプト ${h?.script ?? '?'}）。`, ...summaryLines(mirror.summary)];
  if (sections.includes('channels')) {
    const rows = (mirror.channels ?? []).map((c) => {
      const what = c.plugin ? String(c.plugin) : (CHANNEL_TYPES[Number(c.type)] ?? '?');
      return [`${c.i}「${String(c.name)}」 ${what} → mixer ${String(c.mixer)}、${hex(c.color)}`, detail ? level(c) : '', flags(c), c.sel ? '選んでいる' : ''].filter(Boolean).join('、');
    });
    lines.push(...listing(`チャンネル（${rows.length} 個。番号は 0 から。remove_channels の番号は、これに 1 を足したもの）`, rows, start));
  }
  if (sections.includes('mixer')) {
    const all = mirror.mixer ?? [];
    const plain = commonColor(all);
    const targets = new Set((mirror.channels ?? []).map((c) => c.mixer));
    const slots = new Map((reply.mixer as Row[] | undefined)?.map((m) => [m.i, m.slots as unknown as (string | null)[] | undefined]) ?? []);
    const sends = (reply.sends ?? {}) as Record<string, number[]>;
    // 名前の無い末尾のトラックは、FL が選んでいるトラックを指すために持つもので、プロジェクトのトラックではない。
    const named = (m: Row): boolean => m.name !== `Insert ${m.i}` && m.name !== '';
    const shown = all.filter((m) => detail || m.i === 0 || named(m) || (m.name !== '' && m.color !== plain) || targets.has(m.i) || m.mute || m.solo);
    const rows = shown.map((m) => {
      const fx = (slots.get(m.i) ?? []).map((p, k) => (p ? `${k + 1}: ${p}` : '')).filter(Boolean);
      const to = sends[String(m.i)];
      return [`${m.i}「${String(m.name)}」 ${hex(m.color)}`, detail ? level(m) : '', flags(m), fx.length ? `スロット ${fx.join('、')}` : '', to?.length ? `送り先 ${to.map((d) => `mixer ${d}`).join('、')}` : ''].filter(Boolean).join('、');
    });
    lines.push(...listing(`ミキサー（${detail ? '全部' : 'Master と、名前か色を変えたトラックと、チャンネルの送り先'}。全部で ${all.length} 個。0 が Master）`, rows, start));
  }
  if (sections.includes('playlist')) {
    const all = mirror.playlist ?? [];
    const plain = commonColor(all);
    const shown = all.filter((t) => detail || t.name !== `Track ${t.i}` || t.color !== plain || t.mute || t.solo);
    const rows = shown.map((t) => [`${t.i}「${String(t.name)}」 ${hex(t.color)}`, flags(t)].filter(Boolean).join('、'));
    lines.push(...listing(`プレイリストのトラック（${detail ? '末尾の触っていないトラックを除く' : '名前か色を変えたもの'}。全部で ${mirror.playlistCount ?? '?'} 個。番号は 1 から）`, rows, start));
  }
  if (sections.includes('patterns')) {
    const rows = (mirror.patterns ?? []).map((p) => `${p.i}「${String(p.name)}」 ${hex(p.color)}、${String(p.length)} 拍`);
    lines.push(...listing(`パターン（${rows.length} 個。番号は 1 から）`, rows, start));
  }
  return { text: lines.join('\n') };
};

const recordLine = (r: FlRecord): string => {
  const time = new Date(r.at).toTimeString().slice(0, 8);
  const who = r.who === 'human' ? '人間' : `エージェント（${r.via ?? '?'}）`;
  return `[${time}] ${who}: ${describeChange(r)}`;
};

/** fl_changes：前に読んだところから先の記録を返す。フックで届けた記録も読んだものとして扱う。 */
const flChanges = (sessionId: string, args: Record<string, unknown>): FlToolResult => {
  const since = args.all === true ? Math.max(0, records.last - 50) : (cursors.get(sessionId) ?? 0);
  const list = records.since(since);
  cursors.set(sessionId, records.last);
  const head = list.length ? `前に読んでから ${list.length} 件の変化があった（番号は MIDI スクリプトの API の番号）:` : '前に読んでから、FL の値は変わっていない。';
  return { text: [head, ...list.map(recordLine), ...summaryLines(mirror.summary)].join('\n') };
};

const pushAgentOp = (via: string, changes: FlChange[]): void => {
  if (!changes.length) return;
  agentOps.push({ id: newId(), via, at: Date.now(), changes });
  if (agentOps.length > MAX_AGENT_OPS) agentOps.splice(0, agentOps.length - MAX_AGENT_OPS);
};

const changeLines = (changes: FlChange[]): string[] => changes.map((c) => describeChange(c));

/** fl_python：AI が書いた Python を FL の中で動かす。前と後を読み、変わった値をエージェントの変更として記録する。 */
const flPython = async (args: Record<string, unknown>): Promise<FlToolResult> => {
  const code = typeof args.code === 'string' ? args.code : '';
  if (!code.trim()) return fail('code に、FL の中で動かす Python のコードを渡してください。');
  const limit = Math.min(Math.max(Number(args.timeout) || EXEC_LIMIT_SEC, 0.1), MAX_EXEC_LIMIT_SEC);
  return exclusive(async () => {
    // 前に読んでから人間が変えた値を、人間の記録にしてから動かす。
    const used = usedTracks();
    await refresh(ROW_SECTIONS, 'human', undefined, { sends: used });
    let reply: Record<string, unknown>;
    try {
      if (!link) throw new Error('MIDI のポートを閉じました');
      reply = await link.request({ op: 'exec', code, limit, readonly: readOnly }, (limit + 5) * 1000);
    } catch (e) {
      return fail(`${(e as Error).message}。FL の画面が固まっていないか、人間に確かめてもらってください。`);
    }
    const { changes } = await refresh(ROW_SECTIONS, 'agent', 'fl_python', { sends: used });
    pushAgentOp('fl_python', changes);
    const ok = reply.ok === true;
    const stdout = String(reply.stdout ?? '');
    const result = reply.result === null || reply.result === undefined ? '' : JSON.stringify(reply.result, null, 2);
    const firstLine = code.trim().split('\n')[0] ?? '';
    addLog({ tool: 'fl_python', title: firstLine.slice(0, 80), detail: code, output: [stdout, ok ? result : String(reply.error ?? '')].filter(Boolean).join('\n'), ok, changes: changeLines(changes) });
    const parts = [
      ok ? `動いた（${String(reply.ms)} ms）。` : `例外で止まった（${String(reply.ms)} ms）。例外より前に変えた値は、そのまま残っている。\n${String(reply.error ?? '')}`,
      stdout ? `print の出力:\n${stdout}` : '',
      result ? `result:\n${result}` : '',
      changes.length ? `変わった値（${changes.length} 件。fl_undo で戻せる。FL の元に戻す履歴にも、API の 1 回の呼び出しごとに残る）:\n${changeLines(changes).map((l) => `- ${l}`).join('\n')}` : '名前、色、音量、パン、ミュート、ソロ、送り先は変わっていない。',
    ];
    return { text: parts.filter(Boolean).join('\n\n'), isError: !ok };
  });
};

/** fl_undo：エージェントの変更を、記録から新しいものから戻す。人間がそのあとで変えた値は戻さない。 */
const flUndo = async (args: Record<string, unknown>): Promise<FlToolResult> => {
  if (readOnly) return fail('FL Connect のウィンドウで書き込みを止めているので、戻せません。');
  if (!agentOps.length) return fail('戻せるエージェントの変更がありません。');
  const steps = Math.min(Math.max(Number(args.steps) || 1, 1), agentOps.length);
  return exclusive(async () => {
    const used = usedTracks();
    await refresh(ROW_SECTIONS, 'human', undefined, { sends: used });
    const ops = agentOps.splice(agentOps.length - steps, steps).reverse();
    const lines: string[] = [];
    const skipped: string[] = [];
    for (const op of ops) {
      for (const c of [...op.changes].reverse()) {
        const restore = restoreLines(c);
        if (!restore.length) skipped.push(`${describeChange(c)}（① の道では戻せない）`);
        else if (!same(currentValue(c), c.raw ? c.raw[1] : c.after)) skipped.push(`${describeChange(c)}（人間がそのあとで変えたか、並びが変わったので戻さない）`);
        else lines.push(...restore);
      }
    }
    let changes: FlChange[] = [];
    if (lines.length && link) {
      const reply = await link.request({ op: 'exec', code: lines.join('\n'), limit: MAX_EXEC_LIMIT_SEC }, (MAX_EXEC_LIMIT_SEC + 5) * 1000);
      changes = (await refresh(ROW_SECTIONS, 'agent', 'fl_undo', { sends: used })).changes;
      if (reply.ok !== true) skipped.push(`戻す途中で例外が起きた: ${String(reply.error ?? '')}`);
    }
    addLog({ tool: 'fl_undo', title: `${ops.length} 回分の操作を戻した`, detail: lines.join('\n'), ok: !skipped.length, changes: changeLines(changes) });
    const text = [
      changes.length ? `戻した（${changes.length} 件）:\n${changeLines(changes).map((l) => `- ${l}`).join('\n')}` : '書き戻した値はない。',
      skipped.length ? `戻さなかったもの:\n${skipped.map((l) => `- ${l}`).join('\n')}` : '',
    ];
    return { text: text.filter(Boolean).join('\n\n') };
  });
};

/** 覚えている今の値。音量は書き戻す値で比べる。 */
const currentValue = (c: FlChange): FlValue | undefined => {
  if (c.section === 'sends') return mirror.sends?.[String(c.index)];
  if (c.section === 'song') return undefined;
  const row = mirror[c.section]?.find((r) => r.i === c.index);
  return row?.[c.field] ?? (c.section === 'playlist' && !row ? null : undefined);
};

/** remove_channels の確かめ。人間がウィンドウで答えるまで待つ。ウィンドウが閉じたときと、エージェントが呼び出しを取り消したときは、断ったものとする。 */
const askRemoval = (sessionId: string, channels: FlRemoval['channels'], signal?: AbortSignal): Promise<boolean> =>
  new Promise((resolve) => {
    removal?.resolve(false);
    const id = newId();
    let settled = false;
    removal = {
      id,
      sessionId,
      channels,
      resolve: (allow) => {
        if (settled) return;
        settled = true;
        if (removal?.id === id) removal = undefined;
        broadcast();
        resolve(allow);
      },
    };
    const current = removal;
    signal?.addEventListener('abort', () => current.resolve(false), { once: true });
    broadcast();
  });

const ADDED_CHANNEL_CRASH =
  'は、このプロジェクトで ② の add_channel で足したチャンネルです。② で足したチャンネルを ② の remove_channels で消すと、そのあと FL で Ctrl+Z を押したときに FL 26.1.7 が落ちます（足した操作の元に戻す履歴が、消したチャンネルを指したまま残るため）。消すのは、人間に FL の画面で頼んでください。';

/** ② の remove_channels。消えるチャンネルを ① の道で読み、人間に確かめてから、番号で指して呼ぶ。 */
const removeChannels = async (sessionId: string, args: Record<string, unknown>, signal?: AbortSignal): Promise<FlToolResult> => {
  const why = midiRefusal();
  if (why) return fail(`消すチャンネルを確かめるには、① の道が要ります。${why}`);
  const spec = String(args.target_channels ?? '');
  await exclusive(() => refresh(['channels'], 'human'));
  const rows = mirror.channels ?? [];
  const { indices, unknown } = resolveChannels(spec, rows);
  if (unknown.length) return fail(`「${unknown.join('」「')}」に当たるチャンネルがありません。target_channels には、チャンネルラックの上からの番号（1 から）か、チャンネルの名前をそのまま渡してください。`);
  if (!indices.length) return fail('消すチャンネルがありません。');
  const guarded = indices.filter((i) => addedChannels.includes(i));
  if (guarded.length) return fail(`${guarded.map((i) => `channel ${i}「${String(rows[i]?.name)}」`).join('、')}${ADDED_CHANNEL_CRASH}`);
  const channels = indices.map((i) => ({ position: i + 1, name: String(rows[i]?.name ?? ''), plugin: String(rows[i]?.plugin ?? CHANNEL_TYPES[Number(rows[i]?.type)] ?? '') }));
  if (!(await askRemoval(sessionId, channels, signal))) {
    addLog({ tool: 'fl_gopher', title: 'remove_channels を人間が断った', detail: channels.map((c) => `${c.position}: ${c.name}`).join('\n'), ok: false });
    return { text: '人間が、チャンネルを消すのを断りました。消していません。' };
  }
  return exclusive(async () => {
    await refresh(ROW_SECTIONS, 'human');
    const now = mirror.channels ?? [];
    if (channels.some((c) => now[c.position - 1]?.name !== c.name)) return fail('確かめている間にチャンネルの並びが変わったので、消しませんでした。もう一度呼んでください。');
    const r = await gopher!.call('remove_channels', { target_channels: indices.map((i) => i + 1).join(',') }).catch((e: Error) => e);
    if (r instanceof Error) return fail(r.message);
    const { changes } = await refresh(ROW_SECTIONS, 'agent', 'fl_gopher:remove_channels');
    addLog({ tool: 'fl_gopher', title: 'remove_channels', detail: channels.map((c) => `${c.position}: ${c.name}（${c.plugin}）`).join('\n'), output: r.text, ok: !r.isError, changes: changeLines(changes) });
    return { text: `${r.text}\n消したチャンネルは、FL の元に戻す履歴に残らないので、Ctrl+Z では戻りません。`, isError: r.isError };
  });
};

const toolList = (tools: GopherTool[]): string =>
  tools
    .map((t) => {
      const props = t.inputSchema?.properties ?? {};
      const required = new Set(t.inputSchema?.required ?? []);
      const params = Object.keys(props).map((k) => (required.has(k) ? k : `${k}?`));
      return `- ${t.name}(${params.join(', ')}): ${t.description.replace(/\s+/g, ' ').slice(0, 300)}`;
    })
    .join('\n');

/** fl_gopher：② の道のツールを呼ぶ。ツールの名前を省くと、ツールの一覧を返す。 */
const flGopher = async (sessionId: string, args: Record<string, unknown>, signal?: AbortSignal): Promise<FlToolResult> => {
  const why = gopherRefusal();
  if (why) return fail(why);
  const tool = typeof args.tool === 'string' ? args.tool.trim() : '';
  const toolArgs = (args.arguments && typeof args.arguments === 'object' ? args.arguments : {}) as Record<string, unknown>;
  if (!tool) {
    try {
      await gopher!.check();
    } catch (e) {
      return fail((e as Error).message);
    }
    const tools = gopher!.tools ?? [];
    return { text: `② の道のツール（${tools.length} 個）。fl_gopher の tool に名前を、arguments に引数を渡して呼ぶ。? の付いた引数は省ける:\n${toolList(tools)}` };
  }
  const write = !isReadTool(tool);
  if (write && readOnly) return fail('FL Connect のウィンドウで書き込みを止めているので、FL を変えるツールは呼べません。');
  if (tool === 'remove_channels') return removeChannels(sessionId, toolArgs, signal);
  if (tool === 'add_channel' && midiRefusal()) return fail(`add_channel で足したチャンネルを覚えておくには、① の道が要ります（覚えていないと、消したあとの Ctrl+Z で FL が落ちるのを止められない）。${midiRefusal() ?? ''}`);
  if (tool === 'run_piano_roll_script') return fail('ピアノロールのスクリプトは fl_pianoroll で動かしてください。');
  const track = write && !midiRefusal();
  return exclusive(async () => {
    if (track) await refresh(ROW_SECTIONS, 'human');
    const r = await gopher!.call(tool, toolArgs).catch((e: Error) => e);
    if (r instanceof Error) {
      if (r instanceof GopherError && r.state === 'busy') setGopher('busy', r.message);
      return fail(r.message);
    }
    if (gopherStatus === 'busy') setGopher('connected');
    const changes = track ? (await refresh(ROW_SECTIONS, 'agent', `fl_gopher:${tool}`)).changes : [];
    if (tool === 'add_channel') addedChannels.push(...changes.filter((c) => c.section === 'channels' && c.field === 'added').map((c) => c.index));
    pushAgentOp(`fl_gopher:${tool}`, changes);
    if (write) addLog({ tool: 'fl_gopher', title: tool, detail: JSON.stringify(toolArgs), output: r.text, ok: !r.isError, changes: changeLines(changes) });
    const notes = changes.length ? `\n\n変わった値:\n${changeLines(changes).map((l) => `- ${l}`).join('\n')}` : '';
    return { text: `${r.text}（${r.ms} ms）${notes}`, isError: r.isError };
  });
};

/** fl_pianoroll：ピアノロールのスクリプトを、② の道の run_piano_roll_script で動かす。 */
const flPianoroll = async (args: Record<string, unknown>): Promise<FlToolResult> => {
  const code = typeof args.code === 'string' ? args.code : '';
  if (!code.trim()) return fail('code に、ピアノロールのスクリプト（flpianoroll を使う Python）を渡してください。');
  if (readOnly) return fail('FL Connect のウィンドウで書き込みを止めているので、ピアノロールのスクリプトは動かせません。');
  const why = gopherRefusal();
  if (why) return fail(`ピアノロールのスクリプトは、今は ② の道（FL 2026 の Gopher の中の口）でだけ動かせます。${why}`);
  return exclusive(async () => {
    const r = await gopher!.call('run_piano_roll_script', { source: code }).catch((e: Error) => e);
    if (r instanceof Error) return fail(r.message);
    addLog({ tool: 'fl_pianoroll', title: (code.trim().split('\n')[0] ?? '').slice(0, 80), detail: code, output: r.text, ok: !r.isError });
    return {
      text: `${r.text}（${r.ms} ms）\nFL は、スクリプトが例外を投げても「Script executed without errors」と返し、画面にもエラーを出さない。ノートが思ったとおりになったかは、人間に FL の画面で確かめてもらうこと。ピアノロールのスクリプトで書いたノートは FL の元に戻す履歴に残らず、そのあと人間が Ctrl+Z を押すと、ノートは残ったまま、その前の操作が戻る。`,
      isError: r.isError,
    };
  });
};

/** MCP のツールを動かす。どのツールも、ウィンドウでつないだセッションからだけ使える。signal は、エージェントが呼び出しを取り消したことを知らせる。 */
export const callFlTool = async (name: string, sessionId: string, args: Record<string, unknown>, signal?: AbortSignal): Promise<FlToolResult> => {
  const why = refusal(sessionId);
  if (why) return fail(why);
  try {
    switch (name) {
      case 'fl_changes':
        return flChanges(sessionId, args);
      case 'fl_gopher':
        return await flGopher(sessionId, args, signal);
      case 'fl_pianoroll':
        return await flPianoroll(args);
    }
    const noMidi = midiRefusal();
    if (noMidi) return fail(noMidi);
    switch (name) {
      case 'fl_state':
        return await flState(args);
      case 'fl_python':
        return await flPython(args);
      case 'fl_undo':
        return await flUndo(args);
      default:
        return fail(`知らないツールです: ${name}`);
    }
  } catch (e) {
    return fail(`FL Connect で失敗しました: ${(e as Error).message}`);
  }
};

export const FL_TOOL_NAMES = new Set(['fl_state', 'fl_changes', 'fl_python', 'fl_pianoroll', 'fl_gopher', 'fl_undo']);

/**
 * フックの応答に入れる、前に届けてから人間が FL でした操作。つないだセッションにだけ、ウィンドウが開いている間だけ返す。
 * エージェント自身の変更は、ツールの結果で知っているので入れない。
 */
export const flHookContext = (sessionId: string): string | undefined => {
  if (!windows.size || boundSession !== sessionId) return undefined;
  const since = cursors.get(sessionId) ?? records.last;
  cursors.set(sessionId, records.last);
  const human = records.since(since).filter((r) => r.who === 'human');
  if (!human.length) return undefined;
  const shown = human.slice(-HOOK_LIMIT);
  const rest = human.length > shown.length ? [`ほかに ${human.length - shown.length} 件あった。fl_changes に all: true を渡すと読める。`] : [];
  return [`FL Connect: 前に知らせてから、人間が FL Studio で次の操作をした（番号は MIDI スクリプトの API の番号）。`, ...shown.map(recordLine), ...rest].join('\n');
};
