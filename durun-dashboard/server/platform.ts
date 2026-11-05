// OS ごとに違う情報の取り方と操作を、ここにまとめる互換レイヤー。
// サーバーのほかのコードは process.platform を見ずに、必ず platform を経由する。
// OS ごとの実装は platformFor で名前から選べ、外のコマンドは run で差し替えられるので、どの OS の上でも全部の実装をテストできる。
import type { ChildProcess } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { CdToolName, HostOs, MemoryStatus } from '../shared/types.ts';
import { toMemoryStatus, toMemoryUsage } from './memory.ts';
import { run as runOffThread } from './run.ts';

/** 外のコマンドを動かして標準出力を返す。テストでは偽物に差し替える。 */
export type Run = (file: string, args: string[]) => Promise<string>;

const realRun: Run = async (file, args) => (await runOffThread(file, args)).stdout;

/** FL Studio の、開いているプロジェクトを当てる手がかり。title は FL のウィンドウの題名、recent は FL が覚えている最近のファイル（新しい順）。 */
export type FlProjectHints = { title: string; recent: string[] };

/** PowerShell が JSON で書いた手がかりを読む。最近のファイルが 1 つでも、空でも、配列にそろえる。 */
export const parseFlProjectHints = (json: string): FlProjectHints => {
  const v = JSON.parse(json) as { title?: unknown; recent?: unknown };
  const recent = Array.isArray(v.recent) ? v.recent : v.recent === undefined || v.recent === null ? [] : [v.recent];
  return { title: typeof v.title === 'string' ? v.title : '', recent: recent.filter((f): f is string => typeof f === 'string' && f !== '') };
};

/** ディレクトリの一覧とファイルの中身を読む。テストでは偽物に差し替える。 */
export type Files = { list: (dir: string) => string[]; read: (file: string) => string };

const realFiles: Files = { list: (dir) => fs.readdirSync(dir), read: (file) => fs.readFileSync(file, 'utf8') };

/** 光学ドライブ 1 台。id は設定の drive に書く値（Windows ではドライブ文字、macOS では drutil の番号、Linux ではデバイスのパス）。 */
export type OpticalDrive = { id: string; model: string };

/** 依存ツールを入れるコマンド。runnable が偽なら、管理者権限が要るので実行せず、見せるだけにする。 */
export type ToolInstall = { label: string; command: string; file: string; args: string[]; runnable: boolean };

/** ファイルの有無とディレクトリの一覧。テストでは偽物に差し替える。 */
export type DirLookup = { exists: (file: string) => boolean; list: (dir: string) => string[] };

const realDirs: DirLookup = {
  exists: fs.existsSync,
  list: (dir) => {
    try {
      return fs.readdirSync(dir);
    } catch {
      return [];
    }
  },
};

/** PowerShell が JSON で書いた光学ドライブを読む。1 台だと配列にならないので、配列にそろえる。 */
export const parseWindowsDrives = (json: string): OpticalDrive[] => {
  if (!json.trim()) return [];
  const v = JSON.parse(json) as unknown;
  const rows = (Array.isArray(v) ? v : [v]) as { Drive?: unknown; Name?: unknown }[];
  return rows.flatMap((r) => (typeof r?.Drive === 'string' && r.Drive ? [{ id: r.Drive, model: String(r.Name ?? '').replace(/\s+/g, ' ').trim() }] : []));
};

/** drutil list の表から、番号と機種を読む。 */
export const parseDrutilList = (text: string): OpticalDrive[] =>
  text.split('\n').flatMap((line) => {
    const m = /^\s*(\d+)\s+(\S.*?)\s{2,}\S+\s+\S+\s+\S+\s*$/.exec(line);
    return m ? [{ id: m[1] ?? '', model: (m[2] ?? '').replace(/\s+/g, ' ').trim() }] : [];
  });

/** lsblk -J の出力から、光学ドライブを読む。 */
export const parseLsblk = (json: string): OpticalDrive[] => {
  const v = JSON.parse(json) as { blockdevices?: { name?: string; type?: string; model?: string | null }[] };
  return (v.blockdevices ?? []).flatMap((d) => (d.type === 'rom' && d.name ? [{ id: `/dev/${d.name}`, model: (d.model ?? '').trim() }] : []));
};

/** drutil status の表から、ディスクが入っているかを読む。Type: の行が No Media Inserted でなければ入っている。 */
export const drutilMediaLoaded = (status: string): boolean => {
  const type = /^\s*Type:\s*(.*)$/m.exec(status)?.[1]?.trim();
  return !!type && !/no media/i.test(type);
};

// metaflac は flac のパッケージに、ffprobe は ffmpeg のパッケージに入っている。
const WINGET_IDS: Record<CdToolName, string> = { cyanrip: 'cyanreg.cyanrip', flac: 'Xiph.FLAC', metaflac: 'Xiph.FLAC', ffmpeg: 'Gyan.FFmpeg', ffprobe: 'Gyan.FFmpeg', 'yt-dlp': 'yt-dlp.yt-dlp' };
const PACKAGE_NAMES: Record<CdToolName, string> = { cyanrip: 'cyanrip', flac: 'flac', metaflac: 'flac', ffmpeg: 'ffmpeg', ffprobe: 'ffmpeg', 'yt-dlp': 'yt-dlp' };

/** 名前に * を含む階層を、実在するものに広げる。新しい版を先に返す。 */
const expandWild = (base: string, segments: string[], dirs: DirLookup, join: (...p: string[]) => string): string[] => {
  const [head, ...rest] = segments;
  if (head === undefined) return dirs.exists(base) ? [base] : [];
  if (!head.includes('*')) return expandWild(join(base, head), rest, dirs, join);
  const pattern = new RegExp(`^${head.split('*').map((p) => p.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('.*')}$`, 'i');
  return dirs
    .list(base)
    .filter((n) => pattern.test(n))
    .sort()
    .reverse()
    .flatMap((n) => expandWild(join(base, n), rest, dirs, join));
};

// winget のパッケージの中の置き場所。版や配布元の名前が付く階層は * で書く。
const WINGET_LAYOUT: Record<CdToolName, string[][]> = {
  cyanrip: [['cyanreg.cyanrip_*', 'cyanrip.exe']],
  flac: [['Xiph.FLAC_*', 'flac-*-win', 'Win64', 'flac.exe']],
  metaflac: [['Xiph.FLAC_*', 'flac-*-win', 'Win64', 'metaflac.exe']],
  ffmpeg: [['Gyan.FFmpeg_*', 'ffmpeg-*', 'bin', 'ffmpeg.exe'], ['yt-dlp.FFmpeg_*', 'ffmpeg-*', 'bin', 'ffmpeg.exe']],
  ffprobe: [['Gyan.FFmpeg_*', 'ffmpeg-*', 'bin', 'ffprobe.exe'], ['yt-dlp.FFmpeg_*', 'ffmpeg-*', 'bin', 'ffprobe.exe']],
  'yt-dlp': [['yt-dlp.yt-dlp_*', 'yt-dlp.exe']],
};

export type Platform = {
  name: 'darwin' | 'win32' | 'other';
  /** 資料の書き出しに使う Chrome の、標準の場所。分からない OS では undefined で、CHROME_PATH で指してもらう。 */
  defaultChromePath: (env: NodeJS.ProcessEnv) => string | undefined;
  /** Claude Code がシェル（Windows では Git Bash）で動かすコマンドに書くパス。 */
  shellPath: (file: string) => string;
  /** メモリプレッシャー（Windows では使用率）と内訳。読めない OS では null を返し、画面は表示を出さない。 */
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
  /**
   * バックグラウンドのセッションを動かす supervisor の制御ソケット。claudeDir は Claude Code の設定の場所。読めなければ例外を投げる。
   * ponytail: この場所は Claude Code の文書に無い。2.1.301 で確かめた。変わったら接続できない理由が、送信したメッセージに出る。
   */
  supervisorSocket: (claudeDir: string, files?: Files) => string;
  /** dir の下に置く、このプロセスが listen できるローカルのソケットの場所。 */
  localSocketPath: (dir: string, name: string) => string;
  /** この端末を tailnet の中から指す名前と IP。Tailscale が入っていないか、つながっていなければ空。 */
  tailnetHosts: (run?: Run) => Promise<string[]>;
  /** 画面に渡すパス。画面がどの OS でも / で分けられるよう、区切りを / にそろえる。 */
  displayPath: (file: string) => string;
  /** エクスプローラーの最上位に並べる場所。Windows ではつながっているドライブで、ほかの OS では /。exists はテストで偽物に差し替える。 */
  fileRoots: (exists?: (dir: string) => boolean) => string[];
  /** 人間が設定に書いたコマンドの行を、シェルで動かすときの起動するファイルと引数。node-pty に渡す。 */
  shellCommand: (line: string) => { file: string; args: string[] | string };
  /** シェルで動かしたコマンドを、シェルが起動したプロセスごと止める。止め終わるのは待たない。run はテストで偽物に差し替える。 */
  killTree: (child: ChildProcess, run?: Run) => void;
  /** 直近 1 分の load average。測れない OS では undefined。 */
  loadAverage: () => number | undefined;
  /** ドキュメントのフォルダー。Windows では、OneDrive に移したときも追えるよう、既知のフォルダーから引く。読めなければ undefined。 */
  documentsDir: (run?: Run) => Promise<string | undefined>;
  /**
   * FL Studio のユーザーデータのフォルダーの候補。FL はこの場所をどこに覚えているかを公開していないので、よくある場所を並べる。
   * ドキュメントの下の Image-Line/FL Studio と、Windows では各ドライブの直下の Image-Line/FL Studio。exists はテストで偽物に差し替える。
   */
  flUserDataCandidates: (documents: string | undefined, exists?: (dir: string) => boolean) => string[];
  /** FL Studio 2026 の実行ファイル。FL_STUDIO_PATH で指せる。FL の中の口（Gopher の WebView2）を使えない OS では undefined。 */
  flStudioPath: (env: NodeJS.ProcessEnv) => string | undefined;
  /** FL Studio が動いているか。調べられない OS では undefined。 */
  isFlRunning: (run?: Run) => Promise<boolean | undefined>;
  /** FL Connect の MIDI のループバック（FL Connect (A) と (B)）を作り、設定に保存するコマンド。Windows MIDI Services の無い OS では undefined。 */
  midiLoopbackCommand: (env: NodeJS.ProcessEnv) => { file: string; args: string[] } | undefined;
  /**
   * 開いているプロジェクトを当てる手がかり。FL の API はプロジェクトのファイルの場所を返さないので、FL のウィンドウの題名と、FL が覚えている最近のファイルを読む。
   * major は FL の版の上の桁（FL 2026 なら 26）。読めない OS と、読めなかったときは undefined。
   */
  flProjectHints: (major: number, run?: Run) => Promise<FlProjectHints | undefined>;
  /** Claude Code の認証情報の JSON。/mcp で接続した MCP サーバーの OAuth のトークンを mcpOAuth に持つ。読めなければ undefined。 */
  claudeCredentials: (run?: Run, files?: Files) => Promise<string | undefined>;
  /** Slack のデスクトップアプリが覚えている状態の JSON。ログインしたワークスペースの ID とドメインを workspaces に持つ。読めなければ undefined。 */
  slackAppState: (files?: Files, env?: NodeJS.ProcessEnv) => string | undefined;
  /** つながっている光学ドライブ。調べられなければ空。 */
  opticalDrives: (run?: Run) => Promise<OpticalDrive[]>;
  /** ドライブにディスクが入っているか。調べられなければ undefined。 */
  mediaLoaded: (drive: string, run?: Run) => Promise<boolean | undefined>;
  /** ディスクを排出する。排出できなくても投げない（人間が手で出せる）。 */
  ejectDisc: (drive: string, run?: Run) => Promise<void>;
  /** cyanrip の -d に渡す、ドライブのデバイス。 */
  cdDevice: (drive: string, run?: Run) => Promise<string>;
  /** 音楽フォルダー。Windows では、OneDrive に移したときも追えるよう、既知のフォルダーから引く。 */
  musicDir: (run?: Run) => Promise<string>;
  /** 実行ファイルの名前（Windows では .exe を付ける）。 */
  executableName: (tool: string) => string;
  /** パッケージ管理が置く場所にある依存ツールの候補。新しい版を先に返す。 */
  toolCandidates: (tool: CdToolName, env: NodeJS.ProcessEnv, dirs?: DirLookup) => string[];
  /** 依存ツールを入れるコマンド。 */
  toolInstall: (tool: CdToolName) => ToolInstall;
};

const readOrUndefined = (files: Files, file: string): string | undefined => {
  try {
    return files.read(file);
  } catch {
    return undefined;
  }
};

// Mac 以外の Claude Code は、認証情報をキーチェーンでなくこのファイルに置く。
const credentialsFile = (files: Files, dir: string): Promise<string | undefined> => {
  try {
    return Promise.resolve(files.read(path.join(dir, '.claude', '.credentials.json')));
  } catch {
    return Promise.resolve(undefined);
  }
};

const FL_USER_DATA = path.join('Image-Line', 'FL Studio');

/** tasklist の出力に、image の行があるか。見つからないときも、tasklist は終了コード 0 で知らせの文を出す。 */
export const listedInTasklist = (tasklist: string, image: string): boolean => tasklist.split('\n').some((line) => line.trim().toLowerCase().startsWith(image.toLowerCase()));

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

const brewInstall = (tool: CdToolName): ToolInstall => ({ label: `brew install ${PACKAGE_NAMES[tool]}`, command: `brew install ${PACKAGE_NAMES[tool]}`, file: 'brew', args: ['install', PACKAGE_NAMES[tool]], runnable: true });

const posix = {
  shellPath: (file: string) => file,
  // sh の $PPID が claude の PID になる。
  headersHelperSendsPid: true,
  connectionOwner: async () => undefined,
  // Unix ソケットは OS のユーザーだけが開けるので、Claude Code は認証の行を求めない。
  inboxToken: () => undefined,
  // Claude Code は、ユーザーごとの /tmp の下を、設定の場所のハッシュで分ける。
  supervisorSocket: (claudeDir: string) =>
    path.posix.join('/tmp', `cc-daemon-${process.getuid?.() ?? 0}`, createHash('sha256').update(path.posix.resolve(claudeDir)).digest('hex').slice(0, 8), 'control.sock'),
  localSocketPath: (dir: string, name: string) => path.posix.join(dir, `${name}.sock`),
  tailnetHosts: tailnetHostsVia(['tailscale']),
  displayPath: (file: string) => file,
  fileRoots: () => ['/'],
  shellCommand: (line: string) => ({ file: '/bin/sh', args: ['-c', line] }),
  // sh は -c の 1 つのコマンドを自分と入れ替えて動かすので、sh を止めればコマンドも止まる。
  killTree: (child: ChildProcess) => void child.kill(),
  loadAverage: () => os.loadavg()[0],
  documentsDir: async () => path.posix.join(os.homedir(), 'Documents'),
  flUserDataCandidates: (documents: string | undefined) => (documents ? [path.posix.join(documents, 'Image-Line', 'FL Studio')] : []),
  // FL の Mac 版は WebView2 を使わないので、デバッグのポートを開いて起動する道が無い。
  flStudioPath: () => undefined,
  isFlRunning: async () => undefined,
  midiLoopbackCommand: () => undefined,
  // ponytail: Mac 版の FL が最近のファイルをどこに覚えているかは調べていない。広げるなら、FL の設定の plist を読む。
  flProjectHints: async () => undefined,
  claudeCredentials: (_run: Run = realRun, files: Files = realFiles) => credentialsFile(files, os.homedir()),
  slackAppState: (files: Files = realFiles) => readOrUndefined(files, path.posix.join(os.homedir(), '.config', 'Slack', 'storage', 'root-state.json')),
  musicDir: async () => path.posix.join(os.homedir(), 'Music'),
  executableName: (tool: string) => tool,
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
  slackAppState: (files = realFiles) => readOrUndefined(files, path.posix.join(os.homedir(), 'Library', 'Application Support', 'Slack', 'storage', 'root-state.json')),
  claudeCredentials: async (run = realRun) => {
    try {
      return await run('/usr/bin/security', ['find-generic-password', '-s', 'Claude Code-credentials', '-w']);
    } catch {
      return undefined;
    }
  },
  opticalDrives: async (run = realRun) => {
    try {
      return parseDrutilList(await run('drutil', ['list']));
    } catch {
      return [];
    }
  },
  mediaLoaded: async (drive, run = realRun) => {
    try {
      return drutilMediaLoaded(await run('drutil', ['-drive', drive, 'status']));
    } catch {
      return undefined;
    }
  },
  ejectDisc: async (drive, run = realRun) => {
    await run('drutil', ['-drive', drive, 'eject']).catch(() => undefined);
  },
  // ponytail: macOS のデバイスのパスは、ディスクが入っているときの drutil status の Name: からしか分からない。設定の drive に /dev/diskN と書けば、それを使う。
  cdDevice: async (drive, run = realRun) => {
    if (drive.startsWith('/dev/')) return drive;
    const status = await run('drutil', ['-drive', drive, 'status']).catch(() => '');
    return /^\s*Name:\s*(\S+)/m.exec(status)?.[1] ?? drive;
  },
  toolCandidates: (tool, _env, dirs = realDirs) => ['/opt/homebrew/bin', '/usr/local/bin'].map((d) => path.posix.join(d, tool)).filter((f) => dirs.exists(f)),
  toolInstall: brewInstall,
};

// ponytail: Linux ではメモリを読まない。広げるなら /proc/pressure/memory と /proc/meminfo から読む。
const other: Platform = {
  ...posix,
  name: 'other',
  defaultChromePath: () => undefined,
  readMemory: async () => null,
  opticalDrives: async (run = realRun) => {
    try {
      return parseLsblk(await run('lsblk', ['-J', '-o', 'NAME,TYPE,MODEL']));
    } catch {
      return [];
    }
  },
  mediaLoaded: async (drive, run = realRun) => {
    try {
      return /^E: ID_CDROM_MEDIA=1$/m.test(await run('udevadm', ['info', drive]));
    } catch {
      return undefined;
    }
  },
  ejectDisc: async (drive, run = realRun) => {
    await run('eject', [drive]).catch(() => undefined);
  },
  cdDevice: async (drive) => drive,
  toolCandidates: (tool, _env, dirs = realDirs) => ['/usr/bin', '/usr/local/bin'].map((d) => path.posix.join(d, tool)).filter((f) => dirs.exists(f)),
  // apt は管理者権限が要るので、画面からは実行せず、コマンドを見せる。
  toolInstall: (tool) => ({ label: `sudo apt install ${PACKAGE_NAMES[tool]}`, command: `sudo apt install ${PACKAGE_NAMES[tool]}`, file: 'sudo', args: ['apt', 'install', PACKAGE_NAMES[tool]], runnable: false }),
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

const DRIVE_LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');

const win32: Platform = {
  name: 'win32',
  defaultChromePath: (env) => path.win32.join(env.PROGRAMFILES ?? 'C:\\Program Files', 'Google', 'Chrome', 'Application', 'chrome.exe'),
  // Git Bash は C:/… の形のパスを読める。\ のままだと、引用符の外でエスケープとして消える。
  shellPath: (file) => file.replaceAll('\\', '/'),
  // Node の os.freemem は、Windows ではタスクマネージャーの「利用可能」と同じ値を返す。
  readMemory: async () => toMemoryUsage(os.totalmem(), os.freemem()),
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
  // 名前付きパイプの名前は、Claude Code が設定の場所に置く pipe.key の値で分ける。
  supervisorSocket: (claudeDir, files = realFiles) => {
    const key = files.read(path.win32.join(claudeDir, 'daemon', 'pipe.key')).trim();
    if (!/^[a-f0-9]{16}$/.test(key)) throw new Error('supervisor の pipe.key の形が変わっていて読めません');
    return `\\\\.\\pipe\\cc-daemon-${key}-control`;
  },
  // Windows でプロセス間の接続を listen できるのは、ファイルシステムではなく名前付きパイプの名前空間。
  localSocketPath: (dir, name) => `\\\\.\\pipe\\${path.win32.basename(dir)}-${name}`,
  tailnetHosts: tailnetHostsVia(['tailscale']),
  displayPath: (file) => file.replaceAll('\\', '/'),
  fileRoots: (exists = fs.existsSync) => DRIVE_LETTERS.filter((d) => exists(`${d}:\\`)).map((d) => `${d}:/`),
  // node-pty は配列の引数を引用符でくくり直し、行の中の引用符を崩すので、cmd.exe の引数を 1 つの文字列で渡す。
  // /s を付けた cmd.exe は最初と最後の引用符だけを外し、間の行をそのまま動かす。Node の shell: true と同じ渡し方。
  shellCommand: (line) => ({ file: 'cmd.exe', args: `/d /s /c "${line}"` }),
  // cmd.exe を止めても、cmd.exe が起動したコマンド（gcloud.cmd が起動する Python など）は残るので、taskkill の /T で木ごと止める。
  // もう終わっていれば taskkill は失敗するが、止める物が無いだけなので構わない。
  killTree: (child, run = realRun) => {
    if (child.pid !== undefined) run('taskkill', ['/pid', String(child.pid), '/T', '/F']).catch(() => undefined);
  },
  // Node の os.loadavg は、Windows では負荷によらず 0 を返す。
  loadAverage: () => undefined,
  // reg.exe はコンソールのコードページで書くので、日本語のパスが化ける。PowerShell に UTF-8 で書かせる。
  documentsDir: async (run = realRun) => {
    try {
      const out = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', "[Console]::OutputEncoding = [Text.Encoding]::UTF8; [Environment]::GetFolderPath('MyDocuments')"]);
      return out.trim() || undefined;
    } catch {
      return undefined;
    }
  },
  flUserDataCandidates: (documents, exists = fs.existsSync) => [
    ...(documents ? [path.win32.join(documents, FL_USER_DATA)] : []),
    ...DRIVE_LETTERS.filter((d) => exists(`${d}:\\`)).map((d) => path.win32.join(`${d}:\\`, FL_USER_DATA)),
  ],
  flStudioPath: (env) => env.FL_STUDIO_PATH || path.win32.join(env.PROGRAMFILES ?? 'C:\\Program Files', 'Image-Line', 'FL Studio 2026', 'FL64.exe'),
  isFlRunning: async (run = realRun) => listedInTasklist(await run('tasklist', ['/FI', 'IMAGENAME eq FL64.exe', '/NH']), 'FL64.exe'),
  // Windows MIDI Services の道具は Program Files に入る。識別子は英字と数字だけでないと、設定に保存されない。
  midiLoopbackCommand: (env) => ({
    file: path.win32.join(env.PROGRAMFILES ?? 'C:\\Program Files', 'Windows MIDI Services', 'Tools', 'midi.exe'),
    args: ['loopback', 'create', '--root-name', 'FL Connect', '--unique-identifier', 'flconnect', '--save-to-config'],
  }),
  // FL は最近のファイルを、レジストリの HKCU\Software\Image-Line\FL Studio <版>\MRU に、0、1、… の名前で新しい順に覚えている。
  // reg.exe はコンソールのコードページで書くので日本語のパスが化ける。PowerShell に UTF-8 の JSON で書かせる。
  flProjectHints: async (major, run = realRun) => {
    if (!Number.isInteger(major)) return undefined;
    const script = [
      '[Console]::OutputEncoding = [Text.Encoding]::UTF8',
      "$p = Get-Process -Name 'FL64*' -ErrorAction SilentlyContinue | Select-Object -First 1",
      `$k = Get-ItemProperty 'HKCU:\\Software\\Image-Line\\FL Studio ${major}\\MRU' -ErrorAction SilentlyContinue`,
      '$r = @()',
      'if ($k) { for ($i = 0; $k.PSObject.Properties[[string]$i]; $i++) { $r += [string]$k.([string]$i) } }',
      "ConvertTo-Json -Compress @{ title = if ($p) { $p.MainWindowTitle } else { '' }; recent = $r }",
    ].join('; ');
    try {
      return parseFlProjectHints(await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script]));
    } catch {
      return undefined;
    }
  },
  claudeCredentials: (_run = realRun, files = realFiles) => credentialsFile(files, os.homedir()),
  slackAppState: (files = realFiles, env = process.env) => (env.APPDATA ? readOrUndefined(files, path.win32.join(env.APPDATA, 'Slack', 'storage', 'root-state.json')) : undefined),
  // 機種名の日本語が化けないよう、PowerShell に UTF-8 の JSON で書かせる。
  opticalDrives: async (run = realRun) => {
    try {
      return parseWindowsDrives(await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', '[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-CimInstance Win32_CDROMDrive | Select-Object Drive,Name | ConvertTo-Json -Compress']));
    } catch {
      return [];
    }
  },
  mediaLoaded: async (drive, run = realRun) => {
    try {
      const out = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `(Get-CimInstance Win32_CDROMDrive -Filter "Drive='${drive}'").MediaLoaded`]);
      return out.trim() === 'True';
    } catch {
      return undefined;
    }
  },
  ejectDisc: async (drive, run = realRun) => {
    await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `(New-Object -ComObject Shell.Application).Namespace(17).ParseName('${drive}').InvokeVerb('Eject')`]).catch(() => undefined);
  },
  // cyanrip は Windows では F: の形のドライブ文字を受ける。
  cdDevice: async (drive) => drive,
  musicDir: async (run = realRun) => {
    const fallback = path.win32.join(os.homedir(), 'Music');
    try {
      const out = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', "[Console]::OutputEncoding = [Text.Encoding]::UTF8; [Environment]::GetFolderPath('MyMusic')"]);
      return out.trim() || fallback;
    } catch {
      return fallback;
    }
  },
  executableName: (tool) => `${tool}.exe`,
  toolCandidates: (tool, env, dirs = realDirs) => {
    const base = path.win32.join(env.LOCALAPPDATA ?? path.win32.join(os.homedir(), 'AppData', 'Local'), 'Microsoft', 'WinGet', 'Packages');
    return WINGET_LAYOUT[tool].flatMap((segments) => expandWild(base, segments, dirs, path.win32.join));
  },
  toolInstall: (tool) => {
    const args = ['install', '--id', WINGET_IDS[tool], '-e', '--accept-source-agreements', '--accept-package-agreements'];
    return { label: `winget install ${WINGET_IDS[tool]}`, command: `winget ${args.join(' ')}`, file: 'winget', args, runnable: true };
  },
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
