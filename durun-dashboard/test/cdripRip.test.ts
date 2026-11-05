import assert from 'node:assert/strict';
import type { ChildProcess, spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import fs from 'node:fs';
import path from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { cyanripArgs, listFlacs, parseCyanripOutput, startRip } from '../server/cdripRip.ts';
import { DEFAULT_FEATURES } from '../server/cdrip.ts';
import { tempDir } from './tmp.ts';

// 本物の cyanrip 0.9.3 の出力（aquamarine、8 曲）。進捗の行は曲ごとに先頭の 1 行だけ残してある。
const REAL = fs.readFileSync(path.join(import.meta.dirname, 'fixtures', 'cyanrip-aquamarine.out'), 'utf8');

test('本物の cyanrip.out から、曲数、全体の長さ、ディスク ID、曲ごとの長さを読み、全曲を取り込み終えたことを知る', () => {
  const p = parseCyanripOutput(REAL);
  assert.equal(p.tracks, 8);
  assert.equal(p.totalSeconds, 1517);
  assert.equal(p.tocId, 'v7ZYUHTTYFdt.wXk07cnmAsD8TU-');
  assert.equal(p.durations.length, 8);
  assert.equal(p.durations[0], 223);
  assert.equal(p.finished, true);
  assert.equal(p.error, undefined);
  // CD-Text の無いディスクは Unknown disc と Unknown track になるので、CD-Text として読まない。
  assert.equal(p.cdText, undefined);
});

test('取り込みの途中の出力からは、曲数と進み具合を読み、まだ終わっていないとする。AccurateRip の DB が無い行はエラーにしない', () => {
  const head = REAL.split('\n').slice(0, 40).join('\n');
  const p = parseCyanripOutput(`${head}\rRipping and encoding track 1, progress - 52.30%, ETA - 1m 2s\rRipping and encoding track 1, progress - 52.40%, ETA - 1m 1s`);
  assert.equal(p.tracks, 8);
  assert.deepEqual(p.progress, { track: 1, percent: 52.4 });
  assert.equal(p.finished, false);
  assert.equal(p.error, undefined);
});

test('CD-Text があるディスクは、アルバム名とアーティストと曲名を読む', () => {
  const text = ['Disc tracks:    2', 'Album:          Real Album', 'Album artist:   Real Artist', 'Tracks:', '  Metadata:', '    title:                         First', '    title:                         Unknown track', '    title:                         Second'].join('\n');
  assert.deepEqual(parseCyanripOutput(text).cdText, { album: 'Real Album', artist: 'Real Artist', titles: ['First', 'Second'] });
});

test('取り込みのエラーの数や、ドライブを開けない行は error にする', () => {
  assert.match(parseCyanripOutput('Disc tracks: 3\nRipping errors: 2').error ?? '', /2 件/);
  assert.equal(parseCyanripOutput('Error opening drive F:').error, 'Error opening drive F:');
});

test('cyanrip の引数は、ドライブ、オフセット、paranoia を渡し、検証の回数が 0 のときは -Z を付けない', () => {
  const base = { offset: 667, paranoia: 3, features: DEFAULT_FEATURES };
  const args = cyanripArgs(base, 'F:');
  assert.deepEqual(args.slice(0, 6), ['-d', 'F:', '-s', '667', '-P', '3']);
  assert.ok(!args.includes('-Z'));
  assert.deepEqual(args.slice(-12), ['-N', '-U', '-o', 'flac', '-D', 'disc', '-F', '{track}', '-L', 'rip', '-M', 'rip']);
  const verified = cyanripArgs({ ...base, features: { ...DEFAULT_FEATURES, verify: 2 } }, 'F:');
  assert.deepEqual(verified.slice(6, 8), ['-Z', '2']);
});

type Fake = { child: ChildProcess; stdout: PassThrough; killed: () => boolean };

const fakeChild = (): Fake => {
  const child = new EventEmitter() as ChildProcess;
  const stdout = new PassThrough();
  Object.assign(child, { stdout, stderr: new PassThrough() });
  return { child, stdout, killed: () => false };
};

test('取り込みを起動し、届いた出力を cyanrip.out に残しながら進み具合を渡し、終わったら全文の解析結果を返す', async () => {
  const dir = tempDir('cdrip-rip-');
  const fake = fakeChild();
  const updates: (number | undefined)[] = [];
  const rip = startRip('cyanrip', [], dir, (p) => updates.push(p.progress?.track), { spawn: (() => fake.child) as unknown as typeof spawn });
  const lines = REAL.split('\n');
  for (let i = 0; i < lines.length; i += 50) fake.stdout.write(`${lines.slice(i, i + 50).join('\r')}\r`);
  fake.child.emit('close', 0);
  const result = await rip.done;
  assert.equal(result.code, 0);
  assert.equal(result.aborted, false);
  assert.equal(result.parsed.finished, true);
  assert.ok(updates.includes(8));
  assert.ok(fs.readFileSync(path.join(dir, 'cyanrip.out'), 'utf8').includes('Disc tracks:    8'));
});

test('中断したときは木ごと止めて aborted を真にし、起動できないときは理由を error に入れる', async () => {
  const dir = tempDir('cdrip-rip-');
  const fake = fakeChild();
  let killedChild: ChildProcess | undefined;
  const rip = startRip('cyanrip', [], dir, () => undefined, { spawn: (() => fake.child) as unknown as typeof spawn, killTree: (c) => (killedChild = c) });
  rip.abort();
  fake.child.emit('close', 1);
  const result = await rip.done;
  assert.equal(killedChild, fake.child);
  assert.deepEqual([result.aborted, result.code], [true, 1]);

  const broken = fakeChild();
  const failing = startRip('cyanrip', [], dir, () => undefined, { spawn: (() => broken.child) as unknown as typeof spawn });
  broken.child.emit('error', new Error('ENOENT'));
  assert.match((await failing.done).parsed.error ?? '', /cyanrip を起動できません: ENOENT/);
});

test('disc/ の FLAC は、曲番号の数値の順に並べる', () => {
  const dir = tempDir('cdrip-disc-');
  for (const n of [10, 2, 1, 9]) fs.writeFileSync(path.join(dir, `${n}.flac`), '');
  fs.writeFileSync(path.join(dir, 'rip.log'), '');
  assert.deepEqual(listFlacs(dir).map((f) => path.basename(f)), ['1.flac', '2.flac', '9.flac', '10.flac']);
});
