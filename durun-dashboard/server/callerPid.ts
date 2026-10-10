// MCP の呼び出し元の claude の PID は、headersHelper が X-Claude-Pid で送る。送れない OS では、接続の持ち主から引いて補う。
import type { IncomingMessage } from 'node:http';
import type { Socket } from 'node:net';
import { platform as current, type Platform } from './platform.ts';

// keep-alive で同じ接続に続けて届く呼び出しのたびに持ち主を引き直さないよう、接続ごとに覚える。
const owners = new WeakMap<Socket, Promise<number | undefined>>();

export const fillCallerPid = async (req: IncomingMessage, platform: Platform = current): Promise<void> => {
  if (platform.headersHelperSendsPid) return;
  const { socket } = req;
  let owner = owners.get(socket);
  if (!owner) {
    // 引けなくても呼び出しは通す。セッションを特定できないことは、ツールの結果で Claude に伝わる。
    owner = platform.connectionOwner(socket.remotePort ?? 0, socket.localPort ?? 0).catch(() => undefined);
    owners.set(socket, owner);
  }
  const pid = await owner;
  if (pid) req.headers['x-claude-pid'] = String(pid);
};
