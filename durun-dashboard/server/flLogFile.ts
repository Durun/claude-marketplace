// FL Connect のログを、つないだセッションごと、日ごとの Markdown のファイルに書き足す。ファイルはセッションの資料に足し、人間とエージェントがあとから読む。
// ここの関数は FL にも MIDI にも触れないので、テストで確かめる。
import fs from 'node:fs';
import path from 'node:path';
import type { FlLogEntry } from '../shared/types.ts';

/**
 * ファイルに書く、エージェントの 1 件のコードと出力の長さ。全部は、セッションの会話ログのツールの呼び出しに残る。
 * 資料の画面は 1 MB より大きなファイルを開けないので、1 日のファイルがそれを超えないようにする。
 */
const MAX_BLOCK_CHARS = 2000;

const pad = (n: number): string => String(n).padStart(2, '0');

/** ファイルを分ける日。人間が読む日付なので、この PC の時刻で決める。 */
export const dayOf = (at: number): string => {
  const d = new Date(at);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

export const clockOf = (at: number): string => new Date(at).toTimeString().slice(0, 8);

/** Markdown の記号として読まれる文字を、そのまま見えるようにする。チャンネルの名前などは、人間が FL で付けたものなので。 */
export const escapeMarkdown = (text: string): string => text.replace(/[\\`*_[\]<>#|~]/g, '\\$&').replace(/\n/g, ' ');

/** text を、中のどのバッククォートの並びよりも長いフェンスで囲む。 */
const fenced = (text: string, lang = ''): string[] => {
  const longest = Math.max(2, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const fence = '`'.repeat(longest + 1);
  return [`${fence}${lang}`, ...text.split('\n'), fence];
};

const clip = (text: string): string => (text.length > MAX_BLOCK_CHARS ? `${text.slice(0, MAX_BLOCK_CHARS)}\n…（${text.length} 文字のうち、先頭の ${MAX_BLOCK_CHARS} 文字）` : text);

/** Python のコードを渡すツール。コードの色付けのために、フェンスに言語を書く。 */
const PYTHON_TOOLS = new Set(['fl_python', 'fl_pianoroll', 'fl_undo']);

/** ログの 1 件を、Markdown の箇条書きの 1 項目にする。エージェントの 1 件は、変わった値と、コードと、FL が返したものを、項目の中に入れる。 */
export const markdownEntry = (e: FlLogEntry): string => {
  const head = `- ${clockOf(e.at)} **${e.who === 'you' ? 'You' : 'Agent'}** ${escapeMarkdown(e.text)}${e.ok === false ? '（失敗）' : ''}`;
  const body = [
    ...(e.changes ?? []).map((c) => `- ${escapeMarkdown(c)}`),
    ...(e.detail ? fenced(clip(e.detail), e.tool && PYTHON_TOOLS.has(e.tool) ? 'python' : '') : []),
    ...(e.output ? fenced(clip(e.output)) : []),
  ];
  // 項目の中の行は、項目の本文の位置（2 文字）まで下げる。
  return [head, ...body.map((l) => (l ? `  ${l}` : l))].join('\n');
};

/** ログを書くファイル。セッションごとのフォルダーに、日ごとに分ける。 */
export const logFilePath = (dir: string, sessionId: string, at: number): string => path.join(dir, sessionId, `FL Connect ${dayOf(at)}.md`);

const fileHead = (title: string, day: string): string =>
  `# FL Connect のログ（${escapeMarkdown(title)}、${day}）\n\nFL Connect のウィンドウを開いている間に、人間（You）が FL Studio でした操作と、エージェント（Agent）がツールでしたことを、時刻の順に書き足す。番号は MIDI スクリプトの API の番号。\n`;

/**
 * 項目と見出しを、その日のファイルに書き足す。新しく作ったファイルの場所を返す。
 * 項目は時刻の順に渡す。日をまたいだら、次の日のファイルに書く。
 */
export const appendLog = (dir: string, sessionId: string, title: string, items: ({ at: number; heading: string } | FlLogEntry)[]): string[] => {
  const created: string[] = [];
  const byFile = new Map<string, string[]>();
  for (const item of items) {
    const file = logFilePath(dir, sessionId, item.at);
    const lines = byFile.get(file) ?? [];
    lines.push('heading' in item ? `\n## ${clockOf(item.at)} ${escapeMarkdown(item.heading)}\n` : markdownEntry(item));
    byFile.set(file, lines);
  }
  for (const [file, lines] of byFile) {
    if (!fs.existsSync(file)) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, fileHead(title, path.basename(file, '.md').replace(/^FL Connect /, '')));
      created.push(file);
    }
    fs.appendFileSync(file, `${lines.join('\n')}\n`);
  }
  return created;
};
