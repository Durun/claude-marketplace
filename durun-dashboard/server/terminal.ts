// アプリ「ターミナル」の中継。TERMINAL_ATTACH_COMMAND に書いたコマンドを端末で動かして screen や tmux のセッションにアタッチし、その端末と画面の入出力をつなぐ。
// ダッシュボードはコードを変えるたびに起動し直すので、端末の中身は screen や tmux に持たせ、ここではアタッチするコマンドだけを動かす。
import fs from 'node:fs';
import type { ServerResponse } from 'node:http';
import type { IPty } from 'node-pty';
import { platform } from './platform.ts';
import { newId } from './store.ts';

/** 開いている端末。画面は、接続したときに ready で受け取った id で、入力と大きさの変更を送る。 */
const terminals = new Map<string, IPty>();

/**
 * cwd でアタッチするコマンドを動かし、端末の出力を SSE で流す。最初に ready で id を送り、コマンドが終わったら exit で理由を送って閉じる。
 * 画面が接続を切ったら、アタッチしたコマンドだけを止める。screen や tmux のセッションは残る。
 */
export const streamTerminal = async (res: ServerResponse, cwd: string, command = process.env.TERMINAL_ATTACH_COMMAND): Promise<void> => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  const exit = (message: string): void => void res.end(`event: exit\ndata: ${JSON.stringify(message)}\n\n`);
  if (!command) return exit('~/.durun-dashboard/.env.local の TERMINAL_ATTACH_COMMAND に、アタッチするコマンドを書いてください。');
  if (!fs.statSync(cwd, { throwIfNoEntry: false })?.isDirectory()) return exit(`ディレクトリがありません: ${cwd}`);
  let term: IPty;
  try {
    // ネイティブのモジュールを読めない環境でも、ほかの機能は動くよう、開くときに読む。
    const { spawn } = await import('node-pty');
    const { file, args } = platform.shellCommand(command);
    // ダッシュボードを screen や tmux の中で起動していても、入れ子と見なされずにアタッチできるよう、その印の変数を外す。
    const { TMUX, STY, ...env } = process.env;
    term = spawn(file, args, { name: 'xterm-256color', cwd, env });
  } catch (e) {
    return exit(`端末を開けません（${(e as Error).message}）`);
  }
  // モジュールを読んでいる間に、画面が接続を切っていることがある。
  if (res.destroyed) return term.kill();
  const id = newId();
  terminals.set(id, term);
  res.write(`event: ready\ndata: ${JSON.stringify(id)}\n\n`);
  term.onData((data) => {
    // 画面が受け取りきれないときは、送った分が流れるまで端末を止める。
    if (!res.writableEnded && !res.write(`data: ${JSON.stringify(data)}\n\n`)) {
      term.pause();
      res.once('drain', () => term.resume());
    }
  });
  term.onExit(({ exitCode }) => {
    terminals.delete(id);
    exit(`アタッチしたコマンドが終わりました（終了コード ${exitCode}）。`);
  });
  res.on('close', () => {
    terminals.delete(id);
    term.kill();
  });
};

/** 開いている端末に入力を送る。閉じていれば false を返す。 */
export const writeTerminal = (id: string, data: string): boolean => {
  const term = terminals.get(id);
  term?.write(data);
  return !!term;
};

/** 開いている端末の大きさを変える。閉じていれば false を返す。 */
export const resizeTerminal = (id: string, cols: number, rows: number): boolean => {
  const term = terminals.get(id);
  term?.resize(cols, rows);
  return !!term;
};
