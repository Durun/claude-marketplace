// OS ごとに違う情報の取り方と操作を、ここにまとめる互換レイヤー。
// サーバーのほかのコードは process.platform を見ずに、必ず platform を経由する。
// OS ごとの実装は platformFor で名前から選べ、外のコマンドは run で差し替えられるので、どの OS の上でも全部の実装をテストできる。
import { execFile } from 'node:child_process';
import path from 'node:path';
import { promisify } from 'node:util';
import type { MemoryStatus } from '../shared/types.ts';
import { toMemoryStatus } from './memory.ts';

/** 外のコマンドを動かして標準出力を返す。テストでは偽物に差し替える。 */
export type Run = (file: string, args: string[]) => Promise<string>;

const exec = promisify(execFile);
const realRun: Run = async (file, args) => (await exec(file, args)).stdout;

export type Platform = {
  name: 'darwin' | 'win32' | 'other';
  /** 資料の書き出しに使う Chrome の、標準の場所。分からない OS では undefined で、CHROME_PATH で指してもらう。 */
  defaultChromePath: (env: NodeJS.ProcessEnv) => string | undefined;
  /** Claude Code がシェル（Windows では Git Bash）で動かすコマンドに書くパス。 */
  shellPath: (file: string) => string;
  /** メモリプレッシャーと内訳。読めない OS では null を返し、画面は表示を出さない。 */
  readMemory: (run?: Run) => Promise<MemoryStatus | null>;
  /**
   * MCP の headersHelper が、呼び出し元の claude の PID を X-Claude-Pid に入れられるか。
   * 入れられない OS では、connectionOwner で接続から引く。
   */
  headersHelperSendsPid: boolean;
  /** clientPort から serverPort へ TCP 接続を張っているプロセスの PID。 */
  connectionOwner: (clientPort: number, serverPort: number, run?: Run) => Promise<number | undefined>;
  /** 受信ソケットへ送れない理由。送れる OS では undefined。 */
  inboxBlocker?: string;
  /** dir の下に置く、このプロセスが listen できるローカルのソケットの場所。 */
  localSocketPath: (dir: string, name: string) => string;
};

const SYSCTL_NAMES = ['kern.memorystatus_vm_pressure_level', 'kern.memorystatus_level', 'hw.memsize', 'vm.swapusage'];

const posix = {
  shellPath: (file: string) => file,
  // sh の $PPID が claude の PID になる。
  headersHelperSendsPid: true,
  connectionOwner: async () => undefined,
  localSocketPath: (dir: string, name: string) => path.posix.join(dir, `${name}.sock`),
} satisfies Partial<Platform>;

const darwin: Platform = {
  ...posix,
  name: 'darwin',
  defaultChromePath: () => '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  readMemory: async (run = realRun) => {
    const [sysctl, vmStat] = await Promise.all([run('/usr/sbin/sysctl', ['-n', ...SYSCTL_NAMES]), run('/usr/bin/vm_stat', [])]);
    return toMemoryStatus(sysctl, vmStat);
  },
};

// ponytail: Linux ではメモリを読まない。広げるなら /proc/pressure/memory と /proc/meminfo から読む。
const other: Platform = {
  ...posix,
  name: 'other',
  defaultChromePath: () => undefined,
  readMemory: async () => null,
};

/** netstat -ano の行から、clientPort から serverPort へ張った TCP 接続を持つプロセスの PID を探す。 */
export const ownerInNetstat = (netstat: string, clientPort: number, serverPort: number): number | undefined => {
  for (const line of netstat.split('\n')) {
    const [proto, local, foreign, , pid] = line.trim().split(/\s+/);
    // 閉じた接続（TIME_WAIT）は PID が 0 になり、同じポートが後から別の接続に使われることがあるので読み飛ばす。
    if (proto?.startsWith('TCP') && local?.endsWith(`:${clientPort}`) && foreign?.endsWith(`:${serverPort}`) && Number(pid)) return Number(pid);
  }
  return undefined;
};

const win32: Platform = {
  name: 'win32',
  defaultChromePath: (env) => path.win32.join(env.PROGRAMFILES ?? 'C:\\Program Files', 'Google', 'Chrome', 'Application', 'chrome.exe'),
  // Git Bash は C:/… の形のパスを読める。\ のままだと、引用符の外でエスケープとして消える。
  shellPath: (file) => file.replaceAll('\\', '/'),
  // ponytail: Windows ではメモリを読まない。
  readMemory: async () => null,
  // headersHelper が cmd.exe で動き、$PPID が展開されずに文字のまま届く。
  headersHelperSendsPid: false,
  // MCP の接続を張っているのは claude のプロセスそのものなので、接続の持ち主がそのまま呼び出し元になる。
  connectionOwner: async (clientPort, serverPort, run = realRun) => ownerInNetstat(await run('netstat', ['-ano']), clientPort, serverPort),
  // 名前付きパイプは、最初の行で認証しない接続を、何も届けずに閉じる。届いたと偽らないよう、送らずに理由を返す。
  inboxBlocker: 'Windows の受信ソケットは接続ごとの認証が要り、まだ対応していません。入力待ちのバックグラウンドのセッションと、止まっているセッションには再開して届けられます',
  // Windows でプロセス間の接続を listen できるのは、ファイルシステムではなく名前付きパイプの名前空間。
  localSocketPath: (dir, name) => `\\\\.\\pipe\\${path.win32.basename(dir)}-${name}`,
};

export const platformFor = (name: string): Platform => (name === 'darwin' ? darwin : name === 'win32' ? win32 : other);

export const platform = platformFor(process.platform);

/**
 * claude の CLI を起動するコマンド。
 * テストは DURUN_DASHBOARD_FAKE_CLAUDE に node のスクリプトを指して、本物の代わりに動かす。
 * PATH に偽の実行ファイルを置く方法は、スクリプトをそのまま実行できない Windows では本物の claude を呼んでしまう。
 */
export const claudeCommand = (args: string[], env: NodeJS.ProcessEnv = process.env): { file: string; args: string[] } =>
  env.DURUN_DASHBOARD_FAKE_CLAUDE ? { file: process.execPath, args: [env.DURUN_DASHBOARD_FAKE_CLAUDE, ...args] } : { file: 'claude', args };
