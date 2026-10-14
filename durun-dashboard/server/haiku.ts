// ダッシュボードが画面の補助に使う Claude を、claude -p として動かす。ふだんは軽い haiku を使い、コードを読んで確かめるときと、文章校正で文章を読むときは sonnet を使う。文章校正の修正版は opus に書かせる。
import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import type { TokenTotals } from '../shared/types.ts';
import { claudeCommand } from './platform.ts';

const HAIKU_TIMEOUT_MS = 30_000;
// 長い文章を読ませると、30 秒を超えることがある。
const SONNET_TIMEOUT_MS = 2 * 60_000;
// コードを読みながら答えるので、数分かかることがある。
const READER_TIMEOUT_MS = 10 * 60_000;
// 長い文章では書き直す文が多く、数分かかることがある。
const OPUS_TIMEOUT_MS = 10 * 60_000;

// フック、MCP、CLAUDE.md を外す。フックが動くと、このセッションもダッシュボードに記録される。
const COMMON_ARGS = ['-p', '--safe-mode', '--no-session-persistence', '--output-format', 'json'];

// 数秒で返るよう、ツールと思考も外す。
const HAIKU_ARGS = [
  '--model', 'haiku', '--tools', '',
  '--settings', JSON.stringify({ alwaysThinkingEnabled: false }),
  '--system-prompt', '人間が複数の Claude Code のセッションを見渡すダッシュボード（durun-dashboard）の中で、頼まれた文を作る。',
];

// 文章を読んで判定させる。haiku より確かな sonnet を、ツールを持たせずに動かす。前提知識を持たない読み手にするため、ダッシュボードの説明も渡さない。
const SONNET_ARGS = [
  '--model', 'sonnet', '--tools', '',
  '--settings', JSON.stringify({ alwaysThinkingEnabled: false }),
  '--system-prompt', '頼まれた文章を読み、求められた形で判定する。',
];

// 指摘に沿って文章を書き直させる。直し方を考えさせるため、sonnet より確かな opus を、思考を外さずに動かす。時間と消費を抑えるよう、effort は medium にする。
const OPUS_ARGS = [
  '--model', 'opus', '--effort', 'medium', '--tools', '',
  '--system-prompt', '頼まれた文章を、求められた形で書き直す。',
];

// 事実を確かめるにはコードを読んで推論する必要があるため、haiku より確かな sonnet に、読むだけのツールを渡す。確認の要る操作は断る。
// 作業ディレクトリと Read が求める絶対パスが分かるよう、Claude Code の既定のシステムプロンプトのまま動かす。
const READER_ARGS = ['--model', 'sonnet', '--tools', 'Read,Grep,Glob', '--permission-prompts', 'none'];

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

export const askOpus = (prompt: string, schema: object): Promise<Record<string, unknown>> =>
  askClaude(prompt, schema, OPUS_ARGS, OPUS_TIMEOUT_MS);

/** cwd のコードを読んで答えさせる。 */
export const askReader = (cwd: string, prompt: string, schema: object): Promise<Record<string, unknown>> =>
  askClaude(prompt, schema, READER_ARGS, READER_TIMEOUT_MS, cwd);
