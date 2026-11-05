import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { DEFAULT_FEATURES } from '../server/cdrip.ts';
import { addedRows, ADDED_HEADER, appendAdded, cueSheet, type Exec, exportDisc, ExportError, flacTags, gainDb, maxVolumeOf, mp3Args, mp3Tags, msf, outputPaths, sanitize } from '../server/cdripExport.ts';
import { resolveTool } from '../server/cdripTools.ts';
import type { CdMeta, CdRipSettings } from '../shared/types.ts';
import { tempDir } from './tmp.ts';

// aquamarine（Synthion、8 曲）を、手作業の cdrip finish で書き出したときの入力と結果。サンプル数は取り込んだ各曲の cyanrip.out の Samples。
const SAMPLES = [9859584, 8079708, 8787072, 6963096, 6725544, 2866500, 9890748, 13733916];
const TITLES = ['aquamarine', 'flow', 'high tide', 'asphyxia', 'thalassophobia', 'yunseul', 'stasis', 'deep sea adventure'];
const AQUA: CdMeta = { circle: 'Synthion', album: 'aquamarine', artist: 'Synthion', date: '2026-10-25', genre: 'Electronic', dest: 'CLUB', cover: 'cover.jpg', tracks: TITLES.map((title) => ({ title, artist: 'Synthion' })) };
const SETTINGS: CdRipSettings = {
  drive: 'F:', driveModel: '', offset: 667, paranoia: 3, musicDir: path.join('m'), stageDir: path.join('m', 'stage'),
  dest: { cd: path.join('m', 'CD'), club: path.join('m', 'CLUB'), music: path.join('m', 'Music') },
  features: { ...DEFAULT_FEATURES },
};

test('ファイル名の禁止文字は全角にし、末尾の . と空白を除く', () => {
  assert.equal(sanitize('A/B\\C:D*E?F"G<H>I|J'), 'A／B＼C：D＊E？F＂G＜H＞I｜J');
  assert.equal(sanitize('Mr. Wave... '), 'Mr. Wave');
  assert.equal(sanitize('plain'), 'plain');
});

test('フレーム数は mm:ss:ff にする', () => {
  assert.deepEqual([0, 16768, 4500, 74, 75].map(msf), ['00:00:00', '03:43:43', '01:00:00', '00:00:74', '00:01:00']);
});

test('手作業で書き出した aquamarine の cue と、同じ内容の cue を作る', () => {
  assert.equal(cueSheet(AQUA, SAMPLES), fs.readFileSync(path.join(import.meta.dirname, 'fixtures', 'aquamarine.cue'), 'utf8'));
});

test('cue は、作曲者を SONGWRITER に書き、値の中の " を ” にし、曲のアーティストが無ければアルバムのアーティストを使い、複数枚組は FILE に (Disc N) を付ける', () => {
  const meta: CdMeta = { circle: 'C', album: 'A "x"', artist: 'AA', catalog: 'CAT-1', dest: 'Music', disc: 2, totaldiscs: 2, tracks: [{ title: 'T "1"', composer: 'Comp' }, { title: 'T2', artist: 'Other' }] };
  const cue = cueSheet(meta, [588 * 75, 588]);
  assert.match(cue, /REM CATALOG "CAT-1"/);
  assert.match(cue, /TITLE "A ”x”"/);
  assert.match(cue, /FILE "A ＂x＂ \(Disc 2\)\.flac" WAVE/);
  assert.match(cue, /TITLE "T ”1”"\n {4}PERFORMER "AA"\n {4}SONGWRITER "Comp"\n {4}INDEX 01 00:00:00/);
  assert.match(cue, /PERFORMER "Other"\n {4}INDEX 01 00:01:00/);
});

test('FLAC のタグは手作業と同じ順で、複数枚組は DISCNUMBER と TOTALDISCS を足す', () => {
  assert.deepEqual(flacTags(AQUA), ['ALBUM=aquamarine', 'ALBUMARTIST=Synthion', 'ARTIST=Synthion', 'TITLE=aquamarine', 'ORGANIZATION=Synthion', 'DATE=2026-10-25', 'GENRE=Electronic']);
  assert.deepEqual(flacTags({ ...AQUA, catalog: 'X', disc: 1, totaldiscs: 2 }).slice(-3), ['CATALOGNUMBER=X', 'DISCNUMBER=1', 'TOTALDISCS=2']);
});

test('mp3 の引数は、手作業の ffmpeg と同じタグ（ID3v2.3、320k、ジャケットを attached_pic）にし、V0 と音量の持ち上げと作曲者に対応する', () => {
  const args = mp3Args(AQUA, 0, { input: '1.flac', output: 'o.mp3', cover: 'c.jpg', bitrate: '320k' });
  assert.deepEqual(args.slice(0, 16), ['-v', 'error', '-y', '-i', '1.flac', '-i', 'c.jpg', '-map', '0:a', '-map', '1:v', '-map_metadata', '-1', '-c:a', 'libmp3lame', '-b:a']);
  assert.ok(args.includes('attached_pic') && args.includes('-id3v2_version'));
  assert.equal(args.at(-1), 'o.mp3');
  assert.deepEqual(mp3Tags(AQUA, 0), ['-metadata', 'title=aquamarine', '-metadata', 'artist=Synthion', '-metadata', 'album=aquamarine', '-metadata', 'album_artist=Synthion', '-metadata', 'track=1/8', '-metadata', 'publisher=Synthion', '-metadata', 'date=2026-10-25', '-metadata', 'genre=Electronic']);
  const plain = mp3Args({ ...AQUA, disc: 1, totaldiscs: 2, tracks: [{ title: 'a', composer: 'c' }] }, 0, { input: 'i', output: 'o', bitrate: 'V0', gainDb: 2.5 });
  assert.ok(plain.join(' ').includes('-q:a 0') && plain.join(' ').includes('-af volume=2.5dB') && plain.includes('composer=c') && plain.includes('disc=1/2'));
  assert.ok(!plain.includes('-map') || !plain.includes('1:v'));
});

test('音量の持ち上げは、ピークが -6 dBFS に届かない CD だけ、-1 dBFS になる分にする', () => {
  assert.deepEqual([-20, -6, -5.9, -12.3, 0].map(gainDb), [19, 0, 0, 11.3, 0]);
  assert.equal(maxVolumeOf('[Parsed_volumedetect_0] mean_volume: -30.1 dB\n[Parsed_volumedetect_0] max_volume: -12.3 dB'), -12.3);
  assert.equal(maxVolumeOf('nothing'), undefined);
});

test('書き出し先は、CD の下にサークル、mp3 は CLUB か Music の下にサークルとアルバムで分け、複数枚組は FLAC を分けて mp3 を N-01. の形で並べる', () => {
  const p = outputPaths(AQUA, SETTINGS);
  assert.equal(p.flac, path.join(SETTINGS.dest.cd, 'Synthion', 'aquamarine.flac'));
  assert.equal(p.mp3(0), path.join(SETTINGS.dest.club, 'Synthion', 'aquamarine', '01.Synthion - aquamarine.mp3'));
  const multi = outputPaths({ ...AQUA, album: 'Box: Vol.1', dest: 'Music', disc: 2, totaldiscs: 2 }, SETTINGS);
  assert.equal(multi.flac, path.join(SETTINGS.dest.cd, 'Synthion', 'Box： Vol.1 (Disc 2).flac'));
  assert.equal(multi.mp3(2), path.join(SETTINGS.dest.music, 'Synthion', 'Box： Vol.1', '2-03.Synthion - high tide.mp3'));
});

test('added.tsv の行は、手作業の記録と同じ列で、FLAC イメージ 1 行と mp3 を 1 曲 1 行にする', () => {
  const p = outputPaths(AQUA, { dest: { cd: 'D:\\Music\\CD', club: 'D:\\Music\\CLUB', music: 'D:\\Music\\Music' } });
  const rows = addedRows(AQUA, p, new Date(2026, 9, 27, 0, 36, 25), { flac: true, mp3: true });
  assert.equal(rows.length, 9);
  assert.equal(rows[0], ['2026-10-27 00:36:25', 'flac', 'CD', 'Synthion', 'aquamarine', '', 'Synthion', '', p.flac].join('\t'));
  assert.equal(rows[1]?.split('\t').slice(0, 8).join('|'), '2026-10-27 00:36:25|mp3|CLUB|Synthion|aquamarine|1|Synthion|aquamarine');
  assert.equal(addedRows(AQUA, p, new Date(), { flac: false, mp3: true }).length, 8);

  const dir = tempDir('cdrip-added-');
  const file = path.join(dir, 'added.tsv');
  appendAdded(file, rows.slice(0, 1));
  appendAdded(file, rows.slice(1, 2));
  assert.deepEqual(fs.readFileSync(file, 'utf8').split('\n').slice(0, 2), [ADDED_HEADER, rows[0]]);
});

// 本物の flac と ffmpeg で、小さな 2 曲の CD を書き出す。道具が無い PC では飛ばす。
const flac = resolveTool('flac');
const metaflac = resolveTool('metaflac');
const ffmpeg = resolveTool('ffmpeg');
const ffprobe = resolveTool('ffprobe');
const TOOLS = flac && metaflac && ffmpeg && ffprobe ? { flac, metaflac, ffmpeg } : undefined;

const makeDisc = (): { dir: string; settings: CdRipSettings; meta: CdMeta } => {
  const root = tempDir('cdrip-e2e-');
  const dir = path.join(root, 'stage', 'item1');
  fs.mkdirSync(path.join(dir, 'disc'), { recursive: true });
  for (const n of [1, 2]) execFileSync(ffmpeg ?? '', ['-v', 'error', '-f', 'lavfi', '-i', `sine=frequency=${300 * n}:duration=1`, '-ar', '44100', '-ac', '2', '-sample_fmt', 's16', path.join(dir, 'disc', `${n}.flac`)]);
  fs.writeFileSync(path.join(dir, 'disc', 'rip.log'), 'log');
  execFileSync(ffmpeg ?? '', ['-v', 'error', '-f', 'lavfi', '-i', 'color=c=red:s=1600x900', '-frames:v', '1', path.join(dir, 'cover.jpg')]);
  const settings: CdRipSettings = { ...SETTINGS, stageDir: path.join(root, 'stage'), dest: { cd: path.join(root, 'CD'), club: path.join(root, 'CLUB'), music: path.join(root, 'Music') } };
  const meta: CdMeta = { circle: 'Circ/le', album: 'Alb: um', artist: 'Art', date: '2026-10-25', genre: 'Hardcore', dest: 'CLUB', cover: 'cover.jpg', tracks: [{ title: 'One', composer: 'Comp' }, { title: 'Two', artist: 'Other' }] };
  return { dir, settings, meta };
};

test('実物の flac と ffmpeg で、FLAC イメージ（cue とジャケットを埋め込み）、cue、log、jpg、mp3 を書き出し、記録の行を返す', { skip: !TOOLS && 'flac と ffmpeg が無い' }, async () => {
  const { dir, settings, meta } = makeDisc();
  const { files, rows } = await exportDisc({ dir, meta, settings, tools: TOOLS! });
  const p = outputPaths(meta, settings);
  assert.deepEqual(files, [p.flac, p.mp3(0), p.mp3(1)]);
  assert.equal(rows.length, 3);
  assert.ok([p.cue, p.log, p.jpg].every((f) => fs.existsSync(f)));
  assert.equal(fs.readFileSync(p.cue, 'utf8').charCodeAt(0), 0xfeff);
  assert.equal(fs.readFileSync(p.log, 'utf8'), 'log');
  const info = execFileSync(metaflac!, ['--list', '--block-type=VORBIS_COMMENT,PICTURE,CUESHEET', p.flac], { encoding: 'utf8' });
  assert.match(info, /ALBUM=Alb: um/);
  assert.match(info, /ORGANIZATION=Circ\/le/);
  assert.match(info, /type: 6 \(PICTURE\)/);
  assert.match(info, /type: 5 \(CUESHEET\)/);
  assert.match(info, /number of tracks: 3/);
  const tags = execFileSync(ffprobe!, ['-v', 'error', '-show_entries', 'format_tags=title,artist,composer,track,album_artist', '-of', 'default=nw=1', p.mp3(1)], { encoding: 'utf8' });
  assert.match(tags, /TAG:artist=Other/);
  assert.match(tags, /TAG:track=2\/2/);
  assert.match(tags, /TAG:album_artist=Art/);
  assert.ok(!fs.existsSync(path.join(dir, 'work')));
  // 同じ内容をもう一度書き出そうとすると、既にあるとして断る。
  await assert.rejects(exportDisc({ dir, meta, settings, tools: TOOLS! }), ExportError);
});

test('書き出しの途中で失敗したら、その呼び出しで作ったファイルとフォルダーを消し、同じ内容でやり直せる', { skip: !TOOLS && 'flac と ffmpeg が無い' }, async () => {
  const { dir, settings, meta } = makeDisc();
  const real: Exec = async (file, args, options) => (await import('../server/run.ts')).run(file, args, options);
  const failing: Exec = (file, args, options) => (args.includes('libmp3lame') && args.at(-1)?.includes('02.') ? Promise.reject(new Error('ffmpeg が失敗')) : real(file, args, options));
  await assert.rejects(exportDisc({ dir, meta, settings, tools: TOOLS! }, { exec: failing }), /ffmpeg が失敗/);
  const p = outputPaths(meta, settings);
  assert.ok(!fs.existsSync(p.cdDir) && !fs.existsSync(p.mp3Dir));
  assert.ok(!fs.existsSync(path.join(dir, 'work')));
  await exportDisc({ dir, meta, settings, tools: TOOLS! });
  assert.ok(fs.existsSync(p.flac));
});

test('曲数が取り込んだ曲数と合わなければ、何も作らずに断る', { skip: !TOOLS && 'flac と ffmpeg が無い' }, async () => {
  const { dir, settings, meta } = makeDisc();
  await assert.rejects(exportDisc({ dir, meta: { ...meta, tracks: [{ title: 'only' }] }, settings, tools: TOOLS! }), /曲数/);
  assert.ok(!fs.existsSync(settings.dest.cd));
});
