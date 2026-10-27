// エクスプローラーに出すディレクトリの一覧と、送信欄の @ の補完に出すファイルを読み、メモ帳で編集したファイルを上書きする。
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { rankBy } from '../shared/match.ts';
import type { ExplorerDir, ExplorerEntry, TextSaveResult } from '../shared/types.ts';
import { type Platform, platform } from './platform.ts';

const run = promisify(execFile);

// node_modules のような大きなディレクトリで、画面を待たせすぎない件数。
const MAX_ENTRIES = 2000;
// 大きなリポジトリでも、画面へ送る候補はこの数までにする。
const MAX_FILE_CANDIDATES = 50;
// git に聞いた一覧は、この時間を過ぎると、覚えた一覧で答えてから裏で聞き直す。新しく作ったファイルは、聞き直し終えると候補に出る。
const FILE_LIST_MS = 10_000;
const MAX_BUFFER = 64 * 1024 * 1024;

/** リンクの先を見て、ディレクトリを指すリンクも開けるようにする。切れたリンクと、読む権限の無いものは並べない。 */
const entryOf = (dir: string, name: string, p: Platform): ExplorerEntry | undefined => {
  const full = path.join(dir, name);
  try {
    const stat = fs.statSync(full);
    const isDir = stat.isDirectory();
    return { name, path: p.displayPath(full), dir: isDir, size: isDir ? undefined : stat.size, modifiedAt: stat.mtimeMs };
  } catch {
    return undefined;
  }
};

/**
 * dir の中身を、フォルダーを先に、名前の順で並べて返す。dir を省くとホームディレクトリを読む。~ はホームディレクトリにする。
 * 読めないディレクトリでは例外を投げる。
 */
export const listDir = (dir: string | undefined, p: Platform = platform): ExplorerDir => {
  const home = os.homedir();
  const full = path.resolve(dir ? dir.replace(/^~(?=[\\/]|$)/, home) : home);
  const names = fs.readdirSync(full);
  const entries = names
    .slice(0, MAX_ENTRIES)
    .flatMap((name) => entryOf(full, name, p) ?? [])
    .sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name, undefined, { numeric: true }));
  const parent = path.dirname(full);
  return {
    path: p.displayPath(full),
    parent: parent === full ? undefined : p.displayPath(parent),
    home: p.displayPath(home),
    roots: p.fileRoots(),
    entries,
    truncated: names.length > MAX_ENTRIES,
  };
};

// cwd ごとに、git に聞いた一覧と聞いた時刻を覚える。git のリポジトリの外では files が無い。聞いている最中に git を重ねて動かさないよう、聞き始めた時点でも時刻を改める。
// 一覧は 1 万件のリポジトリで 1 MB 近くになり、作業ツリーごとに増えるので、cache.json には残さずメモリにだけ置く。
const fileLists = new Map<string, { at: number; files?: string[] }>();

/**
 * cwd の下にあり git が無視していないファイルの、cwd からのパス。浅いファイルを先に出せるよう、短い順に並べる。
 * git を待たずに覚えた一覧を返し、無いか古ければ裏で聞き直して、聞き終えたら onLoad を呼ぶ。まだ一覧が無いときと、git のリポジトリの外では undefined。
 */
const gitFiles = (cwd: string, onLoad: () => void): string[] | undefined => {
  const hit = fileLists.get(cwd);
  if (hit && Date.now() - hit.at < FILE_LIST_MS) return hit.files;
  fileLists.set(cwd, { at: Date.now(), files: hit?.files });
  void run('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], { cwd, maxBuffer: MAX_BUFFER })
    .then(
      ({ stdout }) => stdout.split('\0').filter(Boolean).sort((a, b) => a.length - b.length),
      () => undefined,
    )
    .then((files) => {
      fileLists.set(cwd, { at: Date.now(), files });
      onLoad();
    });
  return hit?.files;
};

/** word の最後の / までが指すディレクトリの中身。ディレクトリには / を付ける。読めなければ空。 */
const entriesIn = (cwd: string, word: string): string[] => {
  const dir = word.slice(0, word.lastIndexOf('/') + 1);
  try {
    return listDir(path.resolve(cwd, dir.replace(/^~(?=\/|$)/, os.homedir()))).entries.map((e) => `${dir}${e.name}${e.dir ? '/' : ''}`);
  } catch {
    return [];
  }
};

/** パスの最後の名前。ディレクトリは末尾の / を含める。 */
const lastName = (p: string): string => p.slice(p.lastIndexOf('/', p.length - 2) + 1);

/**
 * 送信欄で @ の後に打った語に合うパスを、名前の頭、パスの一部、パスに順に現れる文字の順に返す。
 * git のリポジトリでは、git が無視していないファイルをすべて探す。リポジトリの外と、~ か / で始まる語では、打ったディレクトリの中だけを見る。
 * 下の階層まで読むと、ホームディレクトリやリポジトリを束ねたディレクトリで、ファイルが多すぎて遅くなる。
 * 混んだマシンでは git が数秒かかるので、git の一覧は待たない。一覧をまだ覚えていなければ打ったディレクトリの中から答え、git に聞き終えたら onLoad で知らせる。
 */
export const fileCandidates = (cwd: string, word: string, onLoad: () => void): string[] => {
  const files = /^[~/]/.test(word) ? undefined : gitFiles(cwd, onLoad);
  // ponytail: 打つたびに一覧のすべてを小文字にして比べる。1 万件で数ミリ秒、30 万件で 100 ミリ秒ほどかかる。大きなリポジトリで遅くなったら、小文字にした一覧も覚える。
  return rankBy(files ?? entriesIn(cwd, word), word, lastName, (p) => p).slice(0, MAX_FILE_CANDIDATES);
};

/**
 * メモ帳で編集した文字で、ファイルを上書きする。modifiedAt は読んだときの更新時刻で、その後にほかで書き換えられていたら、上書きせずに changed を返す。
 * textarea は改行を LF にそろえるので、元のファイルの改行が CRLF なら CRLF に戻す。
 */
export const saveText = (full: string, content: string, modifiedAt: number): TextSaveResult => {
  const now = fs.statSync(full).mtimeMs;
  if (now !== modifiedAt) return { modifiedAt: now, changed: true };
  fs.writeFileSync(full, fs.readFileSync(full, 'utf8').includes('\r\n') ? content.replace(/\r?\n/g, '\r\n') : content);
  return { modifiedAt: fs.statSync(full).mtimeMs };
};
