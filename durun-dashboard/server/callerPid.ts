// Windows では MCP の headersHelper が cmd.exe で動き、$PPID が展開されずに文字のまま X-Claude-Pid に入る。
// MCP の接続を張っているのは claude のプロセスそのものなので、接続の相手のポートを持つプロセスを netstat で引く。
import { execFile } from 'node:child_process';
import type { IncomingMessage } from 'node:http';
import type { Socket } from 'node:net';
import { promisify } from 'node:util';

const run = promisify(execFile);

/** netstat -ano の行から、clientPort から serverPort へ張った TCP 接続を持つプロセスの PID を探す。 */
export const ownerOfConnection = (netstat: string, clientPort: number, serverPort: number): number | undefined => {
  for (const line of netstat.split('\n')) {
    const [proto, local, foreign, , pid] = line.trim().split(/\s+/);
    // 閉じた接続（TIME_WAIT）は PID が 0 になり、同じポートが後から別の接続に使われることがあるので読み飛ばす。
    if (proto?.startsWith('TCP') && local?.endsWith(`:${clientPort}`) && foreign?.endsWith(`:${serverPort}`) && Number(pid)) return Number(pid);
  }
  return undefined;
};

// keep-alive で同じ接続に続けて届く呼び出しのたびに netstat を走らせないよう、接続ごとに覚える。
const owners = new WeakMap<Socket, Promise<number | undefined>>();

/** Windows で X-Claude-Pid が数でなければ、接続を持つプロセスの PID に差し替える。Mac では headersHelper が付けるので何もしない。 */
export const fillCallerPid = async (req: IncomingMessage): Promise<void> => {
  if (process.platform !== 'win32' || Number(req.headers['x-claude-pid'])) return;
  const { socket } = req;
  let owner = owners.get(socket);
  if (!owner) {
    owner = run('netstat', ['-ano']).then(
      ({ stdout }) => ownerOfConnection(stdout, socket.remotePort ?? 0, socket.localPort ?? 0),
      () => undefined,
    );
    owners.set(socket, owner);
  }
  const pid = await owner;
  if (pid) req.headers['x-claude-pid'] = String(pid);
};
