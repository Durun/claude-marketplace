// 取り込んだ CD を、FLAC イメージ + cue + log + jpg と、曲ごとの mp3 に書き出す。手作業で使っていた cdrip finish の移植。
// 純粋な関数（sanitize、msf、cueSheet、flacTags、mp3Args、outputPaths、addedRows、gainDb）と、外部コマンドを動かす exportDisc に分ける。
import { type ChildProcess, spawn as realSpawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { CdMeta, CdRipSettings } from '../shared/types.ts';
import { listFlacs } from './cdripRip.ts';
import { run as runOffThread, type RunOptions } from './run.ts';

// Windows のファイル名に使えない文字は、見た目の近い全角に置き換える。
const FULLWIDTH: Record<string, string> = { '/': '／', '\\': '＼', ':': '：', '*': '＊', '?': '？', '"': '＂', '<': '＜', '>': '＞', '|': '｜' };

/** ファイル名の 1 区間に使える形にする。末尾の . と空白は Windows が削るので、先に除く。 */
export const sanitize = (s: string): string => s.replace(/[/\\:*?"<>|]/g, (c) => FULLWIDTH[c] ?? c).replace(/[. ]+$/, '');

/** CD のフレーム数（1/75 秒）を mm:ss:ff にする。 */
export const msf = (frames: number): string => `${String(Math.floor(frames / 4500)).padStart(2, '0')}:${String(Math.floor(frames / 75) % 60).padStart(2, '0')}:${String(frames % 75).padStart(2, '0')}`;

const FRAME_SAMPLES = 588;

/** cue の値は " で囲むので、値の中の " は全角の ” に置き換える。 */
const q = (s: string): string => s.replaceAll('"', '”');

const trackArtist = (meta: CdMeta, i: number): string => meta.tracks[i]?.artist || meta.artist;

/** FLAC イメージの名前。複数枚組は「<アルバム> (Disc N)」。 */
const imageName = (meta: CdMeta): string => {
  const name = sanitize(meta.album);
  return meta.disc === undefined ? name : `${name} (Disc ${meta.disc})`;
};

/**
 * 1 枚のイメージの cue。samples は取り込んだ各曲のサンプル数で、曲の頭の位置を積み上げて求める。
 * 値の中の " は ” にし、FILE の名前は書き出すファイル名と同じにする。
 */
export const cueSheet = (meta: CdMeta, samples: number[]): string => {
  const lines: string[] = [];
  if (meta.genre) lines.push(`REM GENRE "${q(meta.genre)}"`);
  if (meta.date) lines.push(`REM DATE ${meta.date.slice(0, 4)}`);
  if (meta.catalog) lines.push(`REM CATALOG "${q(meta.catalog)}"`);
  lines.push(`PERFORMER "${q(meta.artist)}"`, `TITLE "${q(meta.album)}"`, `FILE "${imageName(meta)}.flac" WAVE`);
  let frames = 0;
  samples.forEach((n, i) => {
    const t = meta.tracks[i];
    lines.push(`  TRACK ${String(i + 1).padStart(2, '0')} AUDIO`, `    TITLE "${q(t?.title ?? '')}"`, `    PERFORMER "${q(trackArtist(meta, i))}"`);
    if (t?.composer) lines.push(`    SONGWRITER "${q(t.composer)}"`);
    lines.push(`    INDEX 01 ${msf(frames)}`);
    frames += Math.floor(n / FRAME_SAMPLES);
  });
  return `${lines.join('\n')}\n`;
};

/** FLAC イメージに付ける Vorbis コメント（KEY=値）。 */
export const flacTags = (meta: CdMeta): string[] => [
  `ALBUM=${meta.album}`,
  `ALBUMARTIST=${meta.artist}`,
  `ARTIST=${meta.artist}`,
  `TITLE=${meta.album}`,
  `ORGANIZATION=${meta.circle}`,
  ...(meta.date ? [`DATE=${meta.date}`] : []),
  ...(meta.genre ? [`GENRE=${meta.genre}`] : []),
  ...(meta.catalog ? [`CATALOGNUMBER=${meta.catalog}`] : []),
  ...(meta.disc !== undefined ? [`DISCNUMBER=${meta.disc}`, `TOTALDISCS=${meta.totaldiscs ?? meta.disc}`] : []),
];

/** mp3 に付ける ID3 のタグ（ffmpeg の -metadata の引数）。 */
export const mp3Tags = (meta: CdMeta, i: number): string[] => {
  const t = meta.tracks[i];
  const tags: [string, string | undefined][] = [
    ['title', t?.title],
    ['artist', trackArtist(meta, i)],
    ['album', meta.album],
    ['album_artist', meta.artist],
    ['track', `${i + 1}/${meta.tracks.length}`],
    ['publisher', meta.circle],
    ['composer', t?.composer],
    ['date', meta.date],
    ['genre', meta.genre],
    ['disc', meta.disc === undefined ? undefined : `${meta.disc}/${meta.totaldiscs ?? meta.disc}`],
  ];
  return tags.flatMap(([k, v]) => (v ? ['-metadata', `${k}=${v}`] : []));
};

export type Mp3Options = { input: string; output: string; cover?: string; bitrate: CdRipSettings['features']['mp3Bitrate']; gainDb?: number };

/** 1 曲の mp3 を作る ffmpeg の引数。元の FLAC のタグは持ち込まず、必要なタグだけを付ける。 */
export const mp3Args = (meta: CdMeta, i: number, o: Mp3Options): string[] => [
  '-v', 'error', '-y', '-i', o.input,
  ...(o.cover ? ['-i', o.cover] : []),
  '-map', '0:a', ...(o.cover ? ['-map', '1:v'] : []),
  '-map_metadata', '-1',
  '-c:a', 'libmp3lame', ...(o.bitrate === 'V0' ? ['-q:a', '0'] : ['-b:a', o.bitrate]),
  ...(o.gainDb ? ['-af', `volume=${o.gainDb}dB`] : []),
  ...(o.cover ? ['-c:v', 'copy', '-disposition:v', 'attached_pic', '-metadata:s:v', 'title=Album cover', '-metadata:s:v', 'comment=Cover (front)'] : []),
  '-id3v2_version', '3',
  ...mp3Tags(meta, i),
  o.output,
];

const QUIET_PEAK_DB = -6;
const TARGET_PEAK_DB = -1;

/** ピークが -6 dBFS に届かない CD は、ピークが -1 dBFS になる分だけ持ち上げる。それ以外は 0。 */
export const gainDb = (maxVolumeDb: number): number => (maxVolumeDb < QUIET_PEAK_DB ? Math.round((TARGET_PEAK_DB - maxVolumeDb) * 10) / 10 : 0);

/** ffmpeg の volumedetect が標準エラーに書く max_volume（dB）。 */
export const maxVolumeOf = (stderr: string): number | undefined => {
  const m = /max_volume:\s*(-?[\d.]+)\s*dB/.exec(stderr);
  return m ? Number(m[1]) : undefined;
};

export type OutputPaths = { cdDir: string; name: string; flac: string; cue: string; log: string; jpg: string; mp3Dir: string; mp3Prefix: string; mp3: (i: number) => string };

/** 書き出し先のパス。複数枚組は、FLAC を 1 枚ずつ分け、mp3 は同じフォルダーに「N-01.」の形で並べる。 */
export const outputPaths = (meta: CdMeta, s: Pick<CdRipSettings, 'dest'>): OutputPaths => {
  const name = imageName(meta);
  const cdDir = path.join(s.dest.cd, sanitize(meta.circle));
  const mp3Dir = path.join(meta.dest === 'CLUB' ? s.dest.club : s.dest.music, sanitize(meta.circle), sanitize(meta.album));
  const mp3Prefix = meta.disc === undefined ? '' : `${meta.disc}-`;
  return {
    cdDir,
    name,
    flac: path.join(cdDir, `${name}.flac`),
    cue: path.join(cdDir, `${name}.cue`),
    log: path.join(cdDir, `${name}.log`),
    jpg: path.join(cdDir, `${name}.jpg`),
    mp3Dir,
    mp3Prefix,
    mp3: (i) => path.join(mp3Dir, `${mp3Prefix}${String(i + 1).padStart(2, '0')}.${sanitize(`${trackArtist(meta, i)} - ${meta.tracks[i]?.title ?? ''}`)}.mp3`),
  };
};

export const ADDED_HEADER = '追加日時\t種類\tdest\tサークル\tアルバム\t曲番号\tアーティスト\t曲名\tパス';

const stamp = (d: Date): string => {
  const p = (n: number): string => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
};

/** added.tsv に足す行。FLAC イメージ 1 行と、mp3 を 1 曲 1 行。 */
export const addedRows = (meta: CdMeta, p: OutputPaths, at: Date, kinds: { flac: boolean; mp3: boolean }): string[] => {
  const now = stamp(at);
  const rows: string[] = [];
  if (kinds.flac) rows.push([now, 'flac', 'CD', meta.circle, meta.album, '', meta.artist, '', p.flac].join('\t'));
  if (kinds.mp3) meta.tracks.forEach((t, i) => rows.push([now, 'mp3', meta.dest, meta.circle, meta.album, `${p.mp3Prefix}${i + 1}`, trackArtist(meta, i), t.title, p.mp3(i)].join('\t')));
  return rows;
};

/** 書き出せない理由（曲数の不一致、既にあるファイルなど）。人間が直せるので、そのまま画面に出す。 */
export class ExportError extends Error {}

export type Exec = (file: string, args: string[], options?: RunOptions) => Promise<{ stdout: string; stderr: string }>;

export type ExportDeps = { exec?: Exec; spawn?: typeof realSpawn; now?: () => Date };

export type ExportTools = { flac: string; metaflac: string; ffmpeg: string };

export type ExportInput = { dir: string; meta: CdMeta; settings: CdRipSettings; tools: ExportTools };

const CD_SAMPLE_RATE = 44100;
const RAW = ['--force-raw-format', '--endian=little', '--sign=signed'];

/** 曲ごとの FLAC をつないで 1 本のイメージにする。PCM をそのまま連結するので無劣化。 */
const encodeImage = (flac: string, tracks: string[], output: string, spawn: typeof realSpawn): Promise<void> =>
  new Promise((resolve, reject) => {
    let stderr = '';
    const enc = spawn(flac, ['-s', '-8', ...RAW, '--channels=2', '--bps=16', `--sample-rate=${CD_SAMPLE_RATE}`, '-o', output, '-'], { stdio: ['pipe', 'ignore', 'pipe'], windowsHide: true });
    enc.stderr?.on('data', (b: Buffer) => (stderr += b.toString()));
    // 先に落ちたエンコーダーへの書き込みの失敗は、close の終了コードで知らせる。
    enc.stdin?.on('error', () => undefined);
    enc.on('error', reject);
    enc.on('close', (code) => (code === 0 ? resolve() : reject(new Error(`flac でイメージを作れませんでした（終了コード ${code}）${stderr.trim()}`))));
    const decode = (t: string): Promise<void> =>
      new Promise((ok, ng) => {
        const dec: ChildProcess = spawn(flac, ['-d', '-s', '-c', ...RAW, t], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
        if (enc.stdin) dec.stdout?.pipe(enc.stdin, { end: false });
        dec.on('error', ng);
        dec.on('close', (code) => (code === 0 ? ok() : ng(new Error(`${path.basename(t)} を読めませんでした（終了コード ${code}）`))));
      });
    tracks
      .reduce((p, t) => p.then(() => decode(t)), Promise.resolve())
      .then(() => enc.stdin?.end())
      .catch((e: unknown) => {
        enc.kill();
        reject(e);
      });
  });

/** 別のドライブへも動かせるよう、コピーしてから元を消す。 */
const moveFile = (from: string, to: string): void => {
  fs.copyFileSync(from, to);
  fs.rmSync(from);
};

const coverFile = (dir: string, meta: CdMeta): string | undefined => {
  const f = meta.cover ? path.join(dir, meta.cover) : undefined;
  return f && fs.existsSync(f) ? f : undefined;
};

/** 既にある書き出し先を、上書きしないよう先に調べる。 */
const assertFree = (meta: CdMeta, p: OutputPaths, kinds: { flac: boolean; mp3: boolean }): void => {
  if (kinds.flac && fs.existsSync(p.flac)) throw new ExportError(`既にあります: ${p.flac}`);
  if (!kinds.mp3 || !fs.existsSync(p.mp3Dir)) return;
  if (meta.disc === undefined) throw new ExportError(`既にあります: ${p.mp3Dir}`);
  if (fs.readdirSync(p.mp3Dir).some((f) => f.startsWith(p.mp3Prefix))) throw new ExportError(`既にあります: ${path.join(p.mp3Dir, `${p.mp3Prefix}*`)}`);
};

/**
 * dir（取り込み先の行のフォルダー）の disc/*.flac を、meta に従って書き出す。書き出したファイルのパスを返す（FLAC イメージ、mp3 の順）。
 * 途中で失敗したら、この呼び出しで作ったファイルとフォルダーを消し、同じ内容でやり直せるようにする。
 */
export const exportDisc = async ({ dir, meta, settings, tools }: ExportInput, deps: ExportDeps = {}): Promise<{ files: string[]; rows: string[] }> => {
  const exec = deps.exec ?? ((file, args, options) => runOffThread(file, args, options));
  const spawn = deps.spawn ?? realSpawn;
  const { features } = settings;
  const kinds = { flac: features.flacImage, mp3: features.mp3 };
  const tracks = listFlacs(path.join(dir, 'disc'));
  if (tracks.length !== meta.tracks.length) throw new ExportError(`曲情報の曲数（${meta.tracks.length}）が取り込んだ曲数（${tracks.length}）と合いません`);
  const paths = outputPaths(meta, settings);
  assertFree(meta, paths, kinds);

  const createdDirs = [paths.cdDir, paths.mp3Dir].filter((d, i) => (i === 0 ? kinds.flac : kinds.mp3) && !fs.existsSync(d));
  const created: string[] = [];
  const work = path.join(dir, 'work');
  try {
    fs.rmSync(work, { recursive: true, force: true });
    fs.mkdirSync(work, { recursive: true });
    fs.writeFileSync(path.join(dir, 'meta.json'), JSON.stringify(meta, null, 2));
    const samples = await Promise.all(tracks.map(async (t) => Number((await exec(tools.metaflac, ['--show-total-samples', t])).stdout.trim())));
    const original = features.embedCover || kinds.flac ? coverFile(dir, meta) : undefined;
    // 埋め込み用のジャケットは長辺 1000px までに縮める。
    let art: string | undefined;
    if (original && features.embedCover) {
      art = path.join(work, 'cover.jpg');
      await exec(tools.ffmpeg, ['-v', 'error', '-y', '-i', original, '-vf', "scale='min(1000,iw)':'min(1000,ih)':force_original_aspect_ratio=decrease", '-q:v', '2', art]);
    }
    if (kinds.flac) {
      fs.mkdirSync(paths.cdDir, { recursive: true });
      const cue = cueSheet(meta, samples);
      fs.writeFileSync(path.join(work, 'image.cue'), cue);
      await encodeImage(tools.flac, tracks, path.join(work, 'image.flac'), spawn);
      const tagArgs = [...flacTags(meta).flatMap((t) => [`--set-tag=${t}`]), '--set-tag-from-file=CUESHEET=image.cue', '--import-cuesheet-from=image.cue', ...(art ? ['--import-picture-from=3||||cover.jpg'] : []), 'image.flac'];
      await exec(tools.metaflac, tagArgs, { cwd: work });
      moveFile(path.join(work, 'image.flac'), paths.flac);
      created.push(paths.flac);
      // 単体の cue は、文字コードを判別できるよう BOM を付ける。
      fs.writeFileSync(paths.cue, `﻿${cue}`);
      created.push(paths.cue);
      fs.copyFileSync(path.join(dir, 'disc', 'rip.log'), paths.log);
      created.push(paths.log);
      if (original) {
        if (!/\.jpe?g$/i.test(original)) await exec(tools.ffmpeg, ['-v', 'error', '-y', '-i', original, '-q:v', '2', paths.jpg]);
        else fs.copyFileSync(original, paths.jpg);
        created.push(paths.jpg);
      }
    }
    if (kinds.mp3) {
      fs.mkdirSync(paths.mp3Dir, { recursive: true });
      const gain = features.mp3Gain ? await detectGain(tools.ffmpeg, tracks, exec) : 0;
      for (const [i, t] of tracks.entries()) {
        const output = paths.mp3(i);
        await exec(tools.ffmpeg, mp3Args(meta, i, { input: t, output, cover: art, bitrate: features.mp3Bitrate, gainDb: gain }));
        created.push(output);
      }
    }
    const rows = addedRows(meta, paths, (deps.now ?? (() => new Date()))(), kinds);
    fs.rmSync(work, { recursive: true, force: true });
    return { files: created.filter((f) => f.endsWith('.flac') || f.endsWith('.mp3')), rows };
  } catch (e) {
    for (const f of created) fs.rmSync(f, { force: true });
    for (const d of createdDirs.reverse()) {
      try {
        fs.rmdirSync(d);
      } catch {
        // 中身が残っていれば、ほかのものが入っているので消さない。
      }
    }
    fs.rmSync(work, { recursive: true, force: true });
    throw e;
  }
};

/** 全曲のピークを調べて、CD 全体で同じ分だけ持ち上げる量を返す。 */
const detectGain = async (ffmpeg: string, tracks: string[], exec: Exec): Promise<number> => {
  const peaks = await Promise.all(tracks.map(async (t) => maxVolumeOf((await exec(ffmpeg, ['-hide_banner', '-nostats', '-i', t, '-af', 'volumedetect', '-vn', '-f', 'null', '-'])).stderr)));
  const known = peaks.filter((p): p is number => p !== undefined);
  return known.length ? gainDb(Math.max(...known)) : 0;
};

/** added.tsv に行を足す。無ければ見出しから作る。 */
export const appendAdded = (file: string, rows: string[]): void => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  if (!fs.existsSync(file)) fs.writeFileSync(file, `${ADDED_HEADER}\n`);
  fs.appendFileSync(file, `${rows.join('\n')}\n`);
};
