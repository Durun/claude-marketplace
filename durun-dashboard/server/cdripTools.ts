// CD取り込みが使う外部コマンド（cyanrip、flac、ffmpeg など）の検出と導入。同梱せず、利用者がパッケージ管理で入れる。
import fs from 'node:fs';
import path from 'node:path';
import type { CdTool, CdToolName } from '../shared/types.ts';
import { cached, forgetCached } from './cache.ts';
import { type DirLookup, type Platform, platform as hostPlatform, type Run } from './platform.ts';
import { run as runOffThread } from './run.ts';

export const TOOL_NAMES: CdToolName[] = ['cyanrip', 'flac', 'metaflac', 'ffmpeg', 'ffprobe', 'yt-dlp'];

// ponytail: AccurateRip の全表は同梱しない。ここに無い機種は 0 で動かし、画面に警告を出す。
export const KNOWN_OFFSETS: Record<string, number> = { 'PIONEER BDR-UD04': 667 };

/** 機種名に、表のキーの単語がすべて含まれていれば、その読み取りオフセット。 */
export const knownOffset = (model: string): number | undefined => {
  const words = model.toUpperCase().split(/\s+/);
  const key = Object.keys(KNOWN_OFFSETS).find((k) => k.split(' ').every((w) => words.includes(w)));
  return key === undefined ? undefined : KNOWN_OFFSETS[key];
};

/** 環境変数で場所を指すときの名前。yt-dlp なら CDRIP_YT_DLP_PATH。 */
export const toolEnvName = (name: CdToolName): string => `CDRIP_${name.toUpperCase().replaceAll('-', '_')}_PATH`;

export type ResolveOptions = { env?: NodeJS.ProcessEnv; platform?: Platform; dirs?: DirLookup };

const realExists = (file: string): boolean => fs.existsSync(file);

/** 環境変数、PATH、パッケージ管理が置く場所の順に探す。見つからなければ undefined。 */
export const resolveTool = (name: CdToolName, { env = process.env, platform = hostPlatform, dirs }: ResolveOptions = {}): string | undefined => {
  const exists = dirs?.exists ?? realExists;
  const pinned = env[toolEnvName(name)];
  if (pinned && exists(pinned)) return pinned;
  const exe = platform.executableName(name);
  const onPath = (env.PATH ?? env.Path ?? '')
    .split(platform.name === 'win32' ? ';' : ':')
    .filter(Boolean)
    .map((dir) => (platform.name === 'win32' ? path.win32.join(dir, exe) : path.posix.join(dir, exe)))
    .find(exists);
  return onPath ?? platform.toolCandidates(name, env, dirs)[0];
};

const VERSION_ARGS: Record<CdToolName, string[]> = { cyanrip: ['-V'], flac: ['--version'], metaflac: ['--version'], ffmpeg: ['-version'], ffprobe: ['-version'], 'yt-dlp': ['--version'] };

/** 版の表示は、コマンドが出す最初の行にする。 */
export const firstLine = (text: string): string | undefined => text.split(/\r?\n/).find((l) => l.trim())?.trim();

const realRun: Run = async (file, args) => (await runOffThread(file, args, { timeout: 10_000 })).stdout;

// 版が読めなくても、場所が分かればツールは使える。
const versionOf = async (file: string, name: CdToolName, run: Run): Promise<string | undefined> => {
  try {
    return firstLine(await run(file, VERSION_ARGS[name]));
  } catch {
    return undefined;
  }
};

const KEY = (name: CdToolName): string => `cdrip-tool:v1:${name}`;

/** 検出の結果を 1 分覚える。画面が 3 秒ごとに状態を送るたびに、コマンドを起動しないため。手で入れても 1 分で見つかる。 */
const detectOne = async (name: CdToolName, platform: Platform, run: Run): Promise<CdTool> => {
  const found = await cached(KEY(name), { maxAge: 60_000 }, async () => {
    const file = resolveTool(name, { platform });
    return file ? { path: file, version: await versionOf(file, name, run) } : { path: undefined, version: undefined };
  });
  if (found.path) return { name, path: found.path, version: found.version };
  const { label, command, runnable } = platform.toolInstall(name);
  return { name, install: { label, command, runnable } };
};

/** 依存ツールの検出結果。yt-dlp は aiVideo のときだけ数える。 */
export const detectTools = (aiVideo: boolean, platform: Platform = hostPlatform, run: Run = realRun): Promise<CdTool[]> =>
  Promise.all(TOOL_NAMES.filter((n) => aiVideo || n !== 'yt-dlp').map((n) => detectOne(n, platform, run)));

/** 画面の導入ボタンから入れる。runnable でないツールは、コマンドを返すだけで実行しない。 */
export const installTool = async (name: CdToolName, platform: Platform = hostPlatform): Promise<{ message: string; ran: boolean }> => {
  const install = platform.toolInstall(name);
  if (!install.runnable) return { message: `管理者権限が要るので、端末で次を実行してください。\n${install.command}`, ran: false };
  try {
    const { stdout, stderr } = await runOffThread(install.file, install.args, { timeout: 10 * 60_000 });
    return { message: `${install.command}\n${(stdout || stderr).trim().split('\n').slice(-6).join('\n')}`, ran: true };
  } finally {
    forgetCached(/^cdrip-tool:/);
  }
};
