// ダッシュボードが画面の補助に使う Claude を、claude -p として動かす。ふだんは軽い haiku を使い、コードを読んで確かめるときと、文章校正で文章を読むときは sonnet を使う。文章校正の修正版は opus に書かせる。
// 使用制限は、モデルを呼ばずに、SDK の get_usage で読む。
import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import type { RateLimit, RateLimits, TokenTotals } from '../shared/types.ts';
import { claudeCommand } from './platform.ts';

const HAIKU_TIMEOUT_MS = 30_000;
// 長い文章を読ませると、30 秒を超えることがある。
const SONNET_TIMEOUT_MS = 2 * 60_000;
// コードを読みながら答えるので、数分かかることがある。
const READER_TIMEOUT_MS = 10 * 60_000;
// 長い文章では書き直す文が多く、数分かかることがある。
const OPUS_TIMEOUT_MS = 10 * 60_000;
const USAGE_TIMEOUT_MS = 30_000;

const COMMON_ARGS = ['-p', '--no-session-persistence', '--output-format', 'json'];
// フック、MCP、CLAUDE.md を外す。フックが動くと、このセッションもダッシュボードに記録される。
const SAFE_MODE = '--safe-mode';

// 数秒で返るよう、ツールと思考も外す。
const HAIKU_ARGS = [
  SAFE_MODE, '--model', 'haiku', '--tools', '',
  '--settings', JSON.stringify({ alwaysThinkingEnabled: false }),
  '--system-prompt', '人間が複数の Claude Code のセッションを見渡すダッシュボード（durun-dashboard）の中で、頼まれた文を作る。',
];

// 文章を読んで判定させる。haiku より確かな sonnet を、ツールを持たせずに動かす。前提知識を持たない読み手にするため、ダッシュボードの説明も渡さない。
const SONNET_ARGS = [
  SAFE_MODE, '--model', 'sonnet', '--tools', '',
  '--settings', JSON.stringify({ alwaysThinkingEnabled: false }),
  '--system-prompt', '頼まれた文章を読み、求められた形で判定する。',
];

// 指摘に沿って文章を書き直させる。直し方を考えさせるため、sonnet より確かな opus を、思考を外さずに動かす。時間と消費を抑えるよう、effort は medium にする。
const OPUS_ARGS = ['--model', 'opus', '--effort', 'medium', '--system-prompt', '頼まれた文章を、求められた形で書き直す。'];

// safe mode はプラグインのスキルも外す。スキルを使わせるときは、代わりに設定ファイルと MCP を読まずに、そのプラグインだけを足す。設定ファイルを読まなければ、フックも CLAUDE.md も外れる。
// スキルが参照するファイルを読めるよう、Read だけを渡し、プラグインの場所で動かす。ほかの場所のファイルを読むなど、確認の要る操作は断る。
const pluginArgs = (dir: string): string[] => ['--setting-sources', '', '--strict-mcp-config', '--plugin-dir', dir, '--tools', 'Read', '--permission-prompts', 'none'];

// 事実を確かめるにはコードを読んで推論する必要があるため、haiku より確かな sonnet に、読むだけのツールを渡す。確認の要る操作は断る。
// 作業ディレクトリと Read が求める絶対パスが分かるよう、Claude Code の既定のシステムプロンプトのまま動かす。
const READER_ARGS = [SAFE_MODE, '--model', 'sonnet', '--tools', 'Read,Grep,Glob', '--permission-prompts', 'none'];

/** claude の失敗や、求めた形でない答え。ダッシュボードの不具合ではないので、改善要望にはしない。 */
export class UpstreamError extends Error {}

/** 動いている補助のセッション id。実行中だけセッションファイルが現れるので、一覧に記録しないよう見分ける。 */
const running = new Set<string>();

export const isHelperSession = (sessionId: string): boolean => running.has(sessionId);

/** 問い合わせ 1 回で使ったトークン数を受け取る。 */
export type OnUsage = (usage: TokenTotals) => void;

/** claude -p と Jev が返す usage。Jev はキャッシュの項目を返さない。 */
export type RawUsage = { input_tokens?: number; output_tokens?: number; cache_read_input_tokens?: number; cache_creation_input_tokens?: number };

export const tokensOf = (u: RawUsage): TokenTotals => ({
  input: u.input_tokens ?? 0,
  output: u.output_tokens ?? 0,
  cacheRead: u.cache_read_input_tokens ?? 0,
  cacheCreation: u.cache_creation_input_tokens ?? 0,
});

/** すべての項目を必須にした object の JSON Schema。 */
export const objectSchema = (properties: Record<string, object>): object => ({
  type: 'object',
  properties,
  required: Object.keys(properties),
  additionalProperties: false,
});

/** JSON Schema に沿った答えを返す。 */
const askClaude = (prompt: string, schema: object, args: string[], timeout: number, cwd?: string, onUsage?: OnUsage): Promise<Record<string, unknown>> =>
  new Promise((resolve, reject) => {
    const id = randomUUID();
    running.add(id);
    const all = [...COMMON_ARGS, ...args, '--json-schema', JSON.stringify(schema), '--session-id', id];
    const claude = claudeCommand(all);
    const child = execFile(claude.file, claude.args, { timeout, cwd }, (err, stdout, stderr) => {
      running.delete(id);
      if (err?.killed) return reject(new UpstreamError(`${timeout / 1000} 秒以内に終わりませんでした`));
      try {
        const out = JSON.parse(stdout) as { result?: string; structured_output?: Record<string, unknown>; usage?: RawUsage };
        // 求めた形で答えなくても、トークンは使っている。
        if (out.usage) onUsage?.(tokensOf(out.usage));
        if (!out.structured_output) throw new UpstreamError(out.result || '答えが返りませんでした');
        resolve(out.structured_output);
      } catch (e) {
        // 出力が JSON でなければ、claude は理由を標準エラーに書いている。
        reject(e instanceof UpstreamError ? e : new UpstreamError(stderr.trim() || err?.message || String(e)));
      }
    });
    // プロンプトは標準入力で渡す。閉じないと、claude は入力の続きを 3 秒待つ。
    child.stdin?.end(prompt);
  });

export const askHaiku = (prompt: string, schema: object, onUsage?: OnUsage): Promise<Record<string, unknown>> =>
  askClaude(prompt, schema, HAIKU_ARGS, HAIKU_TIMEOUT_MS, undefined, onUsage);

export const askSonnet = (prompt: string, schema: object, onUsage?: OnUsage): Promise<Record<string, unknown>> =>
  askClaude(prompt, schema, SONNET_ARGS, SONNET_TIMEOUT_MS, undefined, onUsage);

/** plugin にプラグインの場所を渡すと、そのプラグインのスキルを使えるようにする。 */
export const askOpus = (prompt: string, schema: object, plugin?: string): Promise<Record<string, unknown>> =>
  plugin
    ? askClaude(prompt, schema, [...pluginArgs(plugin), ...OPUS_ARGS], OPUS_TIMEOUT_MS, plugin)
    : askClaude(prompt, schema, [SAFE_MODE, '--tools', '', ...OPUS_ARGS], OPUS_TIMEOUT_MS);

/** cwd のコードを読んで答えさせる。 */
export const askReader = (cwd: string, prompt: string, schema: object): Promise<Record<string, unknown>> =>
  askClaude(prompt, schema, READER_ARGS, READER_TIMEOUT_MS, cwd);

/** get_usage が返す、使用制限の 1 つの枠。枠の無いプランでは null。 */
type RawWindow = { utilization?: number | null; resets_at?: string | null } | null;
/** get_usage の答え。API キーなど、プランの使用制限が無いログインでは rate_limits が null。 */
type UsageReply = { rate_limits?: { five_hour?: RawWindow; seven_day?: RawWindow } | null };
type ControlResponse = { type?: string; response?: { subtype?: string; request_id?: string; error?: string; response?: UsageReply } };

const USAGE_REQUEST_ID = 'usage';

// リセットの時刻は、読むたびに秒より下の端数が変わる。同じ枠を同じ時刻で見分けられるよう、分に丸める。
const resetOf = (at: string | null | undefined): number | undefined => {
  const ms = Date.parse(at ?? '');
  return Number.isNaN(ms) ? undefined : Math.round(ms / 60_000) * 60_000;
};

const limitOf = (w: RawWindow | undefined): RateLimit | undefined =>
  w?.utilization == null ? undefined : { usedPercent: w.utilization, resetsAt: resetOf(w.resets_at) };

/** get_usage の答えから、5 時間と 7 日の枠を読む。at は読んだ時刻。 */
export const rateLimitsOf = (reply: UsageReply, at: number): RateLimits => ({
  fiveHour: limitOf(reply.rate_limits?.five_hour),
  sevenDay: limitOf(reply.rate_limits?.seven_day),
  updatedAt: at,
});

const parseLine = (line: string): ControlResponse | undefined => {
  try {
    return JSON.parse(line) as ControlResponse;
  } catch {
    return undefined;
  }
};

/** ログインしているアカウントの使用制限を、claude -p に SDK の get_usage を送って読む。 */
export const askRateLimits = (): Promise<RateLimits> =>
  new Promise((resolve, reject) => {
    const id = randomUUID();
    running.add(id);
    const args = ['-p', SAFE_MODE, '--no-session-persistence', '--input-format', 'stream-json', '--output-format', 'stream-json', '--verbose', '--session-id', id];
    const claude = claudeCommand(args);
    const child = execFile(claude.file, claude.args, { timeout: USAGE_TIMEOUT_MS }, (err, stdout, stderr) => {
      running.delete(id);
      if (err?.killed) return reject(new UpstreamError(`${USAGE_TIMEOUT_MS / 1000} 秒以内に終わりませんでした`));
      const reply = stdout.split('\n').map(parseLine).find((m) => m?.type === 'control_response' && m.response?.request_id === USAGE_REQUEST_ID)?.response;
      if (reply?.subtype === 'success' && reply.response) resolve(rateLimitsOf(reply.response, Date.now()));
      else reject(new UpstreamError(reply?.error || stderr.trim() || err?.message || '答えが返りませんでした'));
    });
    // 使用率だけを読むので、7 日分の会話ログを調べて使い方の傾向を出す処理は飛ばさせる。要求を書いて標準入力を閉じると、claude は答えてから終わる。
    child.stdin?.end(`${JSON.stringify({ type: 'control_request', request_id: USAGE_REQUEST_ID, request: { subtype: 'get_usage', skip_behaviors: true } })}\n`);
  });
