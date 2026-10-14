// ダッシュボードが画面の補助に使う Claude を、claude -p として動かす。ふだんは軽い haiku を使い、コードを読んで確かめるときと、文章校正で文章を読むときは sonnet を使う。
import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { claudeCommand } from './platform.ts';

const HAIKU_TIMEOUT_MS = 30_000;
// 長い文章を読ませると、30 秒を超えることがある。
const SONNET_TIMEOUT_MS = 2 * 60_000;
// コードを読みながら答えるので、数分かかることがある。
const READER_TIMEOUT_MS = 10 * 60_000;

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

// 事実を確かめるにはコードを読んで推論する必要があるため、haiku より確かな sonnet に、読むだけのツールを渡す。確認の要る操作は断る。
// 作業ディレクトリと Read が求める絶対パスが分かるよう、Claude Code の既定のシステムプロンプトのまま動かす。
const READER_ARGS = ['--model', 'sonnet', '--tools', 'Read,Grep,Glob', '--permission-prompts', 'none'];

/** claude の失敗や、求めた形でない答え。ダッシュボードの不具合ではないので、改善要望にはしない。 */
export class UpstreamError extends Error {}

/** 動いている補助のセッション id。実行中だけセッションファイルが現れるので、一覧に記録しないよう見分ける。 */
const running = new Set<string>();

export const isHelperSession = (sessionId: string): boolean => running.has(sessionId);

/** すべての項目を必須にした object の JSON Schema。 */
export const objectSchema = (properties: Record<string, object>): object => ({
  type: 'object',
  properties,
  required: Object.keys(properties),
  additionalProperties: false,
});

/** JSON Schema に沿った答えを返す。 */
const askClaude = (prompt: string, schema: object, args: string[], timeout: number, cwd?: string): Promise<Record<string, unknown>> =>
  new Promise((resolve, reject) => {
    const id = randomUUID();
    running.add(id);
    const all = [...COMMON_ARGS, ...args, '--json-schema', JSON.stringify(schema), '--session-id', id];
    const claude = claudeCommand(all);
    const child = execFile(claude.file, claude.args, { timeout, cwd }, (err, stdout, stderr) => {
      running.delete(id);
      if (err?.killed) return reject(new UpstreamError(`${timeout / 1000} 秒以内に終わりませんでした`));
      try {
        const out = JSON.parse(stdout) as { result?: string; structured_output?: Record<string, unknown> };
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

export const askHaiku = (prompt: string, schema: object): Promise<Record<string, unknown>> => askClaude(prompt, schema, HAIKU_ARGS, HAIKU_TIMEOUT_MS);

export const askSonnet = (prompt: string, schema: object, timeout = SONNET_TIMEOUT_MS): Promise<Record<string, unknown>> => askClaude(prompt, schema, SONNET_ARGS, timeout);

/** cwd のコードを読んで答えさせる。 */
export const askReader = (cwd: string, prompt: string, schema: object): Promise<Record<string, unknown>> =>
  askClaude(prompt, schema, READER_ARGS, READER_TIMEOUT_MS, cwd);
