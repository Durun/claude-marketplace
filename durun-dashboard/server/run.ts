// 外のコマンドを、メインスレッドとは別のスレッドで起動する。
// spawn は、コマンドの exec が終わるまで呼んだスレッドを止める。macOS ではセキュリティソフトが exec を許すまで待つので、混んだマシンでは 1 本に数秒から 20 秒ほどかかる。
// メインスレッドで起動すると、その間はすべての API の応答と画面への配信が止まる。
import { type ExecFileException, execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import { parentPort, SHARE_ENV, Worker, workerData } from 'node:worker_threads';

/** input は標準入力に渡す本文。渡さなくても標準入力は閉じ、入力を待つコマンドを待たせない。 */
export type RunOptions = { cwd?: string; timeout?: number; maxBuffer?: number; input?: string };

/** execFile と同じく、失敗の理由に終了コードと出力を添える。killed は、timeout を過ぎて止めたこと。 */
export type RunError = Error & { code?: number | string; killed?: boolean; signal?: string; stdout: string; stderr: string };

type Request = { id: number; file: string; args: string[]; options: RunOptions };
type Reply = { id: number; stdout: string; stderr: string; error?: Pick<RunError, 'message' | 'code' | 'killed' | 'signal'> };
type LaunchRequest = { id: number; file: string; args: string[]; cwd: string; log: string };
/** 起動したら pid を、終わったら終了コードを、起動か実行に失敗したら error を返す。 */
type LaunchReply = { id: number; launch: true; pid?: number; exitCode?: number | null; error?: string; workerStopped?: boolean };

const ROLE = 'durun-dashboard:run';

const launchInWorker = ({ id, file, args, cwd, log }: LaunchRequest): void => {
  const post = (reply: Omit<LaunchReply, 'id' | 'launch'>): void => parentPort?.postMessage({ id, launch: true, ...reply } satisfies LaunchReply);
  try {
    const fd = fs.openSync(log, 'a');
    const child = spawn(file, args, { cwd, detached: true, stdio: ['ignore', fd, fd] });
    fs.closeSync(fd);
    child.on('spawn', () => post({ pid: child.pid }));
    child.on('error', (e) => post({ error: e.message }));
    child.on('exit', (exitCode) => post({ exitCode }));
  } catch (e) {
    post({ error: (e as Error).message });
  }
};

if (workerData === ROLE) {
  parentPort?.on('message', (req: Request | LaunchRequest) => {
    if ('log' in req) return launchInWorker(req);
    const { id, file, args, options: { input, ...options } } = req;
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
const launches = new Map<number, (reply: LaunchReply) => void>();

const startWorker = (): Worker => {
  // 環境変数は共有し、メインスレッドで変えた PATH などもコマンドに渡す。
  const w = new Worker(new URL(import.meta.url), { workerData: ROLE, env: SHARE_ENV });
  w.on('message', (reply: Reply | LaunchReply) => {
    if ('launch' in reply) launches.get(reply.id)?.(reply);
    else {
      waiting.get(reply.id)?.(reply);
      waiting.delete(reply.id);
    }
    // 待っているコマンドが無い間は、スレッドが残っていてもプロセスを終われるようにする。
    if (!waiting.size && !launches.size) w.unref();
  });
  w.on('error', (e) => console.error(`コマンドを起動するスレッドが止まりました: ${e.message}`));
  // 止まったスレッドからは答えが返らないので、待っている呼び手には失敗を返し、次に頼まれたら起動し直す。
  w.on('exit', () => {
    worker = undefined;
    for (const [id, done] of waiting) done({ id, stdout: '', stderr: '', error: { message: 'コマンドを起動するスレッドが止まりました' } });
    waiting.clear();
    // 起動を待っている呼び手には失敗を返す。起動したコマンドは別のプロセスグループで動き続けるが、終わりはもう受け取れない。
    for (const [id, done] of launches) done({ id, launch: true, error: 'コマンドを起動するスレッドが止まりました', workerStopped: true });
    launches.clear();
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

/**
 * サーバーが再起動しても止まらないよう、別のプロセスグループで起動し、出力は log に足す。起動したら pid を返す。
 * 終わったら onExit に終了コードを、起動した後に失敗したら理由を渡す。起動に失敗したら投げる。
 */
export const launchDetached = (file: string, args: string[], { cwd, log }: { cwd: string; log: string }, onExit: (code: number | null, error?: string) => void): Promise<number> =>
  new Promise((resolve, reject) => {
    worker ??= startWorker();
    worker.ref();
    const id = ++lastId;
    let pid: number | undefined;
    launches.set(id, (reply) => {
      if (pid === undefined && reply.pid !== undefined) return resolve((pid = reply.pid));
      launches.delete(id);
      if (pid === undefined) reject(new Error(reply.error ?? 'コマンドを起動できません'));
      else if (!reply.workerStopped) onExit(reply.exitCode ?? null, reply.error);
    });
    worker.postMessage({ id, file, args, cwd, log } satisfies LaunchRequest);
  });
