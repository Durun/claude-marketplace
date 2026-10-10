// 改善要望を保存し、自動で実装する設定なら claude -p を起動して、たまった要望を 1 つのウェーブとして対応させる。
// セッションは要望の全体を見て計画を立て、似た作業をまとめて進める。ウェーブを終えたら一区切りとし、残りと新しい要望は次のセッションで計画し直す。
// OFF にすると、対応中のウェーブを終えたところで止まる。
// 失敗したときと、済んだ要望が無いまま終わったときは、ON のまま待機する。新しい要望が届くか、人間が続けると再開する。
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import type { ImproveSettings, ImproveState } from '../shared/types.ts';
import { isAlive, listDir, readJson, summarizeLog } from './claude.ts';
import { claudeCommand } from './platform.ts';
import { DATA_DIR, ensureSession, newId, state, touch, type ImproveRun } from './store.ts';

const ROOT = path.join(import.meta.dirname, '..');
const FEEDBACK_DIR = path.join(DATA_DIR, 'feedback');
const DONE_DIR = path.join(FEEDBACK_DIR, 'done');
const LOG_FILE = path.join(DATA_DIR, 'improve.log');
const SUMMARY_CHARS = 120;
const DONE_MARK = '対応した要望:';
const BASH_TIMEOUT_MS = 2 * 60 * 1000;

/** 未対応の要望。ファイル名が時刻から始まるので、名前順が古い順になる。 */
export const pendingItems = (): string[] => listDir(FEEDBACK_DIR).filter((f) => f.endsWith('.json')).sort();

const summaryOf = (item: string): string =>
  String(readJson<{ text: string }>(path.join(FEEDBACK_DIR, item))?.text ?? '').replace(/\s+/g, ' ').slice(0, SUMMARY_CHARS);

const promptFor = (items: string[]): string => `[durun-dashboard] 人間がダッシュボードの画面から送った改善要望が ${items.length} 件たまっています。全体を見て作業計画を立て、1 つのウェーブとして実装してください。
- 要望は次のファイルです（古い順）。text が要望で、targets は人間が画面上で指した要素です。selector のクラス名から、描画しているコンポーネントを探せます。context は送ったときの画面の状態です。context.snapshot は大きいので、必要なところだけ読んでください。
${items.map((item) => `  - ${path.relative(ROOT, path.join(FEEDBACK_DIR, item))}: ${summaryOf(item)}`).join('\n')}
- まず全体を読み、似た作業や同じ場所を触る要望をまとめて、このウェーブで対応する要望と順番を決めてください。一度に仕上げられる量にとどめ、残りは次のウェーブに回してください。
- ダッシュボードは起動済みで、コードを変えると画面とサーバーに自動で反映されます。pnpm start は実行しないでください。
- まとまりごとに pnpm check と pnpm test を通し、自分が変更したファイルだけを 1 つのコミットにしてください。push はしないでください。コードレビューは依頼せず、そのまま次のまとまりへ進んでください。
- 判断に迷う点は ask_human で人間に聞いてください。
- 終えたら、まとまりごとに何をどう変えたかを 2、3 行で書いてください。最後に「${DONE_MARK}」の行を置き、続けて対応を終えた要望のファイル名を 1 行に 1 つずつ書いて止まってください。残りの要望は、次のウェーブで計画し直します。`;

// 済んだ要望は、何をしたか（セッションの最後の発言）を先頭に置いて done/ へ移す。
const finishItem = (item: string, sessionId: string, result: string | undefined): void => {
  const file = path.join(FEEDBACK_DIR, item);
  fs.mkdirSync(DONE_DIR, { recursive: true });
  fs.writeFileSync(path.join(DONE_DIR, item), JSON.stringify({ result, sessionId, doneAt: new Date().toISOString(), ...readJson(file) }, null, 2));
  fs.rmSync(file, { force: true });
};

const lastLogLine = (): string => {
  try {
    return fs.readFileSync(LOG_FILE, 'utf8').trim().split('\n').pop() ?? '';
  } catch {
    return '';
  }
};

const alive = (run: ImproveRun): boolean => run.pid > 0 && isAlive(run.pid);

/** 最後の発言の「対応した要望:」より後に名前がある要望を、対応を終えたものとみなす。 */
const doneItems = (message: string | undefined, pending: string[]): string[] => {
  const at = message?.lastIndexOf(DONE_MARK) ?? -1;
  return at < 0 ? [] : pending.filter((item) => message?.includes(item, at));
};

/**
 * 対応を終えた要望を済みにする。1 件も済まなかったときは、同じ計画を繰り返さないよう待機する。
 * 最後の発言を渡されなければ会話ログから読む。プロセスが終わった後なら、会話ログは書き終わっている。
 */
const finishWave = (sessionId: string, lastMessage: string | undefined): void => {
  const transcript = state.sessions[sessionId]?.transcriptPath;
  const result = lastMessage || (transcript ? summarizeLog(transcript).lastMessage : undefined);
  const done = doneItems(result, pendingItems());
  for (const item of done) finishItem(item, sessionId, result);
  if (!done.length) state.improve.error = `改善のセッションが、対応を終えた要望を「${DONE_MARK}」に書かずに止まりました。`;
};

/** 改善のセッションが応答を終えたか、止まったときに呼ぶ。失敗したときは、同じ失敗を繰り返さないよう待機する。 */
const endRun = (sessionId: string, failure?: string, lastMessage?: string): void => {
  const run = state.improve.run;
  if (run?.sessionId !== sessionId) return;
  state.improve.run = undefined;
  if (failure) state.improve.error = `改善のセッションが止まりました（${failure}）。${lastLogLine()}`;
  else finishWave(sessionId, lastMessage);
  touch();
  ensureImprover();
};

const startRun = (items: string[]): void => {
  const sessionId = randomUUID();
  const { model, effort } = state.improve;
  const args = [
    '-p', '--session-id', sessionId,
    // 可変長の引数なので、プロンプトを飲み込まないよう後ろに別のオプションを置く。
    // 人間の確認を待たずに進めるよう、コードレビューの依頼は使わせない。
    '--allowedTools', 'mcp__plugin_durun-dashboard_durun-dashboard', '--disallowedTools', 'mcp__plugin_durun-dashboard_durun-dashboard__request_review',
    // 人が見ていない間も止まらないよう、確認の要る操作は auto モードに判断させ、それでも確認が要るものは断る。
    '--permission-mode', 'auto', '--permission-prompts', 'none',
    // 入力を待ち続けるコマンドがあっても、人が見ていないまま長く止まらないよう、Bash を 2 分でバックグラウンドへ移させる。
    // ユーザー設定の既定タイムアウトは対話向けに長いことがあるので、ここで上書きする。
    '--settings', JSON.stringify({ env: { BASH_DEFAULT_TIMEOUT_MS: String(BASH_TIMEOUT_MS) } }),
    ...(model ? ['--model', model] : []),
    ...(effort ? ['--effort', effort] : []),
    promptFor(items),
  ];
  fs.mkdirSync(DATA_DIR, { recursive: true });
  const log = fs.openSync(LOG_FILE, 'a');
  // サーバーがコードの変更で再起動しても止まらないよう、別のプロセスグループで動かし、出力はファイルに流す。
  const claude = claudeCommand(args);
  const child = spawn(claude.file, claude.args, { cwd: ROOT, detached: true, stdio: ['ignore', log, log] });
  fs.closeSync(log);
  child.on('error', (e) => endRun(sessionId, e.message));
  child.on('exit', (code) => endRun(sessionId, code ? `終了コード ${code}` : undefined));
  Object.assign(ensureSession(sessionId, ROOT), { title: '改善要望の対応', improver: true });
  state.improve.run = { sessionId, pid: child.pid ?? 0, items, summary: `${items.length} 件の要望のウェーブ`, startedAt: Date.now() };
  touch();
};

/** 自動実装が有効で待機中でもなく、未対応の要望があり、改善のセッションが動いていなければ起動する。待機の理由は error に残っている。 */
export const ensureImprover = (): void => {
  const run = state.improve.run;
  // サーバーが再起動した後は終了を受け取れないので、プロセスが消えていたら終わったとみなす。
  if (run && !alive(run)) return endRun(run.sessionId);
  const items = pendingItems();
  if (!run && state.improve.enabled && !state.improve.error && items.length) startRun(items);
};

/**
 * 改善のセッションが応答を終えたとき（Stop）に呼ぶ。ウェーブはここで区切り、次のウェーブは新しいセッションで計画し直させる。
 * プロセスの終了を待たないのは、ウェーブの途中でサーバーが再起動していると、終了を受け取れないため。
 * 最後の発言は、フックが渡すものを使う。Stop の時点では、最後の発言が会話ログにまだ書かれていないことがある。
 */
export const nextImprovement = (sessionId: string, lastMessage?: string): void => endRun(sessionId, undefined, lastMessage);

export const saveFeedback = (text: string, targets: unknown[], context: unknown): string => {
  const createdAt = new Date().toISOString();
  // 同じ秒に続けて送っても順番が崩れないよう、ミリ秒まで名前に入れる。
  const file = path.join(FEEDBACK_DIR, `${createdAt.replace(/[:.]/g, '').slice(0, 20)}-${newId()}.json`);
  fs.mkdirSync(FEEDBACK_DIR, { recursive: true });
  // 画面の状態は大きいので最後に置き、ファイルの先頭で要望と指した要素を読めるようにする。
  fs.writeFileSync(file, JSON.stringify({ createdAt, text, targets, context }, null, 2));
  // 待機中なら、新しい要望を機に再開する。
  if (state.improve.enabled) state.improve.error = undefined;
  // 未対応の数が変わるので、起動しないときも画面を更新させる。
  touch();
  ensureImprover();
  return path.relative(ROOT, file);
};

export const updateImprove = (patch: Partial<ImproveSettings>): void => {
  if (patch.enabled) state.improve.error = undefined;
  Object.assign(state.improve, patch);
  touch();
  ensureImprover();
};

export const improveState = (): ImproveState => {
  const { enabled, model, effort, run, error } = state.improve;
  const running = !!run && alive(run);
  return {
    enabled,
    model,
    effort,
    running,
    sessionId: run?.sessionId,
    current: running ? run?.summary : undefined,
    pending: pendingItems().filter((f) => !run?.items?.includes(f)).length,
    error,
  };
};
