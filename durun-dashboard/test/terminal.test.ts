import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import type { ServerResponse } from 'node:http';
import os from 'node:os';
import { test } from 'node:test';
import { streamTerminal, writeTerminal } from '../server/terminal.ts';

/** 書かれた SSE を body に貯める偽の応答。end されたら、接続が閉じたものとして close を出す。 */
const fakeResponse = () => {
  const res = Object.assign(new EventEmitter(), {
    body: '',
    writableEnded: false,
    destroyed: false,
    writeHead: () => res,
    write: (chunk: string) => {
      res.body += chunk;
      return true;
    },
    end: (chunk: string) => {
      res.body += chunk;
      res.writableEnded = true;
      res.emit('close');
      return res;
    },
  });
  return res;
};

/** SSE の data を、event の種類によらずつないだもの。端末の出力は分かれて届くことがある。 */
const dataOf = (body: string): string => [...body.matchAll(/^data: (.*)$/gm)].map((m) => JSON.parse(m[1] ?? '""') as string).join('');

test('コマンドプロンプトは、アタッチするコマンドを端末で動かして出力を流し、ready の id で入力を届け、コマンドが終わったら exit を送って閉じる', { timeout: 10_000 }, async () => {
  // サーバーと同じく、node --watch の子が受け継ぐ変数を持たせても、止めるときに例外を投げない。
  process.env.WATCH_REPORT_DEPENDENCIES = '1';
  const res = fakeResponse();
  const closed = new Promise((resolve) => res.once('close', resolve));
  // どの OS のシェルでも同じに動くよう、1 行読んで返すコマンドを node で書く。
  const command = `"${process.execPath}" -e "process.stdin.once('data', (d) => { console.log('got:' + d.toString().trim()); process.exit(0); })"`;
  await streamTerminal(res as unknown as ServerResponse, os.tmpdir(), command);
  const id = JSON.parse(/^event: ready\ndata: (.*)$/m.exec(res.body)?.[1] ?? '""') as string;
  assert.ok(writeTerminal(id, 'hello\r'));
  await closed;
  assert.match(dataOf(res.body), /got:hello/);
  assert.match(res.body, /event: exit\ndata: ".*終了コード 0/);
  assert.equal(writeTerminal(id, 'x'), false);
});

test('コマンドプロンプトは、サーバーが受け継いだ COLORTERM を、アタッチするコマンドに渡さない', { timeout: 10_000 }, async () => {
  process.env.COLORTERM = 'truecolor';
  const res = fakeResponse();
  const closed = new Promise((resolve) => res.once('close', resolve));
  await streamTerminal(res as unknown as ServerResponse, os.tmpdir(), `"${process.execPath}" -e "console.log('colorterm:' + process.env.COLORTERM)"`);
  await closed;
  assert.match(dataOf(res.body), /colorterm:undefined/);
});

test('コマンドプロンプトは、アタッチするコマンドが無いか作業ディレクトリが無ければ、端末を開かずに理由を exit で送る', async () => {
  const unset = fakeResponse();
  await streamTerminal(unset as unknown as ServerResponse, os.tmpdir(), '');
  assert.match(unset.body, /^event: exit\ndata: ".*TERMINAL_ATTACH_COMMAND/);
  const missing = fakeResponse();
  await streamTerminal(missing as unknown as ServerResponse, '/no/such/dir', 'tmux attach');
  assert.match(missing.body, /^event: exit\ndata: "ディレクトリがありません/);
});
