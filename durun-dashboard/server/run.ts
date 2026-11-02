// 外のコマンドを、メインスレッドとは別のスレッドで起動する。
// spawn は、コマンドの exec が終わるまで呼んだスレッドを止める。macOS ではセキュリティソフトが exec を許すまで待つので、混んだマシンでは 1 本に数秒から 20 秒ほどかかる。
// メインスレッドで起動すると、その間はすべての API の応答と画面への配信が止まる。
import { type ExecFileException, execFile } from 'node:child_process';
import { parentPort, SHARE_ENV, Worker, workerData } from 'node:worker_threads';

/** input は標準入力に渡す本文。渡さなくても標準入力は閉じ、入力を待つコマンドを待たせない。 */
export type RunOptions = { cwd?: string; timeout?: number; maxBuffer?: number; input?: string };

/** execFile と同じく、失敗の理由に終了コードと出力を添える。killed は、timeout を過ぎて止めたこと。 */
export type RunError = Error & { code?: number | string; killed?: boolean; signal?: string; stdout: string; stderr: string };

type Request = { id: number; file: string; args: string[]; options: RunOptions };
type Reply = { id: number; stdout: string; stderr: string; error?: Pick<RunError, 'message' | 'code' | 'killed' | 'signal'> };

const ROLE = 'durun-dashboard:run';

if (workerData === ROLE) {
  parentPort?.on('message', ({ id, file, args, options: { input, ...options } }: Request) => {
    const reply = (e: ExecFileException | null, stdout: string, stderr: string): void => {
      const error = e ? { message: e.message, code: e.code ?? undefined, killed: e.killed, signal: e.signal ?? undefined } : undefined;
      parentPort?.postMessage({ id, stdout, stderr, error } satisfies Reply);
    };
    execFile(file, args, options, reply).stdin?.end(input);
  });
}

let worker: Worker | undefined;
let lastId = 0;
const waiting = new Map<number, (reply: Reply) => void>();

const startWorker = (): Worker => {
  // 環境変数は共有し、メインスレッドで変えた PATH などもコマンドに渡す。
  const w = new Worker(new URL(import.meta.url), { workerData: ROLE, env: SHARE_ENV });
  w.on('message', (reply: Reply) => {
    waiting.get(reply.id)?.(reply);
    waiting.delete(reply.id);
    // 待っているコマンドが無い間は、スレッドが残っていてもプロセスを終われるようにする。
    if (!waiting.size) w.unref();
  });
  w.on('error', (e) => console.error(`コマンドを起動するスレッドが止まりました: ${e.message}`));
  // 止まったスレッドからは答えが返らないので、待っている呼び手には失敗を返し、次に頼まれたら起動し直す。
  w.on('exit', () => {
    worker = undefined;
    for (const [id, done] of waiting) done({ id, stdout: '', stderr: '', error: { message: 'コマンドを起動するスレッドが止まりました' } });
    waiting.clear();
  });
  return w;
};

/** promisify した execFile と同じく、標準出力と標準エラーを返す。失敗したら RunError を投げる。 */
export const run = (file: string, args: string[], options: RunOptions = {}): Promise<{ stdout: string; stderr: string }> =>
  new Promise((resolve, reject) => {
    worker ??= startWorker();
    worker.ref();
    const id = ++lastId;
    waiting.set(id, ({ stdout, stderr, error }) => (error ? reject(Object.assign(new Error(error.message), error, { stdout, stderr })) : resolve({ stdout, stderr })));
    worker.postMessage({ id, file, args, options } satisfies Request);
  });
