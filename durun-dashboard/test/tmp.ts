// テストが使う一時ディレクトリ。テストのプロセスが終わるときに消して、OS の一時フォルダーに残さない。
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const dirs: string[] = [];

/**
 * 作ったディレクトリを全部消す。テストが終わってすぐではなく、プロセスが終わるときに消す。
 * store の保存のように、テストが予約した書き込みが後から動いて、消したディレクトリを作り直すため。
 * 消すのは後片付けなので、消せなくてもテストを失敗にしない（Windows では、ほかのプロセスが開いているファイルがあると消せない）。
 */
const removeAll = (): void => {
  for (const dir of dirs) {
    try {
      fs.rmSync(dir, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    } catch {
      // 次のディレクトリを消す。
    }
  }
};

/** OS の一時フォルダーの下に、prefix で始まるディレクトリを作る。テストのプロセスが終わるときに消す。 */
export const tempDir = (prefix: string): string => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  if (!dirs.length) process.once('exit', removeAll);
  dirs.push(dir);
  return dir;
};
