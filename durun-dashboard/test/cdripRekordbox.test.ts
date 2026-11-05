import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileLocation, quoteAttr, rekordboxEntries, rekordboxXml } from '../server/cdripRekordbox.ts';
import type { CdRipItem } from '../shared/types.ts';

const fixture = (name: string): string => fs.readFileSync(path.join(import.meta.dirname, 'fixtures', name), 'utf8');

// rbxml.py の「M3 で買ったもの」の判定: Bandcamp 以外と、M3 の新譜として買った Bandcamp のアルバム。
const M3_BANDCAMP = ['Void of the Suppliant(DLC)'];

test('rbxml.py が同じ added.tsv から書いた XML と、1 バイトも違わない XML を作る（記号を含む曲名、" を含む属性、日本語のパス）', () => {
  const rows = fixture('rekordbox-added.tsv').split('\n').slice(1).filter(Boolean).map((l) => l.split('\t')).filter((r) => r[1] === 'mp3');
  const entries = rows.map((r) => {
    const [, , , , album = '', , artist = '', title = '', file = ''] = r;
    const bandcamp = file.includes('\\CLUB\\bandcamp\\');
    return { name: title, artist, album, path: file, playlist: !bandcamp || M3_BANDCAMP.includes(album) ? 'M3で買ったもの' : 'それ以外の追加' };
  });
  assert.equal(rekordboxXml(entries), fixture('rekordbox-golden.xml'));
});

test('属性の囲み方は Python の quoteattr と同じ', () => {
  assert.deepEqual(['a&b<c>', 'say "hi"', "it's", `it's "x"`].map(quoteAttr), ['"a&amp;b&lt;c&gt;"', `'say "hi"'`, `"it's"`, `"it's &quot;x&quot;"`]);
});

test('Location は Windows のパスを / にして % でエスケープし、macOS や Linux の絶対パスは localhost の直後に続ける', () => {
  assert.equal(fileLocation('D:\\Music\\a b\\曲.mp3'), 'file://localhost/D:/Music/a%20b/%E6%9B%B2.mp3');
  assert.equal(fileLocation('/Users/me/Music/a&b.mp3'), 'file://localhost/Users/me/Music/a%26b.mp3');
});

const item = (patch: Partial<CdRipItem>): CdRipItem => ({ id: 'i', hint: 'h', status: 'done', createdAt: 1, updatedAt: 1, ...patch });
const meta = { circle: 'C', album: 'Al', artist: 'AA', dest: 'CLUB' as const, tracks: [{ title: 'T1' }, { title: 'T2', artist: 'Other' }] };

test('rekordbox に渡す曲は、書き出し済みの行の mp3 を曲の順に、グループをプレイリストにして集め、グループの無い行は「追加」に入れる', () => {
  const items = [
    item({ id: 'a', group: 'M3', meta, exported: { at: 1, files: ['/cd/Al.flac', '/m/01.mp3', '/m/02.mp3'] } }),
    item({ id: 'b', meta: { ...meta, album: 'Bl' }, exported: { at: 1, files: ['/m2/01.mp3', '/m2/02.mp3'], rekordboxAt: 5 } }),
    item({ id: 'c', status: 'review', meta }),
    item({ id: 'd', meta, exported: { at: 1, files: ['/cd/Only.flac'] } }),
  ];
  const fresh = rekordboxEntries(items, 'new');
  assert.deepEqual(fresh.entries.map((e) => [e.name, e.artist, e.album, e.playlist]), [['T1', 'AA', 'Al', 'M3'], ['T2', 'Other', 'Al', 'M3']]);
  assert.deepEqual(fresh.itemIds, ['a']);
  const all = rekordboxEntries(items, 'all');
  assert.equal(all.entries.length, 4);
  assert.deepEqual(all.entries.map((e) => e.playlist), ['M3', 'M3', '追加', '追加']);
  // FLAC だけの行は、載せる曲が無いので、渡したことにしない。
  assert.ok(!all.itemIds.includes('d'));
});
