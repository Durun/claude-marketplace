import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fetchCover, matchTrackCount, skeletonMeta, validateMeta } from '../server/cdripResearch.ts';

const good = { circle: ' MEGAREX ', album: 'A', artist: 'AA', dest: 'CLUB', date: '', tracks: [{ title: 'T1', artist: '', composer: 'C', duration: 243 }, { title: 'T2' }] };

test('曲情報の検証は、前後の空白を除き、省ける項目の空文字を無いものにそろえる', () => {
  assert.deepEqual(validateMeta(good), { circle: 'MEGAREX', album: 'A', artist: 'AA', dest: 'CLUB', tracks: [{ title: 'T1', composer: 'C', duration: 243 }, { title: 'T2' }] });
  const full = validateMeta({ ...good, disc: 1, totaldiscs: 2, confidence: 'high', sources: ['https://x', 3], genre: 'Hardcore', catalog: 'X-1', cover: 'cover.jpg', coverUrl: 'https://x/c.jpg', notes: 'n' });
  assert.deepEqual([full.disc, full.totaldiscs, full.confidence, full.sources, full.genre, full.catalog], [1, 2, 'high', ['https://x'], 'Hardcore', 'X-1']);
});

test('曲情報の検証は、必須の項目の欠け、dest の値、曲の空、枚数の食い違いを理由つきで断る', () => {
  assert.throws(() => validateMeta({ ...good, circle: ' ' }), /circle が空/);
  assert.throws(() => validateMeta({ ...good, dest: 'Other' }), /CLUB か Music/);
  assert.throws(() => validateMeta({ ...good, tracks: [] }), /1 曲以上/);
  assert.throws(() => validateMeta({ ...good, tracks: [{ title: '' }] }), /1 曲目の title が空/);
  assert.throws(() => validateMeta({ ...good, disc: 3, totaldiscs: 2 }), /超えて/);
  assert.throws(() => validateMeta({ ...good, disc: 0 }), /1 以上の整数/);
  assert.throws(() => validateMeta('x'), /オブジェクト/);
});

test('曲数が CD と合わないときだけ、理由を返す', () => {
  assert.equal(matchTrackCount({ tracks: [{ title: 'a' }] }, 1), undefined);
  assert.equal(matchTrackCount({ tracks: [{ title: 'a' }] }, undefined), undefined);
  assert.match(matchTrackCount({ tracks: [{ title: 'a' }] }, 8) ?? '', /1 曲.*8 曲/);
});

test('下書きは、CD-Text があればアルバムと曲名を取り、無ければ曲数ぶんの空の曲を作る', () => {
  assert.deepEqual(skeletonMeta({ tracks: 2 }).tracks, [{ title: '' }, { title: '' }]);
  const m = skeletonMeta({ tracks: 2, cdText: { album: 'Al', artist: 'Ar', titles: ['x'] } });
  assert.deepEqual([m.album, m.artist, m.tracks.map((t) => t.title)], ['Al', 'Ar', ['x', '']]);
});

const reply = (body: string, type: string, status = 200): typeof fetch => (async () => new Response(body, { status, headers: { 'content-type': type } })) as unknown as typeof fetch;

test('ジャケットは http か https の画像だけを取り込み、拡張子を画像の種類から決める', async () => {
  assert.deepEqual(await fetchCover('https://x/c', reply('abc', 'image/png; charset=binary')), { bytes: Buffer.from('abc'), ext: 'png' });
  assert.equal((await fetchCover('http://x/c', reply('abc', 'image/jpeg'))).ext, 'jpg');
  await assert.rejects(fetchCover('file:///etc/passwd', reply('', 'image/png')), /http か https/);
  await assert.rejects(fetchCover('https://x/c', reply('<html>', 'text/html')), /画像ではありません/);
  await assert.rejects(fetchCover('https://x/c', reply('', 'image/png', 404)), /HTTP 404/);
});
