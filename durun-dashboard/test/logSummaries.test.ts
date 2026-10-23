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

test('巻き戻して送った依頼に Claude がまだ何も言っていなければ、起動し直した後も、捨てられた分岐の最後の発言とツールを残さない', async () => {
  const file = path.join(tmp, 'rewound.jsonl');
  const entry = (o: object): string => `${JSON.stringify({ timestamp: new Date().toISOString(), ...o })}\n`;
  fs.writeFileSync(
    file,
    entry({ type: 'user', uuid: 'u1', parentUuid: 'a0', origin: { kind: 'human' }, message: { content: 'Python で' } }) +
      entry({ type: 'assistant', uuid: 'm1', parentUuid: 'u1', message: { content: [{ type: 'text', text: 'Python で作ります' }, { type: 'tool_use', name: 'Edit', input: {} }] } }),
  );
  assert.equal(summarizeLog(file).lastMessage, 'Python で作ります');
  saveLogSummaries();
  // 最初の依頼を書き直して送ると、書き直す前の依頼と同じ親につながる。
  fs.appendFileSync(file, entry({ type: 'user', uuid: 'u2', parentUuid: 'a0', promptSource: 'sdk', message: { content: 'TypeScript で' } }));
  const restarted = await import(new URL('../server/claude.ts?rewound', import.meta.url).href);
  const log = restarted.summarizeLog(file);
  assert.deepEqual([log.lastPrompt, log.lastMessage, log.lastMessageAt, log.lastToolUse], ['TypeScript で', undefined, undefined, undefined]);
});

test('スラッシュコマンドの依頼は、タグを外して打ったとおりの「/名前 引数」で見せる', () => {
  const file = path.join(tmp, 'command.jsonl');
  fs.writeFileSync(file, prompt('<command-message>imozuru</command-message>\n<command-name>/imozuru</command-name>\n<command-args>対位法の使い方</command-args>') + prompt('<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>'));
  assert.deepEqual(summarizeLog(file).texts.map((t) => t.text), ['/imozuru 対位法の使い方', '/clear']);
});
