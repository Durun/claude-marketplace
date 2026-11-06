// 学習アプリ。人間が会話で教えたことを振り返り、CLAUDE.md・スキル・記憶・フックへの差分を提案させる。
// 定期実行を ON にすると、人間が作業を止めてしばらくたったら動く。アプリの「今すぐ学習する」でも動かせる。
// 前回より後の人間の発言を集め、claude -p のセッションに渡す。セッションは提案を present で見せて止まる。
// 人間がチャットで承認した提案だけを、再開したセッションが適用する。知見のファイルとその管理は利用者の側にあり、ダッシュボードは書き込まない。
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { ChatEntry, LearnSettings, LearnState } from '../shared/types.ts';
import { CLAUDE_DIR, isAlive, summarizeLog } from './claude.ts';
import { claudeCommand } from './platform.ts';
import { launchDetached } from './run.ts';
import { DATA_DIR, ensureSession, state, touch } from './store.ts';

const LEARN_DIR = path.join(DATA_DIR, 'learn');
const LOG_FILE = path.join(LEARN_DIR, 'learn.log');
// 人間が作業を止めたとみなすまでの時間。日中の休憩では動かさず、夜に動かす。
const IDLE_MS = 90 * 60_000;
// 昼の長い会議の後などに、定期実行で 1 日 2 回動かさない。
const MIN_INTERVAL_MS = 20 * 60 * 60_000;
// 直前の Claude の発言は、人間が何に答えたかが分かる程度に切る。人間の発言は、教えた内容を落とさないよう長めに残す。
const REPLY_CHARS = 400;
const PROMPT_CHARS = 1500;
// 1 回に読ませる量の上限。超えた分は古い発言から落とす。
const DIGEST_CHARS = 300_000;
const RUNS_KEPT = 30;
/** 学習のセッションに渡す依頼の書き出し。次の学習で、この依頼そのものを人間の発言として拾わないために使う。 */
const LEARN_MARK = '[durun-dashboard 学習]';

export type DigestSession = { title: string; cwd: string; texts: ChatEntry[] };

const clip = (text: string, max: number): string => {
  const flat = text.trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
};

/**
 * since より後の人間の発言を、直前の Claude の発言と組にして並べる。
 * 人間が「そうではない、こうだ」と教えるのは、Claude の発言を受けたときなので、その発言を添える。
 */
export const digestOf = (sessions: DigestSession[], since: number): string => {
  const parts: string[] = [];
  for (const s of sessions) {
    const lines: string[] = [];
    let reply: string | undefined;
    for (const e of s.texts) {
      if (e.kind === 'message') reply = e.text;
      if (e.kind !== 'prompt' || (e.at ?? 0) <= since || e.text.startsWith(LEARN_MARK)) continue;
      if (reply) lines.push(`Claude: ${clip(reply, REPLY_CHARS)}`);
      lines.push(`人間: ${clip(e.text, PROMPT_CHARS)}`, '');
      reply = undefined;
    }
    if (lines.length) parts.push(`## ${s.title}\n作業ディレクトリ: ${s.cwd}\n\n${lines.join('\n')}`);
  }
  const all = parts.join('\n\n');
  return all.length > DIGEST_CHARS ? all.slice(all.length - DIGEST_CHARS) : all;
};

const promptFor = (digestFile: string, cwds: string[]): string => `${LEARN_MARK} 人間が会話で教えたことを振り返り、Claude の指示を改善する提案を作ってください。
目的は、人間が「そうではない、こうだ」と教えた知見を蓄え、次からは人間が言わなくても理想の動きをすることです。

## 読むもの
- 人間の発言と、その直前の Claude の発言: ${digestFile}（大きいときは分けて読む）
- いまの指示: ${path.join(CLAUDE_DIR, 'CLAUDE.md')}、${path.join(CLAUDE_DIR, 'skills')}、${path.join(CLAUDE_DIR, 'projects')}/*/memory/、${path.join(CLAUDE_DIR, 'settings.json')} の hooks
- 作業ディレクトリの CLAUDE.md: ${cwds.join('、')}

## 拾うもの
- 人間が Claude の動きを訂正した、却下した、やり直させた、好みや判断の基準を示した発言
- 同じ訂正が何度も出ているもの。既に指示に書いてあるのに破られたものは、書き足さずに、書き方・置き場所を直すか、フックで強制する案にする
- 人間の発言と矛盾する指示、使われていない指示、重複した指示。削除や統合を提案する

## 拾わないもの
- Claude が指示されなくても自分で導ける手順。細かい手順を文書にしない
- その場限りの事情、作業の経緯、人間が教えていない Claude 自身の思いつき

## 提案の形
- 1 件ごとに、置くファイル、変更前の文（ファイルにあるとおり。新しく足すなら空）、変更後の文を書く。ファイル全体は書き直さない
- 置き場所は、いつも効かせるなら CLAUDE.md、場面が限られるならスキル、機械的に判定できるならフック、事実なら記憶を選ぶ
- 防ぐ失敗、根拠（セッションの題名と、人間の発言をそのまま引用）、不要になる条件を 1 行ずつ添える
- 多くても 7 件。人間が教えたことが無ければ、提案は無いと書く

## 進め方
- 提案は Markdown にまとめ、present で人間に見せてから止まる
- 人間が承認するまで、ファイルを変えない。承認されたら、承認された提案だけを適用する`;

/** 定期実行では、前回から十分にたっているときだけ学習する。 */
export const isDue = (now: number, last: number): boolean => now - last >= MIN_INTERVAL_MS;

const mtimeOf = (file: string): number => {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return 0;
  }
};

const running = (): boolean => !!state.learn.pid && isAlive(state.learn.pid);

const lastAt = (): number => state.learn.runs[0]?.at ?? 0;

/** 前回より後の人間の発言を集めて、学習のセッションを起動する。始められなかったときは、理由を error に残す。 */
const learn = (): void => {
  if (running()) return;
  const now = Date.now();
  const since = lastAt() || now - MIN_INTERVAL_MS;
  // 会話ログは全部で 1 GB を超えるので、前回より後に書かれたものだけを読む。
  const metas = Object.values(state.sessions).filter((m) => m.transcriptPath && !m.improver && !m.movedTo && mtimeOf(m.transcriptPath) > since);
  const sessions = metas.map((m) => ({ title: m.title ?? m.name ?? m.id, cwd: m.cwd, texts: summarizeLog(m.transcriptPath as string).texts }));
  const digest = digestOf(sessions, since);
  if (!digest) {
    state.learn.error = '前回の学習より後に、人間の発言がありません。';
    return touch();
  }
  fs.mkdirSync(LEARN_DIR, { recursive: true });
  const digestFile = path.join(LEARN_DIR, `${new Date(now).toISOString().replace(/[:.]/g, '').slice(0, 15)}.md`);
  fs.writeFileSync(digestFile, digest);
  const sessionId = randomUUID();
  Object.assign(ensureSession(sessionId, LEARN_DIR), { title: '人間の指示から学習', learner: true });
  state.learn.runs = [{ at: now, sessionId }, ...state.learn.runs].slice(0, RUNS_KEPT);
  state.learn.error = undefined;
  touch();
  const cwds = [...new Set(sessions.filter((s) => s.texts.some((e) => (e.at ?? 0) > since)).map((s) => s.cwd))];
  const args = [
    '-p', '--session-id', sessionId,
    // 判断の要る読み取りなので opus にする。
    '--model', 'opus',
    // 承認を待つ前にファイルを変えさせない。人間がチャットで返すと、ふだんの権限で再開するので、承認した後は書ける。
    '--disallowedTools', 'Edit,Write,NotebookEdit,Bash',
    // 提案を見せる present を、確認を待たずに使わせる。
    '--allowedTools', 'mcp__plugin_durun-dashboard_durun-dashboard__present',
    // 作業ディレクトリの CLAUDE.md も読めるよう、ホームの下を読ませる。書き込みは上で外してある。
    '--add-dir', os.homedir(),
    '--permission-prompts', 'none',
    '--', promptFor(digestFile, cwds),
  ];
  const claude = claudeCommand(args);
  const fail = (reason: string): void => {
    state.learn.error = `学習のセッションが止まりました（${reason}）。`;
    touch();
  };
  launchDetached(claude.file, claude.args, { cwd: LEARN_DIR, log: LOG_FILE }, (code, error) => {
    state.learn.pid = undefined;
    if (code || error) fail(error ?? `終了コード ${code}`);
    else touch();
  }).then(
    (pid) => {
      state.learn.pid = pid;
      touch();
    },
    (e: Error) => fail(e.message),
  );
};

let idleTimer: NodeJS.Timeout | undefined;

/** 会話ログが動くたびに呼ぶ。定期実行が ON なら、最後に動いてから IDLE_MS たったところで学習する。 */
export const noteActivity = (): void => {
  clearTimeout(idleTimer);
  if (!state.learn.scheduled) return;
  idleTimer = setTimeout(() => isDue(Date.now(), lastAt()) && learn(), IDLE_MS).unref();
};

/** アプリの「今すぐ学習する」。前回からの間隔を問わない。 */
export const learnNow = (): void => learn();

export const updateLearn = (patch: Partial<LearnSettings>): void => {
  Object.assign(state.learn, patch);
  touch();
  noteActivity();
};

export const learnState = (): LearnState => ({ scheduled: state.learn.scheduled, running: running(), runs: state.learn.runs, error: state.learn.error });
