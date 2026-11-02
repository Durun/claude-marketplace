import assert from 'node:assert/strict';
import { test } from 'node:test';
import { run, type RunError } from '../server/run.ts';

// どの OS にもあるコマンドとして、テストを動かしている node を使う。
const node = (script: string): [string, string[]] => [process.execPath, ['-e', script]];

test('run は、標準入力に input を渡し、標準出力と標準エラーを返す', async () => {
  const [file, args] = node('process.stdin.pipe(process.stdout); console.error("note")');
  assert.deepEqual(await run(file, args, { input: 'hello' }), { stdout: 'hello', stderr: 'note\n' });
});

test('run は、失敗したら終了コードと出力を添えて投げ、timeout を過ぎたら止めて killed を立てる', async () => {
  const [file, args] = node('console.log("out"); console.error("bad"); process.exit(3)');
  await assert.rejects(run(file, args), (e: RunError) => e.code === 3 && e.stdout === 'out\n' && e.stderr === 'bad\n' && !e.killed);
  const [slow, wait] = node('setTimeout(() => {}, 10_000)');
  await assert.rejects(run(slow, wait, { timeout: 100 }), (e: RunError) => e.killed === true);
});
