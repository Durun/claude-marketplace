// エクスプローラーに出す、ディレクトリの一覧を読む。
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { ExplorerDir, ExplorerEntry } from '../shared/types.ts';
import { type Platform, platform } from './platform.ts';

// node_modules のような大きなディレクトリで、画面を待たせすぎない件数。
const MAX_ENTRIES = 2000;

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
