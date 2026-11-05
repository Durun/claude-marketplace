// cyanrip の起動と、出力の解析。出力は取り込み先の cyanrip.out に書き残しつつ、曲数・曲の長さ・CD-Text・進み具合を読む。
import { type ChildProcess, spawn as realSpawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { CdRipSettings } from '../shared/types.ts';
import { platform } from './platform.ts';
import { run as runOffThread } from './run.ts';

/** cyanrip が読み取った 1 枚の CD の情報。 */
export type ParsedRip = {
  tracks?: number;
  /** ディスク全体の長さ（秒）。曲ごとの長さは、曲を取り込むたびに出る Duration から読む。 */
  totalSeconds?: number;
  tocId?: string;
  cdText?: { album?: string; artist?: string; titles?: string[] };
  /** 取り込み中の曲と、その曲の進み（0〜100）。 */
  progress?: { track: number; percent: number };
  /** 取り込み終えた曲の長さ（秒）。曲の順に並ぶ。 */
  durations: number[];
  /** 全曲を取り込み終えた。 */
  finished: boolean;
  error?: string;
};

const PROGRESS = /^Ripping and encoding track (\d+), progress - ([\d.]+)%/;
const UNKNOWN_ALBUM = /^Unknown disc/i;
const UNKNOWN_TRACK = /^Unknown track$/i;
const FATAL = /^(?:Error|Failed|Unable to (?:open|read|find|get a handle))/i;

const seconds = (hms: string): number => {
  const [h = '0', m = '0', s = '0'] = hms.split(':');
  return Number(h) * 3600 + Number(m) * 60 + Number(s);
};

/** 出力の全文（進捗の行は \r で続く）から、読み取れたことを集める。 */
export const parseCyanripOutput = (text: string): ParsedRip => {
  const out: ParsedRip = { durations: [], finished: false };
  const titles: string[] = [];
  const cdText: NonNullable<ParsedRip['cdText']> = {};
  for (const raw of text.split(/[\r\n]+/)) {
    const line = raw.trim();
    if (!line) continue;
    const p = PROGRESS.exec(line);
    if (p) {
      out.progress = { track: Number(p[1]), percent: Number(p[2]) };
      continue;
    }
    const kv = /^([A-Za-z][A-Za-z ]*?):\s+(.*)$/.exec(line);
    const [key, value] = [kv?.[1] ?? '', kv?.[2]?.trim() ?? ''];
    if (key === 'Disc tracks') out.tracks = Number(value);
    else if (key === 'DiscID') out.tocId = value;
    else if (key === 'Total time') out.totalSeconds = Math.round(seconds(value));
    // CD-Text があるときだけ、cyanrip は曲名や題を載せる。無いときは Unknown disc と Unknown track になる。
    else if (key === 'Album' && value && !UNKNOWN_ALBUM.test(value) && out.tracks !== undefined && !out.durations.length) cdText.album = value;
    else if ((key === 'Album artist' || key === 'Album Artist') && value) cdText.artist = value;
    else if (key === 'title' && value && !UNKNOWN_TRACK.test(value)) titles.push(value);
    else if (key === 'Duration') out.durations.push(Math.round(seconds(value)));
    else if (FATAL.test(line) && !/AccuRIP/i.test(line)) out.error = line;
    else if (key === 'Ripping errors' && Number(value) > 0) out.error = `取り込みで ${value} 件のエラーがありました`;
  }
  if (cdText.album || cdText.artist || titles.length) out.cdText = { ...cdText, ...(titles.length ? { titles } : {}) };
  out.finished = out.tracks !== undefined && out.tracks > 0 && out.durations.length === out.tracks;
  return out;
};

/** cyanrip の引数。cwd は行の取り込み先で、曲は disc/<曲番号>.flac、log と cue は disc/rip.* に書かれる。 */
export const cyanripArgs = (s: Pick<CdRipSettings, 'offset' | 'paranoia' | 'features'>, device: string): string[] => [
  '-d', device,
  '-s', String(s.offset),
  '-P', String(s.paranoia),
  ...(s.features.verify > 0 ? ['-Z', String(s.features.verify)] : []),
  '-N', '-U', '-o', 'flac', '-D', 'disc', '-F', '{track}', '-L', 'rip', '-M', 'rip',
];

/** 進捗の行は 1 秒に何度も出て長くなるので、解析に使う分だけ残す。 */
const keepForParse = (chunk: string): string =>
  chunk
    .split(/(?<=[\r\n])/)
    .filter((l) => !PROGRESS.test(l.trim()))
    .join('');

export type RipResult = { code: number | null; parsed: ParsedRip; aborted: boolean };

export type RipDeps = { spawn?: typeof realSpawn; killTree?: (child: ChildProcess) => void };

/**
 * cyanrip を起動する。出力は cwd の cyanrip.out に書き、届くたびに onUpdate へ解析結果を渡す。
 * abort は cyanrip を木ごと止める。止めたときは aborted を真にして返す。
 */
export const startRip = (file: string, args: string[], cwd: string, onUpdate: (p: ParsedRip) => void, deps: RipDeps = {}): { done: Promise<RipResult>; abort: () => void } => {
  const child = (deps.spawn ?? realSpawn)(file, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  const out = fs.createWriteStream(path.join(cwd, 'cyanrip.out'));
  let slim = '';
  let latest: ParsedRip['progress'];
  let aborted = false;
  const onData = (buf: Buffer): void => {
    const chunk = buf.toString('utf8');
    out.write(buf);
    slim += keepForParse(chunk);
    const lines = chunk.split(/[\r\n]+/).reverse();
    for (const l of lines) {
      const m = PROGRESS.exec(l.trim());
      if (m) {
        latest = { track: Number(m[1]), percent: Number(m[2]) };
        break;
      }
    }
    onUpdate({ ...parseCyanripOutput(slim), progress: latest });
  };
  child.stdout?.on('data', onData);
  child.stderr?.on('data', onData);
  const done = new Promise<RipResult>((resolve) => {
    const finish = (code: number | null, spawnError?: string): void => {
      const parsed = parseCyanripOutput(slim);
      // cyanrip.out を書き終えてから返す。呼び手がすぐに読むため。
      out.end(() => resolve({ code, parsed: spawnError ? { ...parsed, error: spawnError } : parsed, aborted }));
    };
    child.on('error', (e) => finish(null, `cyanrip を起動できません: ${e.message}`));
    child.on('close', (code) => finish(code));
  });
  return {
    done,
    abort: () => {
      aborted = true;
      (deps.killTree ?? ((c) => platform.killTree(c)))(child);
    },
  };
};

/** disc/ の曲の FLAC を曲番号の順に並べる。 */
export const listFlacs = (discDir: string): string[] =>
  fs
    .readdirSync(discDir)
    .filter((f) => /^\d+\.flac$/.test(f))
    .sort((a, b) => parseInt(a, 10) - parseInt(b, 10))
    .map((f) => path.join(discDir, f));

/** 取り込んだ曲ごとの長さ（サンプル数）。CD は 44.1kHz で、秒は samples / 44100。 */
export const trackSamples = async (metaflac: string, flacs: string[]): Promise<number[]> =>
  Promise.all(flacs.map(async (f) => Number((await runOffThread(metaflac, ['--show-total-samples', f])).stdout.trim())));
