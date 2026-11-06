// 人間が会話で教えたことを振り返り、CLAUDE.md・スキル・記憶・フックへの差分を提案させる。
// 人間が作業を止めてしばらく経ったら、前回より後の人間の発言を集め、claude -p のセッションに渡す。
// セッションは提案を present で見せて止まる。人間がチャットで承認した提案だけを、再開したセッションが適用する。
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { ChatEntry } from '../shared/types.ts';
import { CLAUDE_DIR, readJson, summarizeLog } from './claude.ts';
import { claudeCommand } from './platform.ts';
import { launchDetached } from './run.ts';
import { DATA_DIR, ensureSession, state, touch } from './store.ts';

const REFLECT_DIR = path.join(DATA_DIR, 'reflect');
const LAST_FILE = path.join(REFLECT_DIR, 'last.json');
const LOG_FILE = path.join(REFLECT_DIR, 'reflect.log');
// 人間が作業を止めたとみなすまでの時間。日中の休憩では動かさず、夜に動かす。
const IDLE_MS = 90 * 60_000;
// 昼の長い会議の後などに 1 日 2 回動かさない。
const MIN_INTERVAL_MS = 20 * 60 * 60_000;
// 直前の Claude の発言は、人間が何に答えたかが分かる程度に切る。人間の発言は、教えた内容を落とさないよう長めに残す。
const REPLY_CHARS = 400;
const PROMPT_CHARS = 1500;
// 1 回に読ませる量の上限。超えた分は古い発言から落とす。
const DIGEST_CHARS = 300_000;
/** 振り返りのセッションに渡す依頼の書き出し。次の振り返りで、この依頼そのものを人間の発言として拾わないために使う。 */
const REFLECT_MARK = '[durun-dashboard 振り返り]';

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
      if (e.kind !== 'prompt' || (e.at ?? 0) <= since || e.text.startsWith(REFLECT_MARK)) continue;
      if (reply) lines.push(`Claude: ${clip(reply, REPLY_CHARS)}`);
      lines.push(`人間: ${clip(e.text, PROMPT_CHARS)}`, '');
      reply = undefined;
    }
    if (lines.length) parts.push(`## ${s.title}\n作業ディレクトリ: ${s.cwd}\n\n${lines.join('\n')}`);
  }
  const all = parts.join('\n\n');
  return all.length > DIGEST_CHARS ? all.slice(all.length - DIGEST_CHARS) : all;
};

const promptFor = (digestFile: string, cwds: string[]): string => `${REFLECT_MARK} 人間が会話で教えたことを振り返り、Claude の指示を改善する提案を作ってください。
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

type Last = { at: number; sessionId?: string };

const lastRun = (): Last => ({ at: 0, ...readJson<Last>(LAST_FILE) });

/** 前回より後に人間の発言があり、前回から十分に経っていれば振り返る。 */
export const shouldReflect = (now: number, last: number, digest: string): boolean => now - last >= MIN_INTERVAL_MS && digest.length > 0;

const mtimeOf = (file: string): number => {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return 0;
  }
};

const reflect = (): void => {
  const now = Date.now();
  const since = lastRun().at || now - MIN_INTERVAL_MS;
  // 会話ログは全部で 1 GB を超えるので、前回より後に書かれたものだけを読む。
  const metas = Object.values(state.sessions).filter((m) => m.transcriptPath && !m.improver && !m.movedTo && mtimeOf(m.transcriptPath) > since);
  const sessions = metas.map((m) => ({ title: m.title ?? m.name ?? m.id, cwd: m.cwd, texts: summarizeLog(m.transcriptPath as string).texts }));
  const digest = digestOf(sessions, since);
  if (!shouldReflect(now, lastRun().at, digest)) return;
  fs.mkdirSync(REFLECT_DIR, { recursive: true });
  const digestFile = path.join(REFLECT_DIR, `${new Date(now).toISOString().slice(0, 10)}.md`);
  fs.writeFileSync(digestFile, digest);
  const sessionId = randomUUID();
  fs.writeFileSync(LAST_FILE, JSON.stringify({ at: now, sessionId } satisfies Last));
  Object.assign(ensureSession(sessionId, REFLECT_DIR), { title: '人間の指示の振り返り' });
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
  launchDetached(claude.file, claude.args, { cwd: REFLECT_DIR, log: LOG_FILE }, (code, error) => {
    if (code || error) console.error(`振り返りのセッションが止まりました（${error ?? `終了コード ${code}`}）`);
  }).catch((e: Error) => console.error(`振り返りのセッションを起動できません: ${e.message}`));
};

let idleTimer: NodeJS.Timeout | undefined;

/** 会話ログが動くたびに呼ぶ。最後に動いてから IDLE_MS 経ったら振り返る。 */
export const noteActivity = (): void => {
  clearTimeout(idleTimer);
  idleTimer = setTimeout(reflect, IDLE_MS).unref();
};
