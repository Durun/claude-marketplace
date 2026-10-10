// OS ごとに違う情報の取り方と操作を、ここにまとめる互換レイヤー。
// サーバーのほかのコードは process.platform を見ずに、必ず platform を経由する。
// OS ごとの実装は platformFor で名前から選べ、外のコマンドは run で差し替えられるので、どの OS の上でも全部の実装をテストできる。
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import type { HostOs, MemoryStatus } from '../shared/types.ts';
import { toMemoryStatus } from './memory.ts';

/** 外のコマンドを動かして標準出力を返す。テストでは偽物に差し替える。 */
export type Run = (file: string, args: string[]) => Promise<string>;

const exec = promisify(execFile);
const realRun: Run = async (file, args) => (await exec(file, args)).stdout;

/** ディレクトリの一覧とファイルの中身を読む。テストでは偽物に差し替える。 */
export type Files = { list: (dir: string) => string[]; read: (file: string) => string };

const realFiles: Files = { list: (dir) => fs.readdirSync(dir), read: (file) => fs.readFileSync(file, 'utf8') };

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
  /**
   * 受信ソケットの接続の最初の行で名乗る、認証のトークン。認証の要らない OS では undefined。
   * pid は送り先のセッションの claude のプロセス、sessionsDir は Claude Code がセッションファイルを置く場所。読めなければ例外を投げる。
   */
  inboxToken: (pid: number, sessionsDir: string, files?: Files) => string | undefined;
  /** dir の下に置く、このプロセスが listen できるローカルのソケットの場所。 */
  localSocketPath: (dir: string, name: string) => string;
  /** この端末を tailnet の中から指す名前と IP。Tailscale が入っていないか、つながっていなければ空。 */
  tailnetHosts: (run?: Run) => Promise<string[]>;
};

/** tailscale status --json から、この端末の MagicDNS の名前（完全な名前と先頭のラベル）と IP を取り出す。IPv6 は Host ヘッダーと同じく [] で囲む。 */
export const hostsInTailscaleStatus = (status: string): string[] => {
  const { BackendState, Self } = JSON.parse(status) as { BackendState?: string; Self?: { DNSName?: string; TailscaleIPs?: string[] } };
  if (BackendState !== 'Running' || !Self) return [];
  const dnsName = (Self.DNSName ?? '').replace(/\.$/, '').toLowerCase();
  const names = dnsName ? [dnsName, dnsName.split('.')[0] ?? ''] : [];
  return [...names, ...(Self.TailscaleIPs ?? []).map((ip) => (ip.includes(':') ? `[${ip}]` : ip))].filter(Boolean);
};

/** commands を順に試し、最初に動いた tailscale の CLI に聞く。どれも動かなければ、Tailscale が無いものとして空を返す。 */
const tailnetHostsVia =
  (commands: string[]) =>
  async (run: Run = realRun): Promise<string[]> => {
    for (const command of commands) {
      try {
        return hostsInTailscaleStatus(await run(command, ['status', '--json', '--peers=false']));
      } catch {
        // CLI が無いか、Tailscale が止まっている。次の場所を試す。
      }
    }
    return [];
  };

const SYSCTL_NAMES = ['kern.memorystatus_vm_pressure_level', 'kern.memorystatus_level', 'hw.memsize', 'vm.swapusage'];

const posix = {
  shellPath: (file: string) => file,
  // sh の $PPID が claude の PID になる。
  headersHelperSendsPid: true,
  connectionOwner: async () => undefined,
  // Unix ソケットは OS のユーザーだけが開けるので、Claude Code は認証の行を求めない。
  inboxToken: () => undefined,
  localSocketPath: (dir: string, name: string) => path.posix.join(dir, `${name}.sock`),
  tailnetHosts: tailnetHostsVia(['tailscale']),
} satisfies Partial<Platform>;

const darwin: Platform = {
  ...posix,
  name: 'darwin',
  defaultChromePath: () => '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  readMemory: async (run = realRun) => {
    const [sysctl, vmStat] = await Promise.all([run('/usr/sbin/sysctl', ['-n', ...SYSCTL_NAMES]), run('/usr/bin/vm_stat', [])]);
    return toMemoryStatus(sysctl, vmStat);
  },
  // App Store と配布版の Tailscale は、CLI を PATH に置かず、アプリの中に持つ。
  tailnetHosts: tailnetHostsVia(['tailscale', '/Applications/Tailscale.app/Contents/MacOS/Tailscale']),
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
  // 名前付きパイプは、最初の行で認証しない接続を、何も届けずに閉じる。
  // Claude Code は、OS のユーザーだけが読める <pid>.<識別子>.key をセッションファイルの隣に置く。その peerToken を名乗ると、別のセッションからのメッセージとして届く。
  // ponytail: この鍵の形は Claude Code の文書に無い。2.1.296 で確かめた。変わったら読めない理由を返すので、送信したメッセージに出る。
  inboxToken: (pid, sessionsDir, files = realFiles) => {
    const name = files.list(sessionsDir).find((f) => f.startsWith(`${pid}.`) && f.endsWith('.key'));
    if (!name) throw new Error('このセッションの受信ソケットの鍵がありません');
    const { peerToken } = JSON.parse(files.read(path.win32.join(sessionsDir, name))) as { peerToken?: unknown };
    if (typeof peerToken !== 'string' || !peerToken) throw new Error('受信ソケットの鍵の形が変わっていて読めません');
    return peerToken;
  },
  // Windows でプロセス間の接続を listen できるのは、ファイルシステムではなく名前付きパイプの名前空間。
  localSocketPath: (dir, name) => `\\\\.\\pipe\\${path.win32.basename(dir)}-${name}`,
  tailnetHosts: tailnetHostsVia(['tailscale']),
};

export const platformFor = (name: string): Platform => (name === 'darwin' ? darwin : name === 'win32' ? win32 : other);

export const platform = platformFor(process.platform);

/** サーバーが動いている OS。改善要望に添えて、OS に限った不具合を調べられるようにする。 */
export const hostOs = (): HostOs => ({ platform: process.platform, version: os.version(), release: os.release(), arch: process.arch });

/**
 * claude の CLI を起動するコマンド。
 * テストは DURUN_DASHBOARD_FAKE_CLAUDE に node のスクリプトを指して、本物の代わりに動かす。
 * PATH に偽の実行ファイルを置く方法は、スクリプトをそのまま実行できない Windows では本物の claude を呼んでしまう。
 */
export const claudeCommand = (args: string[], env: NodeJS.ProcessEnv = process.env): { file: string; args: string[] } =>
  env.DURUN_DASHBOARD_FAKE_CLAUDE ? { file: process.execPath, args: [env.DURUN_DASHBOARD_FAKE_CLAUDE, ...args] } : { file: 'claude', args };
