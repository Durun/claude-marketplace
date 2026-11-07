// FL Connect とムービーメーカーのログを、つないだセッションごと、日ごとの JSONL のファイルに書き足す。1 行に 1 件、アプリが返した生の値のまま書き、人間に見せる文にするのはウィンドウに任せる。
// ファイルはセッションの資料に足し、人間とエージェントがあとから読む。ここの関数は FL にも MIDI にも触れないので、テストで確かめる。
import fs from 'node:fs';
import path from 'node:path';
import type { FlLogEntry, MmLogEntry } from '../shared/types.ts';

/**
 * ファイルに書く、エージェントの 1 件のコードと出力の長さ。全部は、セッションの会話ログのツールの呼び出しに残る。
 * 資料の画面は 1 MB より大きなファイルを開けないので、1 日のファイルがそれを超えにくくする。
 */
const MAX_TEXT_CHARS = 20000;

const pad = (n: number, width = 2): string => String(n).padStart(width, '0');

/** ファイルを分ける日。人間が読む日付なので、この PC の時刻で決める。 */
export const dayOf = (at: number): string => {
  const d = new Date(at);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

/** この PC の時刻に時差を添えた、ISO 8601 の時刻。 */
export const isoLocal = (at: number): string => {
  const d = new Date(at);
  const offset = -d.getTimezoneOffset();
  const zone = `${offset >= 0 ? '+' : '-'}${pad(Math.floor(Math.abs(offset) / 60))}:${pad(Math.abs(offset) % 60)}`;
  return `${dayOf(at)}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}${zone}`;
};

/** ログの区切りの印。FL とつながったときなど。 */
export type FlLogEvent = { at: number; event: string } & Record<string, unknown>;

/** ログの 1 件。 */
export type LogItem = FlLogEntry | MmLogEntry | FlLogEvent;

/** ファイルの名前の頭。日付の前に付ける。 */
const DEFAULT_TITLE = 'FL Connect';

const clip = (text: unknown): unknown => (typeof text === 'string' && text.length > MAX_TEXT_CHARS ? `${text.slice(0, MAX_TEXT_CHARS)}…（${text.length} 文字のうち、先頭の ${MAX_TEXT_CHARS} 文字）` : text);

/** ログの 1 件を、JSONL の 1 行にする。時刻は読める形にし、ウィンドウが並べるための id は書かない。 */
export const jsonLine = (item: LogItem): string => {
  const { at, id: _id, ...fields } = item as { at: number; id?: string } & Record<string, unknown>;
  if ('detail' in fields) fields.detail = clip(fields.detail);
  if ('output' in fields) fields.output = clip(fields.output);
  return JSON.stringify({ at: isoLocal(at), ...fields });
};

/** ログを書くファイル。セッションごとのフォルダーに、日ごとに分ける。 */
export const logFilePath = (dir: string, sessionId: string, at: number, title = DEFAULT_TITLE): string => path.join(dir, sessionId, `${title} ${dayOf(at)}.jsonl`);

/**
 * ログを、その日のファイルに書き足す。新しく作ったファイルの場所を返す。
 * 項目は時刻の順に渡す。日をまたいだら、次の日のファイルに書く。
 */
export const appendLog = (dir: string, sessionId: string, items: LogItem[], title = DEFAULT_TITLE): string[] => {
  const created: string[] = [];
  const byFile = new Map<string, string[]>();
  for (const item of items) {
    const file = logFilePath(dir, sessionId, item.at, title);
    byFile.set(file, [...(byFile.get(file) ?? []), jsonLine(item)]);
  }
  for (const [file, lines] of byFile) {
    if (!fs.existsSync(file)) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      created.push(file);
    }
    fs.appendFileSync(file, `${lines.join('\n')}\n`);
  }
  return created;
};
