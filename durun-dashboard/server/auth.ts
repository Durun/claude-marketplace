// Claude が求めた認証を、人間に確かめてから進める。相手は Google Cloud と、ログインの要る MCP サーバー（Slack、Notion など）と、ブラウザでログインする Web サービス。
// Claude のログインが切れて止まったセッションのために、ダッシュボードも Claude のログインを求める。人間は、通知領域のボタンからもログインし直せる。
// 同じ相手の認証を複数のセッションが求めても、人間に尋ねるのは 1 回にし、答えと認証の結果を、求めたすべてのセッションに返す。
import { type ChildProcess, exec, execFile } from 'node:child_process';
import type { AuthRequest, ClaudeLogin } from '../shared/types.ts';
import { claudeCommand, platform } from './platform.ts';
import { changed, newId, state, touch } from './store.ts';

// claude mcp get は、MCP サーバーにつないで確かめるので数秒かかる。
const CHECK_TIMEOUT_MS = 60_000;
// 人間がブラウザでの認証をやめると、コマンドは終わらずに待ち続ける。止めて、次に求められたときに尋ね直せるようにする。
const LOGIN_TIMEOUT_MS = 5 * 60_000;
// 認証の失敗を伝えるときに添える、コマンドの出力の末尾の行数
const ERROR_LINES = 3;

/**
 * 尋ねる前に動かし始めた認証のコマンド。loginUrl は、コマンドが受け取りを待っているログイン画面で、URL を出さずに終わったコマンドは持たない。
 * finish は、OK の後にコマンドが終わるのを待つ。cancel は、却下されたときにコマンドを止める。
 */
type PreparedLogin = { loginUrl?: string; finish: () => Promise<void>; cancel: () => void };

/**
 * 認証する相手。title は「〜の認証」。人間に見せるのは、OK で動かす認証の command か、「進む」で開くログイン画面の url。check は、認証済みなら true を返す。
 * lastLoginAt は、ダッシュボードから前回ログインした時刻で、ダッシュボードが覚えている相手だけが持つ。
 * login は、OK の後に認証する。prepare を持つ相手は login を持たず、尋ねる前に認証のコマンドを動かし始める。
 */
export type AuthProvider = { key: string; title: string; command?: string; url?: string; lastLoginAt?: number; check: () => Promise<boolean>; login?: () => Promise<void>; prepare?: () => Promise<PreparedLogin> };

/** 文字列はシェルで動かす行。 */
type Command = string | { file: string; args: string[] };

/** timedOut は、時間切れかダッシュボードの都合で止めたか。 */
type RunResult = { ok: boolean; output: string; timedOut: boolean };

/** 尋ねてくるコマンドが答えを待ち続けないよう、標準入力は閉じる。 */
const start = (command: Command, cwd: string | undefined, timeout: number, env?: NodeJS.ProcessEnv): { child: ChildProcess; result: Promise<RunResult> } => {
  let resolve: (r: RunResult) => void = () => {};
  const result = new Promise<RunResult>((r) => (resolve = r));
  const done = (e: { killed?: boolean } | null, stdout: string, stderr: string): void => resolve({ ok: !e, output: `${stdout}\n${stderr}`.trim(), timedOut: !!e?.killed });
  const child = typeof command === 'string' ? exec(command, { cwd, timeout, env }, done) : execFile(command.file, command.args, { cwd, timeout, env }, done);
  child.stdin?.end();
  return { child, result };
};

const run = (command: Command, cwd: string | undefined, timeout: number): Promise<RunResult> => start(command, cwd, timeout).result;

const throwIfFailed = ({ ok, output, timedOut }: RunResult): void => {
  if (timedOut) throw new Error(`${LOGIN_TIMEOUT_MS / 60_000} 分たっても認証が終わらないので、止めました。`);
  if (!ok) throw new Error(output.split('\n').slice(-ERROR_LINES).join('\n') || 'コマンドが失敗しました。');
};

const login = async (command: Command, cwd: string | undefined): Promise<void> => throwIfFailed(await run(command, cwd, LOGIN_TIMEOUT_MS));

// gcloud は Python の webbrowser でログイン画面を開くので、BROWSER で、開く代わりに URL を標準出力に書くコマンドへ差し替える。
// webbrowser は %s を含む BROWSER を shlex で分けるので、空白を含むパスも分かれないよう引用符で囲む。
const PRINT_URL = `${JSON.stringify(process.execPath)} -p process.argv[1] %s`;

/**
 * OK を押してから gcloud の起動を待たせないよう、尋ねる前に動かし始め、ログイン画面の URL を出すか、終わるまで待つ。
 * ログイン画面は、画面が OK で開く。待つ時間の上限は、OK を押してから数える。
 */
const prepareLogin = (command: string, cwd: string | undefined): Promise<PreparedLogin> => {
  const { child, result } = start(command, cwd, 0, { ...process.env, BROWSER: PRINT_URL });
  const cancel = (): void => platform.killTree(child);
  // ダッシュボードが止まったら、ログイン画面の受け取りを待ち続けないよう止める。
  process.once('exit', cancel);
  const finish = async (): Promise<void> => {
    const timer = setTimeout(cancel, LOGIN_TIMEOUT_MS);
    throwIfFailed(await result.finally(() => clearTimeout(timer)));
  };
  return new Promise((resolve) => {
    let stdout = '';
    child.stdout?.on('data', (chunk: string) => {
      const loginUrl = /^https?:\/\/\S+(?=\s)/m.exec((stdout += chunk))?.[0];
      if (loginUrl) resolve({ loginUrl, finish, cancel });
    });
    void result.then(() => {
      process.off('exit', cancel);
      resolve({ finish, cancel });
    });
  });
};

/** コマンドは .env.local で差し替えられる。確かめのコマンドが成功すれば、認証済みとみなす。 */
const gcloud = (cwd: string | undefined): AuthProvider => {
  const command = process.env.GCLOUD_LOGIN_COMMAND || 'gcloud auth login';
  const check = process.env.GCLOUD_CHECK_COMMAND || 'gcloud auth print-access-token';
  return { key: 'gcloud', title: 'Google Cloud の認証', command, check: async () => (await run(check, cwd, CHECK_TIMEOUT_MS)).ok, prepare: () => prepareLogin(command, cwd) };
};

/**
 * プロジェクトに限って登録した MCP サーバーもあるので、求めたセッションの作業ディレクトリで動かす。
 * 登録されていない名前は、人間に尋ねずに失敗にする。claude の出力には、登録されている名前が並ぶ。
 */
const mcpServer = (name: string, cwd: string | undefined): AuthProvider => ({
  key: `mcp:${name}`,
  title: `MCP サーバー「${name}」の認証`,
  command: `claude mcp login ${JSON.stringify(name)}`,
  check: async () => {
    const r = await run(claudeCommand(['mcp', 'get', name]), cwd, CHECK_TIMEOUT_MS);
    if (!r.ok) throw new Error(r.output);
    return /Status:.*Connected/.test(r.output);
  },
  login: () => login(claudeCommand(['mcp', 'login', name]), cwd),
});

/**
 * ログインできたかはダッシュボードから確かめられないので、毎回尋ね、人間が「認証した」と答えたら終える。
 * OAuth の URL は求めるたびに state が変わり、別々のログインになるので、同じ URL を求めたセッションだけをまとめる。
 */
const browser = (url: URL): AuthProvider => ({ key: `browser:${url.href}`, title: `${url.host} のブラウザ認証`, url: url.href, check: async () => false, login: async () => {} });

/** authenticate に渡された相手。分からなければ undefined。ログイン画面の URL は画面が window.open で開くので、http と https に限る。 */
export const authProvider = (provider: unknown, server: string | undefined, cwd: string | undefined, url?: string): AuthProvider | undefined => {
  if (provider === 'gcloud') return gcloud(cwd);
  if (provider === 'browser') {
    const page = URL.parse(url ?? '');
    return page?.protocol === 'http:' || page?.protocol === 'https:' ? browser(page) : undefined;
  }
  return provider === 'mcp' && server ? mcpServer(server, cwd) : undefined;
};

const CLAUDE_KEY = 'claude';
// 動いている claude auth login。ダイアログと通知領域のボタンから続けて頼まれても、1 回だけ動かす。
let claudeLoggingIn: Promise<void> | undefined;
// 前回ログインし直せなかった理由。ログインし直せたら消す。
let claudeLoginError: string | undefined;

/** Claude にログインし直す。ログインし終えたら時刻を覚え、Claude のログインを尋ねているダイアログがあれば、ログインし終えたとして閉じる。 */
export const loginClaude = (): Promise<void> => {
  if (claudeLoggingIn) return claudeLoggingIn;
  claudeLoggingIn = login(claudeCommand(['auth', 'login']), undefined)
    .then(
      () => {
        state.claudeLoginAt = Date.now();
        claudeLoginError = undefined;
        const flow = flows.get(CLAUDE_KEY);
        if (flow?.request.asking) answerAuth(flow.request.id, true);
      },
      (e: Error) => {
        claudeLoginError = e.message;
        throw e;
      },
    )
    .finally(() => {
      claudeLoggingIn = undefined;
      touch();
    });
  changed();
  return claudeLoggingIn;
};

/**
 * Claude のログイン。ログインが切れたことは会話ログで分かってから求めるので、確かめずに尋ねる。
 * since は求め始めた時刻。尋ねている間に人間が通知領域のボタンからログインし直していれば、もうログインしない。
 */
export const claudeLogin = (since: number): AuthProvider => ({
  key: CLAUDE_KEY,
  title: 'Claude のログイン',
  command: 'claude auth login',
  lastLoginAt: state.claudeLoginAt,
  check: async () => false,
  login: async () => {
    if ((state.claudeLoginAt ?? 0) <= since) await loginClaude();
  },
});

/** 通知領域に出す、ダッシュボードからの Claude のログイン。 */
export const claudeLoginState = (): ClaudeLogin => ({ at: state.claudeLoginAt, running: !!claudeLoggingIn, error: claudeLoginError });

/** ok は、認証済みになったか。text は、Claude に返す結果。 */
export type AuthResult = { ok: boolean; text: string };

type Flow = { request: AuthRequest & { asking: boolean }; decide: (ok: boolean) => void; result: Promise<AuthResult> };

// 確かめか認証が進んでいる相手ごとの流れ。終わったら消す。
const flows = new Map<string, Flow>();

const settle = async (provider: AuthProvider, request: Flow['request'], answer: Promise<boolean>): Promise<AuthResult> => {
  if (await provider.check()) return { ok: true, text: `${provider.title}は済んでいます。` };
  const prepared = await provider.prepare?.();
  if (prepared?.loginUrl) request.loginUrl = prepared.loginUrl;
  request.asking = true;
  changed();
  if (!(await answer)) {
    prepared?.cancel();
    return { ok: false, text: `${provider.title}を、人間が却下しました。` };
  }
  await (prepared ? prepared.finish() : provider.login?.());
  return { ok: true, text: `${provider.title}を終えました。` };
};

/** 認証済みならすぐに返し、切れていれば人間に尋ねて、OK なら認証する。同じ相手の流れが進んでいれば、それに加わって同じ結果を待つ。 */
export const requestAuth = (provider: AuthProvider, sessionId: string): Promise<AuthResult> => {
  let flow = flows.get(provider.key);
  if (!flow) {
    const { title, command, url, lastLoginAt } = provider;
    const request: Flow['request'] = { id: newId(), title, ...(url ? { url } : { command }), ...(lastLoginAt ? { lastLoginAt } : {}), sessionIds: [], asking: false };
    let decide: (ok: boolean) => void = () => {};
    const answer = new Promise<boolean>((resolve) => (decide = resolve));
    flow = { request, decide, result: settle(provider, request, answer).finally(() => flows.delete(provider.key)) };
    flows.set(provider.key, flow);
  }
  if (!flow.request.sessionIds.includes(sessionId)) {
    flow.request.sessionIds.push(sessionId);
    changed();
  }
  return flow.result;
};

/** 人間に尋ねている認証。求めた順。 */
export const authRequests = (): AuthRequest[] => [...flows.values()].flatMap(({ request: { asking, ...rest } }) => (asking ? [rest] : []));

/** 人間の答えを返す。ほかの画面で先に答えていたら何もしない。 */
export const answerAuth = (id: string, ok: boolean): void => {
  const flow = [...flows.values()].find((f) => f.request.id === id && f.request.asking);
  if (!flow) return;
  flow.request.asking = false;
  changed();
  flow.decide(ok);
};
