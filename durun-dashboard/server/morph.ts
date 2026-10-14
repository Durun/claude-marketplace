// 日本語の形態素解析。文章校正で、候補の語（和語の動詞、指示語、内容語、述語）を拾うのに使う。
// 辞書は lindera 6.2.0 用の UniDic を初回に GitHub から取り、DATA_DIR の下に置く。
// lindera 7 で辞書の形式が変わるため、lindera と辞書は 6.2.0 に揃えて固定する。
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import type { Token } from 'lindera';
import { DATA_DIR } from './store.ts';

/** 形態素 1 つ。start と end は、元の文章での JS の文字位置。 */
export type Morpheme = {
  /** 解析した文字列（NFKC 済み）での表層形。コードだけは元の文字列。 */
  surface: string;
  start: number;
  end: number;
  /** 品詞と細分類（UniDic の details[0..3] から '*' を除いたもの）。コードの部分は ['コード']。 */
  pos: string[];
  /** 基本形（UniDic の書字形基本形 details[10]）。未知語やコードは表層形。 */
  base: string;
  /** 語種（UniDic の details[12]：和、漢、外、混、固、記号 など）。未知語やコードは undefined。 */
  origin?: string;
  unknown: boolean;
};

// ---- 辞書の取得と展開 ----

const DICT_VERSION = '6.2.0';
const DICT_URL = `https://github.com/lindera/lindera/releases/download/v${DICT_VERSION}/lindera-unidic-${DICT_VERSION}.zip`;
const DOWNLOAD_TIMEOUT_MS = 10 * 60_000;
export const DICT_DIR = path.join(DATA_DIR, 'lindera', `unidic-${DICT_VERSION}`);
export const hasDictionary = (): boolean => fs.existsSync(path.join(DICT_DIR, 'metadata.json'));

/** 同時に何度呼ばれても f を 1 回だけ動かし、結果を共有する。失敗したら、次の呼び出しでやり直せるよう覚えない。 */
export const once = <T>(f: () => Promise<T>): (() => Promise<T>) => {
  let shared: Promise<T> | undefined;
  return () => (shared ??= f().catch((e) => {
    shared = undefined;
    throw e;
  }));
};

const ZIP_END = 0x06054b50;
const ZIP_CENTRAL = 0x02014b50;
const ZIP_LOCAL = 0x04034b50;

/** 展開先の外へ書き出せるパス（絶対パス、ドライブ名、..）を拒否し、/ で区切ったパスの部品を返す。 */
const zipPathParts = (name: string): string[] => {
  const parts = name.replaceAll('\\', '/').split('/');
  if (name.includes('\0') || /^([/\\]|[A-Za-z]:)/.test(name) || parts.includes('..')) throw new Error(`zip の中のパスが不正: ${name}`);
  return parts.filter((p) => p && p !== '.');
};

/**
 * zip の中央ディレクトリを読み、ファイルの名前（/ 区切りの部品）と中身を返す。
 * 外部コマンドは OS ごとに有無が違うので使わない。stored と deflate だけを扱い、CRC とサイズを確かめる。
 */
export const unzip = (zip: Buffer): { parts: string[]; data: Buffer }[] => {
  const end = zip.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (end < 0 || zip.readUInt32LE(end) !== ZIP_END) throw new Error('zip の終端が見つからない');
  const count = zip.readUInt16LE(end + 10);
  let p = zip.readUInt32LE(end + 16);
  if (count === 0xffff || p === 0xffffffff) throw new Error('zip64 は扱わない');
  const files: { parts: string[]; data: Buffer }[] = [];
  for (let n = 0; n < count; n++) {
    if (p + 46 > zip.length || zip.readUInt32LE(p) !== ZIP_CENTRAL) throw new Error('zip の中央ディレクトリが壊れている');
    const [flags, method, crc, size, rawSize, nameLen, extraLen, commentLen, local] = [
      zip.readUInt16LE(p + 8), zip.readUInt16LE(p + 10), zip.readUInt32LE(p + 16), zip.readUInt32LE(p + 20),
      zip.readUInt32LE(p + 24), zip.readUInt16LE(p + 28), zip.readUInt16LE(p + 30), zip.readUInt16LE(p + 32), zip.readUInt32LE(p + 42),
    ] as const;
    const name = zip.toString('utf8', p + 46, p + 46 + nameLen);
    p += 46 + nameLen + extraLen + commentLen;
    const parts = zipPathParts(name);
    if (name.endsWith('/')) continue;
    if (flags & 1) throw new Error(`暗号化された zip は扱わない: ${name}`);
    if (local + 30 > zip.length || zip.readUInt32LE(local) !== ZIP_LOCAL) throw new Error(`zip のローカルヘッダが壊れている: ${name}`);
    const start = local + 30 + zip.readUInt16LE(local + 26) + zip.readUInt16LE(local + 28);
    if (start + size > zip.length) throw new Error(`zip が途中で切れている: ${name}`);
    const raw = zip.subarray(start, start + size);
    if (method !== 0 && method !== 8) throw new Error(`zip の圧縮方式に未対応（${method}）: ${name}`);
    const data = method === 0 ? raw : zlib.inflateRawSync(raw, { maxOutputLength: Math.max(rawSize, 1) });
    if (data.length !== rawSize || zlib.crc32(data) !== crc) throw new Error(`zip の中身が壊れている（CRC かサイズが合わない）: ${name}`);
    files.push({ parts, data });
  }
  return files;
};

/** zip を dest に展開する。全部が同じ最上位のディレクトリの下にあれば、その 1 段を外す。 */
export const extractZip = (zip: Buffer, dest: string): void => {
  const files = unzip(zip);
  const top = files[0]?.parts[0];
  const strip = top !== undefined && files.every((f) => f.parts.length > 1 && f.parts[0] === top) ? 1 : 0;
  for (const { parts, data } of files) {
    const file = path.join(dest, ...parts.slice(strip));
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, data);
  }
};

/** 辞書を取得して DICT_DIR に展開する。一時ディレクトリに展開してから rename するので、途中で落ちても壊れた辞書は残らない。 */
const ensureDictionary = once(async (): Promise<string> => {
  if (hasDictionary()) return DICT_DIR;
  const res = await fetch(DICT_URL, { signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS) });
  if (!res.ok) throw new Error(`UniDic の取得に失敗（HTTP ${res.status}）: ${DICT_URL}`);
  const zip = Buffer.from(await res.arrayBuffer());
  fs.mkdirSync(path.dirname(DICT_DIR), { recursive: true });
  const tmp = fs.mkdtempSync(path.join(path.dirname(DICT_DIR), '.unidic-'));
  try {
    extractZip(zip, tmp);
    if (!fs.existsSync(path.join(tmp, 'metadata.json'))) throw new Error('UniDic の zip に metadata.json が無い');
    fs.renameSync(tmp, DICT_DIR);
  } catch (e) {
    fs.rmSync(tmp, { recursive: true, force: true });
    // 別のプロセスが先に置き終えていたなら、それを使う。
    if (!hasDictionary()) throw e;
  }
  return DICT_DIR;
});

// 辞書のインスタンスは tokenizer に 1 回しか渡せないので、tokenizer を 1 つ作って使い回す。
// OS ごとの実体を持つネイティブのモジュールは、使うときまで読み込まない。
const getTokenizer = once(async () => {
  const { loadDictionary, Tokenizer } = await import('lindera');
  return new Tokenizer(loadDictionary(await ensureDictionary()), 'normal');
});

// ---- 解析前の整形と、位置の戻し ----

/** コードの代わりに解析へ渡す名詞。文の形を保ったまま、中身を日本語として割らせないため。 */
const CODE_WORD = 'コード';

/** 整形した文字列（UTF-16 の 1 つごと）が、元の文章のどの範囲に当たるか。code は、コードの代わりの文字。 */
export type Prepared = { text: string; from: number[]; to: number[]; code: boolean[] };

type Unit = { c: string; from: number; to: number; code: boolean };

const LINE_MARKER = /^[ \t]*(#{1,6}[ \t]+|[-*+][ \t]+|\d+[.)][ \t]+)?/;
const FENCE = /^[ \t]*```[^`]*$/;
// 半角カナの濁点と半濁点は、直前の文字と組にしてから NFKC をかける。
const KANA_MARK = /[\u3099\u309a\uff9e\uff9f]/;

/** 半角スペースが前後の非 ASCII（日本語、改行、行頭と行末）に隣り合うなら除く。6.2.0 は半角スペースの直後で文脈が切れ、「が」「で」が接続詞になるため。 */
const dropSpaces = (units: Unit[]): Unit[] => {
  const out: Unit[] = [];
  const isBoundary = (u?: Unit): boolean => !u || u.c === '\n' || u.c.charCodeAt(0) > 0x7f;
  units.forEach((u, i) => {
    if (u.c !== ' ' && u.c !== '\t') return void out.push(u);
    let next = i + 1;
    while (units[next]?.c === ' ' || units[next]?.c === '\t') next++;
    if (!isBoundary(out.at(-1)) && !isBoundary(units[next])) out.push(u);
  });
  return out;
};

/** line[i] から始まるバッククォートのコードの終わり（閉じの次の位置）。同じ数のバッククォートで閉じていなければ -1。 */
const inlineCodeEnd = (line: string, i: number): number => {
  const ticks = /^`+/.exec(line.slice(i))?.[0] ?? '';
  const close = line.indexOf(ticks, i + ticks.length);
  return close < 0 || line[close + ticks.length] === '`' ? -1 : close + ticks.length;
};

/**
 * 解析前に文章を整える。バッククォートと ``` のコードは CODE_WORD に置き換え、見出し、箇条書き、番号、** の記号を除く。
 * 日本語に隣り合う半角スペースを除き、NFKC をかける。出力の各文字は、元の文字列の範囲を覚えている。
 */
export const prepare = (text: string): Prepared => {
  const units: Unit[] = [];
  const put = (s: string, from: number, to: number, code = false): void => {
    for (let k = 0; k < s.length; k++) units.push({ c: s[k] as string, from, to, code });
  };
  const lines = text.split('\n');
  let pos = 0;
  let fenceAt = -1;
  lines.forEach((line, n) => {
    const lineEnd = pos + line.length;
    const newline = (): void => (n < lines.length - 1 ? put('\n', lineEnd, lineEnd + 1) : undefined);
    const closes = fenceAt >= 0 && FENCE.test(line);
    if (fenceAt < 0 && FENCE.test(line)) fenceAt = pos;
    if (fenceAt >= 0) {
      // フェンスの行から閉じる行までを 1 つのコードとする。閉じなければ末尾まで。
      if (closes || n === lines.length - 1) {
        put(CODE_WORD, fenceAt, lineEnd, true);
        fenceAt = -1;
        if (closes) newline();
      }
      pos = lineEnd + 1;
      return;
    }
    let i = (LINE_MARKER.exec(line) as RegExpExecArray)[0].length;
    while (i < line.length) {
      const c = line[i] as string;
      const codeEnd = c === '`' ? inlineCodeEnd(line, i) : -1;
      if (codeEnd > 0) {
        put(CODE_WORD, pos + i, pos + codeEnd, true);
        i = codeEnd;
        continue;
      }
      if ((c === '*' && line[i + 1] === '*') || c === '\r') {
        i += c === '*' ? 2 : 1;
        continue;
      }
      const cp = line.codePointAt(i) as number;
      let chunk = String.fromCodePoint(cp);
      if (KANA_MARK.test(line[i + chunk.length] ?? '')) chunk += line[i + chunk.length];
      put(chunk.normalize('NFKC'), pos + i, pos + i + chunk.length);
      i += chunk.length;
    }
    newline();
    pos = lineEnd + 1;
  });
  const kept = dropSpaces(units);
  return { text: kept.map((u) => u.c).join(''), from: kept.map((u) => u.from), to: kept.map((u) => u.to), code: kept.map((u) => u.code) };
};

/** 文字列の UTF-8 のバイト位置から、UTF-16 の文字位置への対応。lindera の位置はバイトで返る。 */
const charAtByte = (text: string): Int32Array => {
  const at = new Int32Array(Buffer.byteLength(text) + 1).fill(-1);
  let b = 0;
  for (let u = 0; u < text.length; ) {
    at[b] = u;
    const cp = text.codePointAt(u) as number;
    b += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    u += cp > 0xffff ? 2 : 1;
  }
  at[b] = text.length;
  return at;
};

const ASCII_WORD = /^[A-Za-z0-9]+$/;
const known = (s: string | undefined): s is string => s !== undefined && s !== '*' && s !== '';

/** lindera のトークンを、元の文章での位置を持つ Morpheme にする。コードの代わりの文字は、元の範囲をまとめて 1 つにする。 */
export const toMorphemes = (original: string, prepared: Prepared, tokens: Pick<Token, 'surface' | 'byteStart' | 'byteEnd' | 'isUnknown' | 'details'>[]): Morpheme[] => {
  const at = charAtByte(prepared.text);
  const out: Morpheme[] = [];
  for (const t of tokens) {
    const s = at[t.byteStart] as number;
    const e = at[t.byteEnd] as number;
    if (t.surface.trim() === '') continue;
    const start = prepared.from[s] as number;
    const end = prepared.to[e - 1] as number;
    if (prepared.code[s]) {
      if (out.at(-1)?.start !== start || out.at(-1)?.pos[0] !== 'コード') {
        const surface = original.slice(start, end);
        out.push({ surface, start, end, pos: ['コード'], base: surface, unknown: false });
      }
      continue;
    }
    const d = t.details;
    const prev = out.at(-1);
    // UniDic は 1 文字の ASCII を既知語（記号-文字）にするので、辞書に無い英単語（PR、API、src）が 1 文字ずつに割れる。隣り合う英数字は 1 つの未知語にまとめる。
    if (prev && prev.end === start && ASCII_WORD.test(prev.surface) && ASCII_WORD.test(t.surface) && /[A-Za-z]/.test(prev.surface + t.surface)) {
      const surface = prev.surface + t.surface;
      out[out.length - 1] = { surface, start: prev.start, end, pos: ['名詞', '普通名詞', '一般'], base: surface, unknown: true };
      continue;
    }
    out.push({
      surface: t.surface, start, end,
      pos: d.slice(0, 4).filter(known),
      base: !t.isUnknown && known(d[10]) ? d[10] : t.surface,
      origin: !t.isUnknown && known(d[12]) ? d[12] : undefined,
      unknown: t.isUnknown,
    });
  }
  return out;
};

/** 文章を解析する。初回は辞書を取得して展開するので時間がかかる。 */
export const analyze = async (text: string): Promise<Morpheme[]> => {
  const tokenizer = await getTokenizer();
  const prepared = prepare(text);
  return toMorphemes(text, prepared, tokenizer.tokenize(prepared.text));
};

// ---- 候補の語を拾う ----

// 機能的に使われる動詞。書字形基本形で照合するので、仮名と漢字の両方の表記を並べる。
// 「上げる」は授受の「あげる」と別の語なので入れない。
// UniDic の「非自立可能」は語ごとの印で、使われ方では決まらない（「出す」「付く」「見る」も非自立可能）ので、条件には使わない。
const FUNCTIONAL_VERBS = new Set([
  'する', 'なる', 'ある', 'いる', 'できる', 'おる', 'くる', 'いく', 'みる', 'おく', 'しまう', 'もらう', 'くれる', 'あげる', 'いう',
  '為る', '成る', '有る', '在る', '居る', '出来る', '来る', '行く', '見る', '置く', '仕舞う', '貰う', '呉れる', '言う',
]);
const FUNCTIONAL_ADJECTIVES = new Set(['ない', 'ほしい']);
// UniDic 6.2.0 は形式名詞を区別しない（「こと」「もの」も 名詞-普通名詞-一般）ので、仮名の表記で除く。
const FORMAL_NOUNS = new Set(['こと', 'もの', 'ため', 'ところ', 'わけ', 'はず', 'ほう', 'つもり', 'とおり', 'ほか', 'うち', 'まま']);

/** 和語の単純な動詞（焼く、受ける、持つ、出す）。機能的な動詞（する、いる、おく など）は除く。 */
export const isPlainWagoVerb = (m: Morpheme): boolean => m.pos[0] === '動詞' && m.origin === '和' && !FUNCTIONAL_VERBS.has(m.base);

const ADNOMINALS = ['この', 'その', 'あの'];
// こう + し + た のように、複数のトークンに割れる指示語。
const ADNOMINAL_PHRASES = ['こうした', 'そうした'];
const PRONOUNS = ['これ', 'それ', 'こちら', 'そちら'];
const NOUN_DEMONSTRATIVES = ['上記', '前述', '後述', '当該', '同'];
const NOUN_LIKE = new Set(['名詞', '接頭辞', '接尾辞', 'コード']);

/** ms[i] から始まる指示語の長さ（トークンの数）。後ろの名詞まで 1 語句にするかも返す。指示語でなければ undefined。 */
const demonstrativeHead = (ms: Morpheme[], i: number): { len: number; extend: boolean } | undefined => {
  const m = ms[i] as Morpheme;
  const len = [1, 2, 3].find((n) => ADNOMINAL_PHRASES.includes(ms.slice(i, i + n).map((x) => x.surface).join('')));
  if (len) return { len, extend: true };
  const [pos, surface] = [m.pos[0], m.surface];
  if ((pos === '連体詞' && ADNOMINALS.includes(surface)) || (pos === '名詞' && NOUN_DEMONSTRATIVES.includes(surface))) return { len: 1, extend: true };
  if (pos === '代名詞' && PRONOUNS.includes(surface)) return { len: 1, extend: false };
  return undefined;
};

/**
 * 指示語の語句。連体詞の「この」「その」「あの」と「こうした」「そうした」、名詞の「上記」「前述」「後述」「当該」「同」は、続く名詞までを 1 語句にする（「この定数」「同サービス」）。
 * 代名詞の「これ」「それ」「こちら」「そちら」は単独で拾う。
 * フィラーの「あの」は連体詞でないので拾わない。text は、解析した文字列（NFKC 済み）で連結した表層形。
 */
export const demonstratives = (ms: Morpheme[]): { start: number; end: number; text: string }[] => {
  const out: { start: number; end: number; text: string }[] = [];
  for (let i = 0; i < ms.length; ) {
    const head = demonstrativeHead(ms, i);
    if (!head) {
      i++;
      continue;
    }
    let end = i + head.len;
    // 形式名詞（そのため、そのこと）で語句を閉じる。続く名詞は、その修飾を受けていない。
    while (head.extend && NOUN_LIKE.has(ms[end]?.pos[0] ?? '')) {
      const closes = FORMAL_NOUNS.has((ms[end] as Morpheme).base);
      end++;
      if (closes) break;
    }
    const words = ms.slice(i, end);
    out.push({ start: (words[0] as Morpheme).start, end: (words.at(-1) as Morpheme).end, text: words.map((w) => w.surface).join('') });
    i = end;
  }
  return out;
};

/** 内容語。名詞（普通名詞と固有名詞。数詞、代名詞、形式名詞、助数詞は除く）、機能的でない動詞と形容詞、コード。 */
export const isContentWord = (m: Morpheme): boolean => {
  switch (m.pos[0]) {
    case 'コード':
      return true;
    case '名詞':
      // 未知語の「-」や「_」は、名詞と推定されても内容語にしない。
      return (m.pos[1] === '普通名詞' || m.pos[1] === '固有名詞') && m.pos[2] !== '形式名詞' && m.pos[2] !== '助数詞可能' && !FORMAL_NOUNS.has(m.base) && /[\p{L}\p{N}]/u.test(m.surface);
    case '動詞':
      return !FUNCTIONAL_VERBS.has(m.base);
    case '形容詞':
      return !FUNCTIONAL_ADJECTIVES.has(m.base);
    default:
      return false;
  }
};

const isSahenNoun = (m?: Morpheme): boolean => m?.pos[0] === '名詞' && m.pos[2] === 'サ変可能';
const isTeDe = (m?: Morpheme): boolean => m?.pos[1] === '接続助詞' && (m.surface === 'て' || m.surface === 'で');

/**
 * ms[i] を述語として数えるか。1 文の述語の数を、true の数で数えるための判定。
 * - 動詞と形容詞を 1 つと数える。助動詞（ない、れる、た）は数えない。
 * - 「て」「で」の後ろの機能的な動詞（ている の いる、てしまう、ておく）と、形容詞の「ほしい」「ない」は補助なので数えない。
 * - サ変名詞 + する（受信する）は 1 つと数え、数える位置は名詞にする。直前がサ変可能の名詞の「する」は数えない。
 * - 形状詞 + だ、です、で（静かだ、静かで）は、だ、です、で を 1 つと数える。「な」「に」は連体と連用の修飾なので数えない。
 * - 「静かである」の「ある」は、で と合わせて 1 つなので数えない。
 * - 名詞 + だ、である（名詞述語）は数えない。「静かではない」のような形状詞 + では + ない は 2 つと数える。
 */
export const isPredicate = (ms: Morpheme[], i: number): boolean => {
  const m = ms[i];
  if (!m) return false;
  const prev = ms[i - 1];
  const next = ms[i + 1];
  switch (m.pos[0]) {
    case '動詞':
      if (isTeDe(prev) && FUNCTIONAL_VERBS.has(m.base)) return false;
      if (m.base === 'ある' && prev?.pos[0] === '助動詞' && prev.surface === 'で') return false;
      return !(m.base === 'する' && isSahenNoun(prev));
    case '名詞':
      return isSahenNoun(m) && next?.pos[0] === '動詞' && next.base === 'する';
    case '形容詞':
      return !(isTeDe(prev) && FUNCTIONAL_ADJECTIVES.has(m.base));
    case '助動詞':
      return (m.base === 'だ' || m.base === 'です') && prev?.pos[0] === '形状詞' && m.surface !== 'な' && m.surface !== 'に';
    default:
      return false;
  }
};
