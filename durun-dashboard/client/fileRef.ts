// 発言の中でファイルを指すコード（`client/Home.tsx:32` など）を見分け、コードの画面で開くファイルと行を探す。
import { type DiffFile, MEDIA_TYPES } from '../shared/types.ts';
import { languageOf } from './highlight.ts';

// 空白を含まず、拡張子と、省略できる行番号で終わるもの。
const FILE_REF = /^[^\s:]+\.\w+(?::\d+(?:-\d+)?)?$/;
// スキームを省いた URL（example.com/main.js）。最初の区切りがドメイン名に見えるものは、ファイルとみなさない。
const HOST_PREFIX = /^[\w-]+(?:\.[\w-]+)*\.[a-z]{2,}\//i;

/** エクスプローラーからドラッグしたファイルの、絶対パスを運ぶ種類。資料のウィジェットが受け取る。 */
export const FILE_DRAG_TYPE = 'application/x-durun-file';

/** 画像か動画なら、その MIME タイプ。 */
export const mediaTypeOf = (path: string): string | undefined => MEDIA_TYPES[/\.\w+$/.exec(path)?.[0].toLowerCase() ?? ''];

/** `meta.title` のような式と見分けるため、色付けできる拡張子か、画像と動画の拡張子のものに限る。 */
export const isFileRef = (code: string): boolean => {
  const path = code.replace(/:[\d-]+$/, '');
  return FILE_REF.test(code) && !HOST_PREFIX.test(code) && !!(languageOf(path) || mediaTypeOf(path));
};

/** 開くファイルと行。index は差分のファイルの並びでの位置。 */
export type Focus = { index: number; line?: number };

/** 絶対パスやファイル名だけの指定は、末尾が合うファイルを選ぶ。 */
export const findFocus = (files: DiffFile[], ref: string): Focus | undefined => {
  const [, path = ref, line] = /^(.*?)(?::(\d+)(?:-\d+)?)?$/.exec(ref) ?? [];
  const exact = files.findIndex((f) => f.path === path);
  const index = exact >= 0 ? exact : files.findIndex((f) => path.endsWith(`/${f.path}`) || f.path.endsWith(`/${path}`));
  return index >= 0 ? { index, line: line ? Number(line) : undefined } : undefined;
};
