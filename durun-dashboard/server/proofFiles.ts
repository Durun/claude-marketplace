// 文章校正のファイル。1 件を 1 つの JSON にして置き、校正を止めても、後から開いて続けられるようにする。
import fs from 'node:fs';
import path from 'node:path';
import type { ProofFile, ProofFileMeta } from '../shared/types.ts';
import { DATA_DIR } from './store.ts';

const DIR = path.join(DATA_DIR, 'proofread');
// Windows XP の「最近使ったファイル」と同じく、新しい順に 15 件まで出す。
const RECENT = 15;

const fileOf = (id: string): string => path.join(DIR, `${id}.json`);

/** Claude が MCP でコメントか修正版を書き込んだ時刻を、ファイルの id ごとに持つ。開いているアプリは、時刻が変わったらファイルを読み直して取り込む。 */
export const proofCommentedAt: Record<string, number> = {};

/** 最後に保存した順に並べる。 */
export const recentProofFiles = (): ProofFileMeta[] => {
  if (!fs.existsSync(DIR)) return [];
  return fs.readdirSync(DIR)
    .filter((name) => name.endsWith('.json'))
    .map((name) => ({ name, at: fs.statSync(path.join(DIR, name)).mtimeMs }))
    .sort((a, b) => b.at - a.at)
    .slice(0, RECENT)
    .map(({ name }) => {
      const { id, title, updatedAt } = JSON.parse(fs.readFileSync(path.join(DIR, name), 'utf8')) as ProofFile;
      return { id, title, updatedAt };
    });
};

/** id は、呼び出し側で英数字と - と _ だけに絞ってから渡す。 */
export const readProofFile = (id: string): ProofFile | null => (fs.existsSync(fileOf(id)) ? (JSON.parse(fs.readFileSync(fileOf(id), 'utf8')) as ProofFile) : null);

/** 書いている途中で止まっても壊れたファイルを残さないよう、別名で書いてから置き換える。 */
export const writeProofFile = (file: ProofFile): void => {
  fs.mkdirSync(DIR, { recursive: true });
  const tmp = `${fileOf(file.id)}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(file));
  fs.renameSync(tmp, fileOf(file.id));
};
