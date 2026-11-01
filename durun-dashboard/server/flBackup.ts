// FL Connect が、エージェントが FL に書き込む前に取る、プロジェクトのバックアップの決めごと。ファイルを読み書きする部分は flconnect.ts に置く。
// 手順は 3 段：今のファイル（人間が最後に保存した版）を写す → 保存していない変更があれば上書き保存する → 保存したファイル（エージェントが触る直前の状態）を写す。
// 写し先はユーザーのフォルダーの外で、エージェントの操作のあとには保存しない。
// パスは Windows の形で扱う。開いているプロジェクトの手がかり（platform.ts の flProjectHints）を読めるのが Windows だけのため。
import fs from 'node:fs';
import path from 'node:path';
import type { FlProjectHints } from './platform.ts';

/** FL が名乗る版（FL 26.1.7 なら「Producer Edition v26.1.7 [build 5654]」）から、上の桁（26）を出す。最近のファイルを覚えているレジストリのキーの名前に使う。読めなければ NaN。 */
export const flMajorOf = (fl: string | undefined): number => Number(/(\d+)\.\d+\.\d+/.exec(fl ?? '')?.[1]);

/** 開いているプロジェクトのファイル。name は拡張子を除いた名前。untitled は名前の無いプロジェクト、unknown は当てられなかったとき。 */
export type ProjectFile = { kind: 'file'; file: string; name: string } | { kind: 'untitled' } | { kind: 'unknown'; why: string };

/**
 * FL のウィンドウの題名から、開いているプロジェクトのファイルの名前（拡張子つき）を出す。
 * FL 26.1.7 は「曲.flp - FL Studio 2026」と出し、名前の無いプロジェクトでは「FL Studio 2026」だけを出す（空の文字を返す）。
 */
export const projectNameOf = (title: string): string =>
  title
    .split(' - ')
    .filter((part) => !/^FL Studio\b/.test(part.trim()))
    .join(' - ')
    .replace(/\*$/, '')
    .trim();

/** ファイルを探すときの、ファイルシステムの読み方。テストでは偽物に差し替える。 */
export type FileLookup = { list: (dir: string) => { name: string; dir: boolean }[]; mtime: (file: string) => number | undefined };

const realLookup: FileLookup = {
  list: (dir) => {
    try {
      return fs.readdirSync(dir, { withFileTypes: true }).map((e) => ({ name: e.name, dir: e.isDirectory() }));
    } catch {
      return [];
    }
  },
  mtime: (file) => {
    try {
      return fs.statSync(file).mtimeMs;
    } catch {
      return undefined;
    }
  },
};

/** Projects のフォルダーを、何段まで降りて探すか。FL は新しいプロジェクトを「Projects\曲\曲.flp」のように、名前のフォルダーを作って保存する。 */
const SEARCH_DEPTH = 2;

/** dir の中と、depth 段下までのフォルダーから、name と同じ名前のファイルを集める。FL の自動保存の Backup のフォルダーは見ない。 */
const findIn = (dir: string, name: string, depth: number, io: FileLookup, out: string[]): void => {
  for (const e of io.list(dir)) {
    const file = path.win32.join(dir, e.name);
    if (!e.dir) {
      if (e.name.toLowerCase() === name.toLowerCase()) out.push(file);
    } else if (depth > 0 && e.name.toLowerCase() !== 'backup') findIn(file, name, depth - 1, io, out);
  }
};

/**
 * 開いているプロジェクトのファイルを当てる。FL の API はファイルの場所を返さず、FL が最近のファイルをレジストリに書くのは FL を閉じたときなので、
 * ウィンドウの題名に出るファイルの名前と同じファイルを、最近のファイルとそのフォルダー、FL のユーザーデータの Projects の下から探し、いちばん新しく書いたものにする。
 */
export const projectFileOf = (hints: FlProjectHints | undefined, projectsDir: string | undefined, io: FileLookup = realLookup): ProjectFile => {
  if (!hints) return { kind: 'unknown', why: 'FL のウィンドウの題名と、最近のファイルを読めなかった' };
  if (!hints.title) return { kind: 'unknown', why: 'FL のウィンドウが見つからなかった' };
  const name = projectNameOf(hints.title);
  if (!name) return { kind: 'untitled' };
  if (!/\.flp$/i.test(name)) return { kind: 'unknown', why: `「${name}」は .flp ではない（まとめて書き出したプロジェクトは、上書き保存で何が起きるか確かめていない）` };
  const found = hints.recent.filter((f) => path.win32.basename(f).toLowerCase() === name.toLowerCase());
  for (const dir of new Set(hints.recent.map((f) => path.win32.dirname(f)))) findIn(dir, name, 0, io, found);
  if (projectsDir) findIn(projectsDir, name, SEARCH_DEPTH, io, found);
  const unique = [...new Map(found.map((f) => [f.toLowerCase(), f])).values()];
  const newest = unique
    .map((file) => ({ file, at: io.mtime(file) }))
    .filter((c): c is { file: string; at: number } => c.at !== undefined)
    .sort((a, b) => b.at - a.at)[0];
  if (!newest) return { kind: 'unknown', why: `「${name}」が、FL の最近のファイルのフォルダーにも、FL のユーザーデータの Projects の下にも見当たらない` };
  return { kind: 'file', file: newest.file, name: name.replace(/\.flp$/i, '') };
};

/** バックアップの手順。copy は今のファイルを写すか、save は上書き保存してもう一度写すか。note は、省いた手順とその理由。 */
export type BackupPlan = { copy: boolean; save: boolean; note?: string };

/**
 * エージェントが書き込む前に取る手順を決める。省くときは undefined。
 * - 前のバックアップのあとで人間が FL を触っていなければ、取り直さない。そのあとの変更はエージェントのものなので、前のバックアップがエージェントの前の状態になっている。
 * - 保存していない変更にエージェントの変更が混ざっていても、上書き保存する（人間が決めた。2026-11-01）。
 * - 録音している間は、上書き保存しない。
 * changed は general.getChangedFlag() の値（0 は保存してから変わっていない、1 と 2 は保存していない変更がある）。
 */
export const backupPlan = (s: { changed: number | undefined; humanSinceBackup: boolean; recording: boolean }): BackupPlan | undefined => {
  if (!s.humanSinceBackup) return undefined;
  if (!s.changed) return { copy: true, save: false };
  if (s.recording) return { copy: true, save: false, note: '録音しているので、上書き保存しなかった' };
  return { copy: true, save: true };
};

const pad = (n: number): string => String(n).padStart(2, '0');

/** バックアップのファイルの名前。kind は、saved が人間が最後に保存した版、before がエージェントが触る直前の状態。時刻はこの PC の時刻で、ファイルの名前に使えない : は使わない。 */
export const backupFileName = (name: string, at: number, kind: 'saved' | 'before'): string => {
  const d = new Date(at);
  const stamp = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}-${pad(d.getMinutes())}-${pad(d.getSeconds())}`;
  return `${name} ${stamp} ${kind === 'saved' ? '前回の保存' : 'エージェントの前'}.flp`;
};
