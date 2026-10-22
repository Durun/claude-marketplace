// 外部の情報源（gh、JIRA、Notion、haiku）から読んだ値を、キーごとに覚えておく。
// 覚えた値はファイルにも残し、サーバーを起動し直した直後から返す。古くなった値は返してから裏で読み直し、変わったら画面に知らせる。
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { DATA_DIR, changed } from './store.ts';

const FILE = path.join(DATA_DIR, 'cache.json');
const SAVE_DELAY_MS = 1000;
// これより前に読んだ値は、ファイルに残さない。
const KEEP_MS = 14 * 24 * 60 * 60 * 1000;
// maxAge の無い値を裏で読み直せなかったとき、次に試すまで空ける時間。
const RETRY_MS = 60_000;

/** at は読んだ時刻。input は値を作った材料のハッシュ。 */
type Entry = { value: unknown; at: number; input?: string };

/**
 * maxAge は、これより古い値を読み直す時間（ミリ秒）。無ければ時間では古くならない。
 * input は値を作った材料で、覚えた値の材料と違えば読み直す。
 * wait は、古い値を返さずに、読み直し終えるのを待つ。古い値では役に立たないときに使う。
 * fresh は、人間が「更新」を押したときに渡す。覚えた値が古くなくても読み直し、読み終えるのを待つ。
 * keep は、覚えた値があれば、材料や時間が変わっても読み直さない。今は作り直さなくてよいときに使う。
 */
export type Policy = { maxAge?: number; input?: string; wait?: boolean; fresh?: boolean; keep?: boolean };

const readFile = (): Map<string, Entry> => {
  try {
    return new Map(Object.entries(JSON.parse(fs.readFileSync(FILE, 'utf8')) as Record<string, Entry>));
  } catch {
    // 無いか壊れていれば、空から覚え直す。覚えた値は読み直せば戻る。
    return new Map();
  }
};

const entries = readFile();
// 読み込み中のもの。キーと材料の組みごとに 1 つにまとめる。
const running = new Map<string, Promise<unknown>>();
const failedAt = new Map<string, number>();
// 起動し直しても、画面が覚えている前の版と重ならないよう、起動した時刻から数える。
let version = Date.now();
let saveTimer: NodeJS.Timeout | undefined;

export const saveCache = (): void => {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  const since = Date.now() - KEEP_MS;
  fs.mkdirSync(DATA_DIR, { recursive: true });
  const tmp = `${FILE}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(Object.fromEntries([...entries].filter(([, e]) => e.at > since))));
  fs.renameSync(tmp, FILE);
};

const scheduleSave = (): void => {
  saveTimer ??= setTimeout(saveCache, SAVE_DELAY_MS).unref();
};

/** 画面が持っている値が古くなったことを知らせ、取り直させる。 */
const bump = (): void => {
  version++;
  changed();
};

const digest = (input: string | undefined): string | undefined => input && createHash('sha1').update(input).digest('base64url');

const isStale = (e: Entry, p: Policy): boolean => !!p.fresh || (!p.keep && (e.input !== digest(p.input) || Date.now() - e.at > (p.maxAge ?? Infinity)));

/**
 * load を呼んで覚える。同じキーと材料の読み込みが進んでいれば、それを待つ。fresh なら、進んでいる読み込みより後に始めたものを覚える。
 * 覚えるのは、読み込みが登録されたままのときだけにする。forgetCached の前に始めた読み込みの、書き込む前の値を覚えないため。
 * ponytail: 材料の違う読み込みが並ぶと、後に終わったほうを覚える。古い材料のほうが後に終わると、次に読むときにもう一度読み直す。困ったら、始めた順の新しいほうだけを覚える。
 */
const reload = <T,>(key: string, p: Policy, load: () => Promise<T>): Promise<T> => {
  const input = digest(p.input);
  const id = `${key}\0${input ?? ''}`;
  const current = running.get(id);
  if (current && !p.fresh) return current as Promise<T>;
  const task: Promise<T> = load().then(
    (value) => {
      if (running.get(id) !== task) return value;
      const before = entries.get(key);
      entries.set(key, { value, at: Date.now(), input });
      failedAt.delete(key);
      scheduleSave();
      // 無い値を待たずに返した画面にも、届いた値を取り直させる。
      if (!before || before.input !== input || JSON.stringify(before.value) !== JSON.stringify(value)) bump();
      return value;
    },
    (e: unknown) => {
      failedAt.set(key, Date.now());
      throw e;
    },
  );
  running.set(id, task);
  const done = (): void => void (running.get(id) === task && running.delete(id));
  task.then(done, done);
  return task;
};

/**
 * 裏で読み直す。失敗したら、maxAge か RETRY_MS が過ぎるまで試さない。スナップショットを作るたびに外部へ問い合わせが積もらないようにする。
 * 読めなかったことも画面に知らせ、読み込み中と見せていた表示を改めさせる。
 */
const refreshLater = (key: string, p: Policy, load: () => Promise<unknown>): void => {
  if (Date.now() - (failedAt.get(key) ?? -Infinity) < (p.maxAge ?? RETRY_MS)) return;
  reload(key, p, load).catch((e: unknown) => {
    console.error(`${key} を読み直せません: ${(e as Error).message?.split('\n')[0]}`);
    bump();
  });
};

/**
 * key で覚えた値を返す。古ければ、覚えた値を返してから裏で読み直す。
 * 覚えた値が無いときと、wait か fresh のときは、読み終えるのを待つ。
 */
export const cached = async <T,>(key: string, p: Policy, load: () => Promise<T>): Promise<T> => {
  const e = entries.get(key);
  if (e && !isStale(e, p)) return e.value as T;
  if (!e || p.wait || p.fresh) return reload(key, p, load);
  refreshLater(key, p, load);
  return e.value as T;
};

/** スナップショットのように待てないところで使う。覚えた値をすぐに返し、無いか古ければ裏で読む。 */
export const peekCached = <T,>(key: string, p: Policy, load: () => Promise<T>): T | undefined => {
  const e = entries.get(key);
  if (!e || isStale(e, p)) refreshLater(key, p, load);
  return e?.value as T | undefined;
};

/** 外部に書き込んだ後に、その値を忘れる。文字列は同じキー、正規表現は合うキーを忘れ、次に読むときは読み終えるのを待たせる。 */
export const forgetCached = (...keys: (string | RegExp)[]): void => {
  const match = (key: string): boolean => keys.some((k) => (typeof k === 'string' ? k === key : k.test(key)));
  for (const key of entries.keys()) if (match(key)) entries.delete(key);
  for (const id of running.keys()) if (match(id.slice(0, id.indexOf('\0')))) running.delete(id);
  scheduleSave();
  bump();
};

/** key を、材料 input で読み直している最中か。返した値が古いことを、画面に添えるのに使う。 */
export const refreshing = (key: string, input?: string): boolean => running.has(`${key}\0${digest(input) ?? ''}`);

/** 覚えた値が増えたか変わったときと、裏で読めなかったときに増える。画面はこれが変わったら取り直す。 */
export const cacheVersion = (): number => version;
