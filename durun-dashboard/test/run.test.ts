import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { launchDetached, run, type RunError } from '../server/run.ts';
import { tempDir } from './tmp.ts';

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

test('launchDetached は、別のプロセスグループで起動して pid を返し、出力を log に足して、終わったら終了コードを渡す', async () => {
  const log = path.join(tempDir('durun-dashboard-run-'), 'log');
  fs.writeFileSync(log, 'before\n');
  const [file, args] = node('console.log("out"); process.exit(3)');
  let exited: (code: number | null) => void = () => {};
  const code = new Promise<number | null>((resolve) => (exited = resolve));
  const pid = await launchDetached(file, args, { cwd: process.cwd(), log }, (c) => exited(c));
  assert.ok(pid > 0);
  assert.equal(await code, 3);
  assert.equal(fs.readFileSync(log, 'utf8'), 'before\nout\n');
  await assert.rejects(launchDetached(path.join(log, 'missing'), [], { cwd: process.cwd(), log }, () => {}));
});
