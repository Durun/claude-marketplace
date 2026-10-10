import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import zlib from 'node:zlib';
import { compress, compressedFile, compressedStream, etagOf, notModified, pickEncoding } from '../server/compress.ts';

test('pickEncoding は、ブラウザが受けられる中から brotli を先に選び、q=0 で断られたものと知らないものは選ばない', () => {
  assert.equal(pickEncoding('gzip, deflate, br, zstd'), 'br');
  assert.equal(pickEncoding('gzip, deflate'), 'gzip');
  assert.equal(pickEncoding('br;q=0, GZIP;q=0.5'), 'gzip');
  assert.equal(pickEncoding('gzip;q=0'), undefined);
  assert.equal(pickEncoding('identity'), undefined);
  assert.equal(pickEncoding(undefined), undefined);
});

test('notModified は、If-None-Match に並んだ ETag のどれかが今のものと同じときだけ true を返す', () => {
  const etag = etagOf({ mtimeMs: 1_700_000_000_123.4, size: 2048 });
  assert.match(etag, /^W\/"[0-9a-z]+-[0-9a-z]+"$/);
  assert.equal(notModified(etag, etag), true);
  assert.equal(notModified(`"other", ${etag}`, etag), true);
  assert.equal(notModified(etagOf({ mtimeMs: 1_700_000_000_123.4, size: 2049 }), etag), false);
  assert.equal(notModified(undefined, etag), false);
});

test('compress は、選んだ縮め方で元に戻せる形に縮める', async () => {
  const raw = Buffer.from(JSON.stringify({ text: 'あいうえお'.repeat(500) }));
  const br = await compress(raw, 'br');
  const gz = await compress(raw, 'gzip');
  assert.ok(br.length < raw.length / 10);
  assert.deepEqual(zlib.brotliDecompressSync(br), raw);
  assert.deepEqual(zlib.gunzipSync(gz), raw);
});

test('compressedFile は、etag が変わるまで縮めた結果を使い回し、変わったら縮め直す', async () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-compress-')), 'app.js');
  fs.writeFileSync(file, 'const a = 1;'.repeat(200));
  const first = compressedFile(file, 'v1', 'gzip');
  assert.equal(compressedFile(file, 'v1', 'gzip'), first);
  fs.writeFileSync(file, 'const b = 2;'.repeat(200));
  assert.equal(zlib.gunzipSync(await compressedFile(file, 'v2', 'gzip')).toString(), 'const b = 2;'.repeat(200));
});

test('compressedStream は、送るたびにそこまでを読める形で流し、brotli では前とほとんど同じ内容を小さく送る', async () => {
  // 圧縮が覚えている範囲に収まらないと差が出ないので、スナップショットと同じくらいの、縮みにくい内容にする。
  const sessions = Array.from({ length: 400 }, (_, i) => ({ id: i, text: Buffer.from(`${i}:${Math.sin(i)}`).toString('base64').repeat(4) }));
  const event = (now: number): string => `data: ${JSON.stringify({ now, sessions })}\n\n`;
  const out = new PassThrough();
  const sizes: number[] = [];
  const unpacked: string[] = [];
  const unpack = zlib.createBrotliDecompress();
  unpack.on('data', (chunk: Buffer) => unpacked.push(chunk.toString()));
  out.on('data', (chunk: Buffer) => {
    sizes[sizes.length - 1]! += chunk.length;
    unpack.write(chunk);
  });
  const send = compressedStream(out, 'br');
  const settle = () => new Promise((resolve) => setTimeout(resolve, 100));
  for (const now of [1, 2]) {
    sizes.push(0);
    send(event(now));
    await settle();
  }
  assert.equal(unpacked.join(''), event(1) + event(2));
  assert.ok(sizes[1]! < sizes[0]! / 20, `2 回目は ${sizes[1]} バイト、1 回目は ${sizes[0]} バイト`);
  out.destroy();
});

test('compressedStream は、縮め方が無ければそのまま流す', () => {
  const out = new PassThrough();
  compressedStream(out, undefined)('data: x\n\n');
  assert.equal(out.read().toString(), 'data: x\n\n');
});
