// movie-maker を入れる口。開発用（ソースからビルドする）と配布用（ビルド済みを入れる）の 2 つの Installer を持ち、
// DURUN_DASHBOARD_DISTRIBUTION=1 のときは配布用を使う。どちらも同じ場所（<データの場所>/movie-maker/bin）に入れ、
// 入れたものを install.json に書くので、起動する側はどちらで入れたかを気にしない。
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { MmInstallMode, MmInstallView } from '../shared/types.ts';
import { type Platform, platform as hostPlatform } from './platform.ts';

/** 画面に見せる、手順の出力の行数。 */
const OUTPUT_LINES = 12;
const INSTALL_FILE = 'install.json';
const APP = 'movie-maker';
const CLI = 'mm';

/** 入れたものの記録。 */
export type InstallRecord = { mode: MmInstallMode; commit?: string; features: string[]; at: string };

/** 手順の 1 つ。line があればシェルで、無ければ file と args をそのまま動かす。 */
export type InstallStep = { label: string; cwd: string; file?: string; args?: string[]; line?: string; env?: Record<string, string> };

/** 開発用のビルドの手順と、できた物を入れる場所への写し方。 */
export type BuildPlan = { steps: InstallStep[]; copies: { from: string; to: string }[]; features: string[] };

/** repo はダッシュボードのある git リポジトリ、worktree はビルド専用の作業ツリー（origin/main に固定）、source はその中の movie-maker。 */
export type InstallPaths = { repo: string; worktree: string; source: string; target: string; bin: string };

/** ビルドの元にするブランチ。人間の作業中の変更を混ぜないよう、リモートの先端からビルドする。 */
const BASE_REF = 'origin/main';

/** 入れる場所と、ビルドに使う場所。target は環境変数で変えられる。 */
export const installPaths = (dataDir: string, env: NodeJS.ProcessEnv = process.env, os: Platform = hostPlatform): InstallPaths => {
  const worktree = path.join(dataDir, 'movie-maker', 'src');
  return {
    repo: path.join(import.meta.dirname, '..', '..'),
    worktree,
    source: path.join(worktree, 'movie-maker'),
    target: env.DURUN_MM_TARGET_DIR || os.movieMakerTargetDir(dataDir),
    bin: path.join(dataDir, 'movie-maker', 'bin'),
  };
};

export const installModeOf = (env: NodeJS.ProcessEnv = process.env): MmInstallMode => (env.DURUN_DASHBOARD_DISTRIBUTION === '1' ? 'release' : 'build');

/** ビルド専用の作業ツリーを origin/main の先端に合わせる手順。無ければ作る。 */
const sourceSteps = (paths: InstallPaths, exists: (p: string) => boolean): InstallStep[] => [
  { label: 'git fetch origin main', cwd: paths.repo, file: 'git', args: ['-C', paths.repo, 'fetch', 'origin', 'main'] },
  exists(paths.worktree)
    ? { label: `作業ツリーを ${BASE_REF} に合わせる`, cwd: paths.repo, file: 'git', args: ['-C', paths.worktree, 'checkout', '--detach', '--force', BASE_REF] }
    : { label: `作業ツリーを ${BASE_REF} で作る`, cwd: paths.repo, file: 'git', args: ['-C', paths.repo, 'worktree', 'add', '--detach', paths.worktree, BASE_REF] },
];

/**
 * ソースからビルドして入れる手順。UI（ui/dist）は Tauri が実行ファイルに埋め込むので、先に作る。
 * FFMPEG_DIR があれば FFmpeg ありで作り、実行ファイルの隣に FFmpeg のライブラリを写す（Windows は実行ファイルの隣の DLL を読む）。
 * release を一度だけ作るので、増分のキャッシュ（数 GB）は作らない。
 */
export const buildPlan = (paths: InstallPaths, env: NodeJS.ProcessEnv, os: Platform = hostPlatform, list: (dir: string) => string[] = safeList, exists: (p: string) => boolean = fs.existsSync): BuildPlan => {
  const ffmpeg = env.FFMPEG_DIR;
  const features = ffmpeg ? ['ffmpeg'] : [];
  const cargoArgs = ['build', '--release', '-p', APP, '-p', CLI, ...(ffmpeg ? ['--features', `${APP}/ffmpeg,${CLI}/ffmpeg`] : [])];
  const release = path.join(paths.target, 'release');
  const exes = [APP, CLI].map((name) => os.executableName(name));
  const libs = ffmpeg ? list(path.join(ffmpeg, 'bin')).filter((f) => f.endsWith(os.sharedLibraryExt)) : [];
  return {
    features,
    steps: [
      ...sourceSteps(paths, exists),
      { label: 'UI を作る（pnpm install、pnpm build）', cwd: path.join(paths.source, 'ui'), line: 'pnpm install --frozen-lockfile && pnpm build' },
      { label: `cargo ${cargoArgs.join(' ')}`, cwd: paths.source, file: 'cargo', args: cargoArgs, env: { CARGO_TARGET_DIR: paths.target, CARGO_INCREMENTAL: '0' } },
    ],
    copies: [...exes.map((exe) => ({ from: path.join(release, exe), to: path.join(paths.bin, exe) })), ...libs.map((lib) => ({ from: path.join(ffmpeg ?? '', 'bin', lib), to: path.join(paths.bin, lib) }))],
  };
};

function safeList(dir: string): string[] {
  try {
    return fs.readdirSync(dir);
  } catch {
    return [];
  }
}

export const readInstallRecord = (bin: string): InstallRecord | undefined => {
  try {
    return JSON.parse(fs.readFileSync(path.join(bin, INSTALL_FILE), 'utf8')) as InstallRecord;
  } catch {
    return undefined;
  }
};

/** 入れた movie-maker と mm の実行ファイル。入れていなければ undefined。 */
export const installedApp = (bin: string, os: Platform = hostPlatform): { app: string; cli: string } | undefined => {
  const app = path.join(bin, os.executableName(APP));
  const cli = path.join(bin, os.executableName(CLI));
  return fs.existsSync(app) && fs.existsSync(cli) ? { app, cli } : undefined;
};

/** 入れる口。status は入れたものの状態、install は入れる（入れ直す）。 */
export type Installer = {
  mode: MmInstallMode;
  view: () => MmInstallView;
  install: () => Promise<void>;
  /** 状態が変わったら呼ぶ。ウィンドウへ送り直すため。 */
  onChange: (fn: () => void) => void;
};

/** 手順を 1 つ動かし、出力を行ごとに渡す。失敗したら例外。 */
const runStep = (step: InstallStep, onLine: (line: string) => void, os: Platform): Promise<void> =>
  new Promise((resolve, reject) => {
    const shell = step.line ? os.shellCommand(step.line) : undefined;
    const file = shell?.file ?? step.file ?? '';
    const args = shell ? shell.args : (step.args ?? []);
    const child = spawn(file, typeof args === 'string' ? [args] : args, { cwd: step.cwd, env: { ...process.env, ...step.env }, windowsVerbatimArguments: typeof args === 'string', windowsHide: true });
    let rest = '';
    const feed = (chunk: Buffer): void => {
      const lines = (rest + chunk.toString()).split(/\r?\n/);
      rest = lines.pop() ?? '';
      lines.filter((l) => l.trim()).forEach(onLine);
    };
    child.stdout?.on('data', feed);
    child.stderr?.on('data', feed);
    child.on('error', (e) => reject(new Error(`${step.label} を始められません: ${e.message}`)));
    child.on('close', (code) => {
      if (rest.trim()) onLine(rest);
      if (code === 0) resolve();
      else reject(new Error(`${step.label} が終了コード ${code} で失敗しました`));
    });
  });

/** dir の git で ref が指すコミット。git が無いか、dir が git の中に無ければ undefined。 */
const commitOf = (dir: string, ref: string): Promise<string | undefined> =>
  new Promise((resolve) => {
    const child = spawn('git', ['-C', dir, 'rev-parse', '--short', ref], { windowsHide: true });
    let out = '';
    child.stdout.on('data', (c: Buffer) => (out += c.toString()));
    child.on('error', () => resolve(undefined));
    child.on('close', (code) => resolve(code === 0 ? out.trim() || undefined : undefined));
  });

/** 開発用。ソースからビルドして入れる。 */
export const buildInstaller = (paths: InstallPaths, env: NodeJS.ProcessEnv = process.env, os: Platform = hostPlatform): Installer => {
  const listeners: (() => void)[] = [];
  let busy = false;
  let output: string[] = [];
  let error: string | undefined;
  let head: string | undefined;
  const changed = (): void => listeners.forEach((fn) => fn());
  // 作り直しを勧めるかは、最後に fetch した origin/main と比べて決める。
  const refreshHead = (): void => void commitOf(paths.repo, BASE_REF).then((c) => c !== head && ((head = c), changed()));
  refreshHead();
  const line = (l: string): void => {
    output = [...output, l].slice(-OUTPUT_LINES);
    changed();
  };
  return {
    mode: 'build',
    onChange: (fn) => void listeners.push(fn),
    view: () => {
      const record = readInstallRecord(paths.bin);
      return { mode: 'build', path: installedApp(paths.bin, os)?.app, commit: record?.commit, sourceCommit: head, features: record?.features, busy, output, error };
    },
    install: async () => {
      if (busy) return;
      busy = true;
      output = [];
      error = undefined;
      changed();
      try {
        const plan = buildPlan(paths, env, os);
        for (const step of plan.steps) {
          line(`> ${step.label}`);
          await runStep(step, line, os);
        }
        fs.mkdirSync(paths.bin, { recursive: true });
        for (const c of plan.copies) fs.copyFileSync(c.from, c.to);
        const commit = await commitOf(paths.worktree, 'HEAD');
        const record: InstallRecord = { mode: 'build', commit, features: plan.features, at: new Date().toISOString() };
        fs.writeFileSync(path.join(paths.bin, INSTALL_FILE), `${JSON.stringify(record, null, 2)}\n`);
        head = commit;
        line(`入れました: ${paths.bin}`);
      } catch (e) {
        error = (e as Error).message;
      } finally {
        busy = false;
        changed();
      }
    },
  };
};

/**
 * 配布用。ビルド済みの movie-maker を落として入れる口。ダッシュボードを配布するときに中身を作る。
 * ponytail: GitHub Releases の zip（movie-maker.exe、mm.exe、FFmpeg の DLL、install.json）を落として bin に展開する想定で、まだリリースを作っていないので、今は入れられないと返す。
 */
export const releaseInstaller = (paths: InstallPaths, os: Platform = hostPlatform): Installer => {
  const record = (): InstallRecord | undefined => readInstallRecord(paths.bin);
  return {
    mode: 'release',
    onChange: () => undefined,
    view: () => ({ mode: 'release', path: installedApp(paths.bin, os)?.app, commit: record()?.commit, features: record()?.features, busy: false, output: [], error: 'ビルド済みの movie-maker の配布はまだ用意していません。' }),
    install: async () => undefined,
  };
};

export const installerFor = (dataDir: string, env: NodeJS.ProcessEnv = process.env): Installer => {
  const paths = installPaths(dataDir, env);
  return installModeOf(env) === 'release' ? releaseInstaller(paths) : buildInstaller(paths, env);
};
