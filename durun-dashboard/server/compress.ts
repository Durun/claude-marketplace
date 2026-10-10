// 応答を縮めて送るための道具と、変わっていないファイルを送り直さないための ETag。
import fs from 'node:fs';
import type { Writable } from 'node:stream';
import { promisify } from 'node:util';
import zlib from 'node:zlib';

export type Encoding = 'br' | 'gzip';

const ENCODINGS: Encoding[] = ['br', 'gzip'];
// 作り直すまで同じファイルは、時間をかけて小さくする。API の応答は、待たせないよう軽く縮める。
const FILE_QUALITY = 9;
const LIVE_QUALITY = 4;

const brotli = promisify(zlib.brotliCompress);
const gzip = promisify(zlib.gzip);

/** Accept-Encoding から、ブラウザが受けられる縮め方を選ぶ。q=0 で断られたものは使わない。 */
export const pickEncoding = (accept: string | undefined): Encoding | undefined => {
  const accepted = new Set(
    (accept ?? '')
      .split(',')
      .map((part) => part.trim().toLowerCase().split(/\s*;\s*q=/))
      .filter(([, q]) => q === undefined || Number(q) > 0)
      .map(([name]) => name),
  );
  return ENCODINGS.find((e) => accepted.has(e));
};

/** 縮め方が違っても同じ値を返すので、弱い ETag にする。 */
export const etagOf = (stat: { mtimeMs: number; size: number }): string => `W/"${Math.round(stat.mtimeMs).toString(36)}-${stat.size.toString(36)}"`;

/** If-None-Match に、いまの ETag が入っているか。入っていれば、ブラウザが持っているものをそのまま使わせる。 */
export const notModified = (ifNoneMatch: string | undefined, etag: string): boolean =>
  (ifNoneMatch ?? '').split(',').some((tag) => tag.trim() === etag || tag.trim() === '*');

export const compress = (raw: Buffer, encoding: Encoding, quality = LIVE_QUALITY): Promise<Buffer> =>
  encoding === 'br' ? brotli(raw, { params: { [zlib.constants.BROTLI_PARAM_QUALITY]: quality, [zlib.constants.BROTLI_PARAM_SIZE_HINT]: raw.length } }) : gzip(raw);

const files = new Map<string, { file: string; etag: string; body: Promise<Buffer> }>();

/** ファイルを縮めたものを返す。作り直されて etag が変わるまで、縮めた結果を使い回す。 */
export const compressedFile = (file: string, etag: string, encoding: Encoding): Promise<Buffer> => {
  const key = `${encoding}:${file}`;
  const hit = files.get(key);
  if (hit?.etag === etag) return hit.body;
  // 作り直すたびに名前が変わるチャンクの、消されたものを覚え続けない。
  for (const [k, v] of files) if (!fs.existsSync(v.file)) files.delete(k);
  const body = fs.promises.readFile(file).then((raw) => compress(raw, encoding, FILE_QUALITY));
  files.set(key, { file, etag, body });
  body.catch(() => files.delete(key));
  return body;
};

/**
 * SSE のように少しずつ送り続ける応答を縮める。送るたびに flush して、ブラウザへすぐ届ける。
 * 圧縮は前に送った内容を覚えているので、brotli では、ほとんど変わらないスナップショットを送り直しても変わったところの分しか流れない。
 * gzip は覚えている範囲（32 KB）がスナップショットより狭く、毎回の全体を縮めるだけになる。
 */
export const compressedStream = (out: Writable, encoding: Encoding | undefined): ((text: string) => void) => {
  if (!encoding) return (text) => void out.write(text);
  const packer = encoding === 'br' ? zlib.createBrotliCompress({ params: { [zlib.constants.BROTLI_PARAM_QUALITY]: LIVE_QUALITY } }) : zlib.createGzip();
  packer.pipe(out);
  out.on('close', () => packer.destroy());
  return (text) => {
    packer.write(text);
    packer.flush();
  };
};
