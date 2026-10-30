// 発言の中でファイルを指すコード（`client/Home.tsx:32` など）と地の文のパスを見分け、コードの画面で開くファイルと行を探す。
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

// 日本語の文では、パスの後ろに空白を置かずに文を続けるので、パスに使う ASCII の文字が続くところまでをパスとみなす。
const TEXT_PATH = /([\w./~@+-]+\.\w+(?::\d+(?:-\d+)?)?)/;

/** 地の文を、ファイルを指すパスとそれ以外に分ける。 */
export const splitFilePaths = (text: string): { text: string; file: boolean }[] =>
  // 区切りをキャプチャすると、split の結果は当たらない部分と当たった部分が交互に並ぶ。
  text.split(TEXT_PATH).flatMap((part, i) => (part ? [{ text: part, file: i % 2 === 1 && isFileRef(part) }] : []));

/** 開くファイルと行。index は差分のファイルの並びでの位置。 */
export type Focus = { index: number; line?: number };

/** 絶対パスやファイル名だけの指定は、末尾が合うファイルを選ぶ。 */
export const findFocus = (files: DiffFile[], ref: string): Focus | undefined => {
  const [, path = ref, line] = /^(.*?)(?::(\d+)(?:-\d+)?)?$/.exec(ref) ?? [];
  const exact = files.findIndex((f) => f.path === path);
  const index = exact >= 0 ? exact : files.findIndex((f) => path.endsWith(`/${f.path}`) || f.path.endsWith(`/${path}`));
  return index >= 0 ? { index, line: line ? Number(line) : undefined } : undefined;
};

/** 差分の表の行の数。ハンクの見出しも 1 行に数える。 */
export const rowCount = (file: DiffFile): number => file.hunks.reduce((n, h) => n + 1 + h.lines.length, 0);

/** 差分のファイルを from 番目から順に、表の行の数が合わせて rows に収まるところまで描くとき、描かずに残る最初のファイルの位置。rows より多いファイルも、1 つは描く。 */
export const diffBatchEnd = (files: DiffFile[], from: number, rows: number): number => {
  let end = from;
  for (let total = 0; end < files.length; end++) {
    total += rowCount(files[end]!);
    if (end > from && total > rows) break;
  }
  return end;
};
