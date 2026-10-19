import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 要約を書き出すファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-log-summaries-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
const { saveLogSummaries, summarizeLog } = await import('../server/claude.ts');

const prompt = (text: string): string => `${JSON.stringify({ type: 'user', origin: { kind: 'human' }, timestamp: new Date().toISOString(), message: { content: text } })}\n`;

test('書き出した会話ログの要約は、起動し直した後に続きから読み、前に読んだところは読み直さない', async () => {
  const file = path.join(tmp, 'log.jsonl');
  fs.writeFileSync(file, prompt('一つ目'));
  assert.deepEqual(summarizeLog(file).texts.map((t) => t.text), ['一つ目']);
  saveLogSummaries();
  // 読み直したら分かるよう、読んだところを同じ長さの別の依頼に書き換えてから、続きを足す。
  fs.writeFileSync(file, prompt('二つ目') + prompt('三つ目'));
  // 別の URL で読み込むと、起動し直したときと同じく、ファイルから要約を読んだモジュールになる。
  const restarted = await import(new URL('../server/claude.ts?restarted', import.meta.url).href);
  assert.deepEqual(restarted.summarizeLog(file).texts.map((t: { text: string }) => t.text), ['一つ目', '三つ目']);
});

test('スラッシュコマンドの依頼は、タグを外して打ったとおりの「/名前 引数」で見せる', () => {
  const file = path.join(tmp, 'command.jsonl');
  fs.writeFileSync(file, prompt('<command-message>imozuru</command-message>\n<command-name>/imozuru</command-name>\n<command-args>対位法の使い方</command-args>') + prompt('<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>'));
  assert.deepEqual(summarizeLog(file).texts.map((t) => t.text), ['/imozuru 対位法の使い方', '/clear']);
});
