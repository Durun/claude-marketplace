import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import zlib from 'node:zlib';
import { analyze, demonstratives, extractZip, hasDictionary, isContentWord, isPlainWagoVerb, isPredicate, type Morpheme, once, prepare, toMorphemes, unzip } from '../server/morph.ts';

type ZipEntry = { name: string; data: string; method?: 0 | 8; crc?: number };

/** stored（0）か deflate（8）で、zip をバイト列から組み立てる。crc を渡すと、わざと合わない値を書ける。 */
const buildZip = (entries: ZipEntry[]): Buffer => {
  const parts: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const e of entries) {
    const name = Buffer.from(e.name);
    const data = Buffer.from(e.data);
    const method = e.method ?? 0;
    const body = method === 8 ? zlib.deflateRawSync(data) : data;
    const crc = e.crc ?? zlib.crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(method, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(name.length, 26);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(method, 10);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(name.length, 28);
    entry.writeUInt32LE(offset, 42);
    parts.push(local, name, body);
    central.push(entry, name);
    offset += 30 + name.length + body.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, directory, end]);
};

test('unzip は、stored と deflate のファイルを取り出し、ディレクトリの項目は飛ばす', () => {
  const zip = buildZip([
    { name: 'dict/', data: '' },
    { name: 'dict/a.txt', data: 'stored の中身' },
    { name: 'dict/sub/b.txt', data: 'deflate '.repeat(50), method: 8 },
  ]);
  const files = unzip(zip).map((f) => [f.parts.join('/'), f.data.toString()]);
  assert.deepEqual(files, [['dict/a.txt', 'stored の中身'], ['dict/sub/b.txt', 'deflate '.repeat(50)]]);
});

test('unzip は、CRC が合わない zip を拒否する', () => {
  assert.throws(() => unzip(buildZip([{ name: 'a.txt', data: 'abc', crc: 1 }])), /CRC/);
  assert.throws(() => unzip(buildZip([{ name: 'a.txt', data: 'abc'.repeat(9), method: 8, crc: 1 }])), /CRC/);
});

test('unzip は、.. や絶対パスやドライブ名を含むパスを拒否する', () => {
  for (const name of ['../evil.txt', 'a/../../evil.txt', 'a\\..\\evil.txt', '/etc/evil.txt', '\\evil.txt', 'C:/evil.txt', '../dir/']) {
    assert.throws(() => unzip(buildZip([{ name, data: 'x' }])), /パスが不正/, name);
  }
  assert.equal(unzip(buildZip([{ name: 'a..b/c..txt', data: 'x' }])).length, 1);
});

test('unzip は、zip でないバイト列を拒否する', () => {
  assert.throws(() => unzip(Buffer.from('zip ではない')), /終端/);
});

test('extractZip は、全部が同じ最上位のディレクトリにあればその 1 段を外して書き出す', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-morph-'));
  extractZip(buildZip([{ name: 'top/', data: '' }, { name: 'top/a.txt', data: 'A' }, { name: 'top/d/b.txt', data: 'B', method: 8 }]), path.join(tmp, 'stripped'));
  assert.equal(fs.readFileSync(path.join(tmp, 'stripped', 'a.txt'), 'utf8'), 'A');
  assert.equal(fs.readFileSync(path.join(tmp, 'stripped', 'd', 'b.txt'), 'utf8'), 'B');
  extractZip(buildZip([{ name: 'x/a.txt', data: 'A' }, { name: 'y.txt', data: 'Y' }]), path.join(tmp, 'kept'));
  assert.equal(fs.readFileSync(path.join(tmp, 'kept', 'x', 'a.txt'), 'utf8'), 'A');
  assert.equal(fs.readFileSync(path.join(tmp, 'kept', 'y.txt'), 'utf8'), 'Y');
});

test('extractZip は、拒否した zip の中身を 1 つも書き出さない', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-morph-'));
  assert.throws(() => extractZip(buildZip([{ name: 'a.txt', data: 'A' }, { name: '../b.txt', data: 'B' }]), path.join(tmp, 'out')));
  assert.equal(fs.existsSync(path.join(tmp, 'out')), false);
  assert.equal(fs.existsSync(path.join(tmp, 'b.txt')), false);
});

test('once は、同時に呼ばれても 1 回だけ動かして結果を共有し、失敗したら次の呼び出しでやり直す', async () => {
  let calls = 0;
  const f = once(async () => {
    calls++;
    await Promise.resolve();
    if (calls === 1) throw new Error('失敗');
    return calls;
  });
  const first = await Promise.allSettled([f(), f(), f()]);
  assert.equal(calls, 1);
  assert.deepEqual(first.map((r) => r.status), ['rejected', 'rejected', 'rejected']);
  assert.deepEqual(await Promise.all([f(), f()]), [2, 2]);
  assert.equal(await f(), 2);
  assert.equal(calls, 2);
});

/** 整形後の位置 i の文字が当たる、元の文字列の範囲。 */
const originOf = (original: string, p: ReturnType<typeof prepare>, i: number): string => original.slice(p.from[i], p.to[i]);

test('prepare は、コード、見出し、箇条書き、強調、日本語に隣り合う半角スペースを除き、NFKC をかける', () => {
  const original = '# 見出し\n- 公開 `src` を **受信** する。\n\n```sh\nls\n```\nＡＢＣ ｶﾞ test ok';
  const p = prepare(original);
  assert.equal(p.text, '見出し\n公開コードを受信する。\n\nコード\nABCガtest ok');
  assert.equal(originOf(original, p, p.text.indexOf('コードを')), '`src`');
  assert.equal(originOf(original, p, p.text.indexOf('コード\n')), '```sh\nls\n```');
  assert.equal(originOf(original, p, p.text.indexOf('A')), 'Ａ');
  assert.equal(originOf(original, p, p.text.indexOf('ガ')), 'ｶﾞ');
  assert.equal(originOf(original, p, p.text.indexOf('受信')), '受');
  assert.deepEqual(p.code.map((c, i) => (c ? p.text[i] : '')).join(''), 'コードコード');
});

test('prepare は、閉じていないフェンスを末尾までコードとし、数の合わないバッククォートはコードにしない', () => {
  const unclosed = '前。\n```\nconst a = 1;\nconst b = 2;';
  const p = prepare(unclosed);
  assert.equal(p.text, '前。\nコード');
  assert.equal(originOf(unclosed, p, p.text.indexOf('コード')), '```\nconst a = 1;\nconst b = 2;');
  const inline = prepare('`a` と ``b`c`` と `d');
  assert.equal(inline.text, 'コードとコードと`d');
  assert.equal(prepare('1行目\r\n2行目').text, '1行目\n2行目');
});

test('prepare は、ASCII だけに挟まれた半角スペースを残す', () => {
  assert.equal(prepare('Cloud Run  で  deploy').text, 'Cloud Runでdeploy');
  assert.equal(prepare('a  b').text, 'a  b');
});

type Raw = Parameters<typeof toMorphemes>[2][number];

/** 整形後の文字列を surfaces で区切った lindera 風のトークン。位置は UTF-8 のバイトで持つ。 */
const rawTokens = (text: string, surfaces: string[], details: (surface: string) => string[] = () => ['名詞', '普通名詞', '一般', '*']): Raw[] => {
  let byte = 0;
  let rest = text;
  return surfaces.map((surface) => {
    assert.ok(rest.startsWith(surface), `${surface} が ${rest} の先頭に無い`);
    rest = rest.slice(surface.length);
    const token = { surface, byteStart: byte, byteEnd: byte + Buffer.byteLength(surface), isUnknown: false, details: details(surface) };
    byte = token.byteEnd;
    return token;
  });
};

test('toMorphemes は、バイト位置を元の文字列の文字位置に戻し、コードの範囲を 1 つの形態素にする', () => {
  const original = '𠮷野家の公開 `src` を受信する。';
  const p = prepare(original);
  const ms = toMorphemes(original, p, rawTokens(p.text, ['𠮷', '野家', 'の', '公開', 'コー', 'ド', 'を', '受信', 'する', '。']));
  assert.deepEqual(ms.map((m) => original.slice(m.start, m.end)), ['𠮷', '野家', 'の', '公開', '`src`', 'を', '受信', 'する', '。']);
  const code = ms[4] as Morpheme;
  assert.deepEqual([code.surface, code.pos, code.base, code.origin, code.unknown], ['`src`', ['コード'], '`src`', undefined, false]);
});

test('toMorphemes は、NFKC で変わった文字も元の範囲に戻し、1 文字ずつに割れた英数字を 1 つの未知語にまとめる', () => {
  const original = 'この ＰＲ が １００ 件';
  const p = prepare(original);
  assert.equal(p.text, 'このPRが100件');
  const ms = toMorphemes(original, p, rawTokens(p.text, ['この', 'P', 'R', 'が', '1', '0', '0', '件']));
  assert.deepEqual(ms.map((m) => [m.surface, original.slice(m.start, m.end)]), [['この', 'この'], ['PR', 'ＰＲ'], ['が', 'が'], ['1', '１'], ['0', '０'], ['0', '０'], ['件', '件']]);
  assert.deepEqual([ms[1]?.pos, ms[1]?.unknown], [['名詞', '普通名詞', '一般'], true]);
});

test('toMorphemes は、details から品詞、基本形、語種を取り、未知語は表層形を基本形にする', () => {
  const details = ['動詞', '一般', '*', '*', '五段-カ行', '連用形-イ音便', 'ヤク', '焼く', '焼い', 'ヤイ', '焼く', '焼く', '和'];
  const [known, unknown] = toMorphemes('焼いfoo', prepare('焼いfoo'), [
    { surface: '焼い', byteStart: 0, byteEnd: 6, isUnknown: false, details },
    { surface: 'foo', byteStart: 6, byteEnd: 9, isUnknown: true, details: ['名詞', '普通名詞', '一般', '*'] },
  ]) as [Morpheme, Morpheme];
  assert.deepEqual([known.pos, known.base, known.origin, known.unknown], [['動詞', '一般'], '焼く', '和', false]);
  assert.deepEqual([unknown.pos, unknown.base, unknown.origin, unknown.unknown], [['名詞', '普通名詞', '一般'], 'foo', undefined, true]);
});

/** 品詞を - でつないだ文字列から、合成の形態素を作る。位置は表層形の長さで連ねる。 */
const seq = (...words: [surface: string, pos: string, extra?: { base?: string; origin?: string }][]): Morpheme[] => {
  let at = 0;
  return words.map(([surface, pos, extra]) => {
    const m: Morpheme = { surface, start: at, end: at + surface.length, pos: pos.split('-'), base: extra?.base ?? surface, origin: extra?.origin, unknown: false };
    at = m.end;
    return m;
  });
};

test('isPlainWagoVerb は、和語の動詞だけを拾い、機能的な動詞と漢語サ変を除く', () => {
  const [yaku, deru, suru, miru, kanji, taisuru, uke, jushin] = seq(
    ['焼い', '動詞-一般', { base: '焼く', origin: '和' }],
    ['出す', '動詞-非自立可能', { origin: '和' }],
    ['し', '動詞-非自立可能', { base: 'する', origin: '和' }],
    ['見', '動詞-非自立可能', { base: '見る', origin: '和' }],
    ['いる', '動詞-非自立可能', { base: 'いる', origin: '和' }],
    ['対する', '動詞-一般', { origin: '混' }],
    ['受ける', '動詞-一般', { origin: '和' }],
    ['受信', '名詞-普通名詞-サ変可能', { origin: '漢' }],
  ) as Morpheme[] as [Morpheme, Morpheme, Morpheme, Morpheme, Morpheme, Morpheme, Morpheme, Morpheme];
  assert.deepEqual([yaku, deru, uke].map(isPlainWagoVerb), [true, true, true]);
  assert.deepEqual([suru, miru, kanji, taisuru, jushin].map(isPlainWagoVerb), [false, false, false, false, false]);
});

test('isContentWord は、名詞、機能的でない動詞と形容詞、コードを拾う', () => {
  const content = seq(['定数', '名詞-普通名詞-一般'], ['東京', '名詞-固有名詞-地名-一般'], ['焼い', '動詞-一般', { base: '焼く' }], ['良い', '形容詞-非自立可能'], ['`x`', 'コード']);
  const notContent = seq(
    ['3', '名詞-数詞'], ['これ', '代名詞'], ['こと', '名詞-普通名詞-一般'], ['個', '名詞-普通名詞-助数詞可能'], ['し', '動詞-非自立可能', { base: 'する' }],
    ['ない', '形容詞-非自立可能'], ['が', '助詞-格助詞'], ['た', '助動詞'], ['-', '名詞-普通名詞-サ変可能'], ['。', '補助記号-句点'], ['静か', '形状詞-一般'],
  );
  assert.deepEqual(content.map(isContentWord), [true, true, true, true, true]);
  assert.equal(notContent.filter(isContentWord).length, 0, notContent.filter(isContentWord).map((m) => m.surface).join());
});

test('demonstratives は、連体詞と続く名詞を 1 語句にし、代名詞は単独で拾う', () => {
  const ms = seq(
    ['この', '連体詞'], ['定数', '名詞-普通名詞-一般'], ['は', '助詞-係助詞'], ['これ', '代名詞'], ['を', '助詞-格助詞'],
    ['その', '連体詞'], ['`x`', 'コード'], ['関数', '名詞-普通名詞-一般'], ['の', '助詞-格助詞'], ['あの', '感動詞-フィラー'], ['こちら', '代名詞'],
  );
  assert.deepEqual(demonstratives(ms), [
    { start: 0, end: 4, text: 'この定数' },
    { start: 5, end: 7, text: 'これ' },
    { start: 8, end: 15, text: 'その`x`関数' },
    { start: 18, end: 21, text: 'こちら' },
  ]);
});

test('demonstratives は、こう + し + た に割れる「こうした」と、名詞の「上記」「同」を拾う', () => {
  const ms = seq(
    ['こう', '副詞'], ['し', '動詞-非自立可能', { base: 'する' }], ['た', '助動詞'], ['問題', '名詞-普通名詞-一般'], ['を', '助詞-格助詞'],
    ['上記', '名詞-普通名詞-一般'], ['の', '助詞-格助詞'], ['同', '名詞-普通名詞-一般'], ['サービス', '名詞-普通名詞-サ変可能'], ['に', '助詞-格助詞'],
    ['こう', '副詞'], ['する', '動詞-非自立可能'],
  );
  assert.deepEqual(demonstratives(ms).map((d) => d.text), ['こうした問題', '上記', '同サービス']);
});

test('demonstratives は、形式名詞で語句を閉じ、続く名詞を取り込まない', () => {
  const ms = seq(['その', '連体詞'], ['ため', '名詞-普通名詞-副詞可能'], ['計測', '名詞-普通名詞-サ変可能'], ['が', '助詞-格助詞'], ['この', '連体詞'], ['こと', '名詞-普通名詞-一般'], ['計測', '名詞-普通名詞-サ変可能']);
  assert.deepEqual(demonstratives(ms).map((d) => d.text), ['そのため', 'このこと']);
});

test('isPredicate は、動詞、形容詞、形状詞 + だ を数え、補助の動詞と助動詞を数えない', () => {
  const count = (ms: Morpheme[]): number => ms.filter((_, i) => isPredicate(ms, i)).length;
  // 受信している：受信する は 1 つで、いる は補助。
  assert.equal(count(seq(['受信', '名詞-普通名詞-サ変可能'], ['し', '動詞-非自立可能', { base: 'する' }], ['て', '助詞-接続助詞'], ['いる', '動詞-非自立可能'])), 1);
  // 焼いた
  assert.equal(count(seq(['焼い', '動詞-一般', { base: '焼く' }], ['た', '助動詞'])), 1);
  // 見てほしい
  assert.equal(count(seq(['見', '動詞-非自立可能', { base: '見る' }], ['て', '助詞-接続助詞'], ['ほしい', '形容詞-非自立可能'])), 1);
  // 取り出して出す：出す は補助ではなく、数える。
  assert.equal(count(seq(['取り出し', '動詞-一般', { base: '取り出す' }], ['て', '助詞-接続助詞'], ['出す', '動詞-非自立可能'])), 2);
  // 高いです
  assert.equal(count(seq(['高い', '形容詞-一般'], ['です', '助動詞'])), 1);
  // 問題がある
  assert.equal(count(seq(['問題', '名詞-普通名詞-一般'], ['が', '助詞-格助詞'], ['ある', '動詞-非自立可能'])), 1);
});

test('isPredicate は、形状詞 + だ、で を 1 つと数え、「な」「に」と名詞 + だ、「である」の「ある」を数えない', () => {
  const at = (ms: Morpheme[]): number[] => ms.flatMap((_, i) => (isPredicate(ms, i) ? [i] : []));
  assert.deepEqual(at(seq(['静か', '形状詞-一般'], ['だ', '助動詞', { base: 'だ' }])), [1]);
  assert.deepEqual(at(seq(['静か', '形状詞-一般'], ['で', '助動詞', { base: 'だ' }], ['ある', '動詞-非自立可能'])), [1]);
  assert.deepEqual(at(seq(['静か', '形状詞-一般'], ['な', '助動詞', { base: 'だ' }], ['部屋', '名詞-普通名詞-一般'])), []);
  assert.deepEqual(at(seq(['静か', '形状詞-一般'], ['に', '助動詞', { base: 'だ' }], ['動く', '動詞-一般'])), [2]);
  assert.deepEqual(at(seq(['設定', '名詞-普通名詞-サ変可能'], ['だ', '助動詞', { base: 'だ' }])), []);
  assert.equal(isPredicate([], 0), false);
});

// 実際の辞書を使う確認。辞書は analyze が初回に取得するが、テストからは取得しないので、DATA_DIR に無ければ飛ばす。
const withDictionary = { skip: hasDictionary() ? false : '辞書が DATA_DIR に無い（analyze を一度動かすと取得される）' };

test('analyze は、整形して解析し、位置を元の文章に戻してコードを 1 つにする', withDictionary, async () => {
  const text = '## 概要\n公開 HTML が焼いた `src` を受信する。\n```ts\nconst a = 1;\n```\nこの PR を確認する。';
  const ms = await analyze(text);
  const code = ms.filter((m) => m.pos[0] === 'コード');
  assert.deepEqual(code.map((m) => text.slice(m.start, m.end)), ['`src`', '```ts\nconst a = 1;\n```']);
  assert.deepEqual(code.map((m) => m.surface), ['`src`', '```ts\nconst a = 1;\n```']);
  for (const m of ms.filter((m) => m.pos[0] !== 'コード')) assert.equal(text.slice(m.start, m.end).replaceAll(' ', ''), m.surface);
  assert.deepEqual(ms.filter((m) => m.pos[0] === '接続詞'), []);
  assert.equal(ms.find((m) => m.surface === '焼い')?.base, '焼く');
  assert.equal(ms.find((m) => m.surface === 'PR')?.unknown, true);
});

test('analyze の結果から、和語の動詞、指示語、述語を拾える', withDictionary, async () => {
  const ms = await analyze('この定数を持つ。受信する。公開 HTML が前へ出す。その時点で静かである。参照して確認している。');
  assert.deepEqual(ms.filter(isPlainWagoVerb).map((m) => m.base), ['持つ', '出す']);
  assert.deepEqual(demonstratives(ms).map((d) => d.text), ['この定数', 'その時点']);
  assert.equal(ms.filter((_, i) => isPredicate(ms, i)).length, 6);
  assert.deepEqual(ms.filter(isContentWord).map((m) => m.surface).slice(0, 3), ['定数', '持つ', '受信']);
});
