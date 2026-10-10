// 本物の claude の代わりに動かす偽物を、node のスクリプトで置く。sh のスクリプトにすると、Windows では実行できない。
import fs from 'node:fs';
import path from 'node:path';

// 偽物の本体から使える道具。args は claude に渡された引数、stdin() は標準入力の全文。
// later は、偽物が終わった後に別のプロセスでコードを動かす。claude stop がすぐに戻り、後から名簿が書き換わる様子を再現する。
const PREAMBLE = `
import fs from 'node:fs';
import { spawn } from 'node:child_process';
const args = process.argv.slice(2);
const stdin = () => fs.readFileSync(0, 'utf8');
const lines = (items) => items.map((item) => item + '\\n').join('');
const later = (ms, code) => spawn(process.execPath, ['-e', 'const fs = require("node:fs"); setTimeout(() => {' + code + '}, ' + ms + ')'], { detached: true, stdio: 'ignore' }).unref();
`;

let count = 0;

/** 本体に値を埋め込むときの書き方。Windows のパスの \ も、そのまま文字列リテラルになる。 */
export const js = (value: unknown): string => JSON.stringify(value);

/**
 * これより後に起動される claude を、body を本体とする偽物にする。
 * 失敗させるときは process.exit ではなく process.exitCode を使う。出力を書き終える前に終わらせない。
 */
export const useFakeClaude = (dir: string, body: string): void => {
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `fake-claude-${count++}.mjs`);
  fs.writeFileSync(file, `${PREAMBLE}\n${body}\n`);
  process.env.DURUN_DASHBOARD_FAKE_CLAUDE = file;
};
