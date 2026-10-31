// イルカへの音声入力。マイクを ON にしている間は聞き続け、呼びかけで始まる発言をイルカへの頼みとして渡す。
// 音声は Chrome が Mac の中で書き起こし、読み上げも Mac の声を使うので、話した内容をブラウザの外へ送らない。
// ON にしたかと呼びかけの言葉は、ブラウザに保存する。
import { useEffect, useState } from 'preact/hooks';

const MIC_KEY = 'dolphin-mic';
const WORDS_KEY = 'dolphin-wake-words';
const DEFAULT_WORDS = ['イルカ', 'ねえイルカ'];
const LANG = 'ja-JP';
// 呼びかけだけで黙ったときや、イルカが答えた後に、呼びかけなしで続きを話せる時間（ms）。送る指示に「送って」と答えるのもこの間にする。
const FOLLOW_UP_MS = 8000;
// 認識が止まってから聞き直すまでの間（ms）。止まり続けるときに、聞き直しを繰り返して固まらないようにする。
const RESTART_MS = 1000;

/** listening は、呼びかけを聞いて続きを待っている間で、text は話している途中の文字。ask は頼みの全文。 */
export type VoiceEvent = { kind: 'listening'; text: string } | { kind: 'idle' } | { kind: 'ask'; text: string } | { kind: 'error'; message: string };

/** 使う分だけの、Chrome の SpeechRecognition。型定義には、Mac の中で認識するための processLocally と available、install がまだ無い。 */
type Recognizer = {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  processLocally: boolean;
  onresult: ((e: SpeechRecognitionEvent) => void) | null;
  onerror: ((e: SpeechRecognitionErrorEvent) => void) | null;
  onend: (() => void) | null;
  start(): void;
  abort(): void;
};
type LocalOptions = { langs: string[]; processLocally: boolean };
type RecognizerClass = {
  new (): Recognizer;
  available?: (o: LocalOptions) => Promise<'available' | 'downloadable' | 'downloading' | 'unavailable'>;
  install?: (o: LocalOptions) => Promise<boolean>;
};

const NO_JAPANESE = 'この Chrome は、Mac の中で日本語を認識できません';

/** 聞き続けても直らないエラー。ほかのエラー（無音など）は、止まった後に聞き直す。 */
const FATAL: Partial<Record<SpeechRecognitionErrorCode, string>> = {
  'not-allowed': 'マイクを使う許可がありません',
  'service-not-allowed': 'マイクを使う許可がありません',
  'audio-capture': 'マイクが見つかりません',
  'language-not-supported': NO_JAPANESE,
};

const load = <T,>(key: string, fallback: T): T => {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
};

const save = (key: string, value: unknown): void => {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // 保存できなくても、開いている間は使える。
  }
};

/** 呼びかけと発言を比べるための形。カタカナをひらがなにし、全角と半角、大文字と小文字、空白と句読点の違いを消す。 */
export const fold = (s: string): string =>
  s
    .normalize('NFKC')
    .toLowerCase()
    .replace(/[\s、。,.!?「」『』()・]/g, '')
    .replace(/[ァ-ヶ]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0x60));

/** text が呼びかけのどれかで始まれば、呼びかけの後の発言を返す。呼びかけだけなら空、始まらなければ undefined。長い呼びかけから比べる。 */
export const afterWake = (text: string, words: string[]): string | undefined => {
  const chars = [...text];
  for (const word of words.map(fold).filter(Boolean).sort((a, b) => b.length - a.length)) {
    let got = '';
    for (const [i, c] of chars.entries()) {
      got += fold(c);
      if (got === word) return chars.slice(i + 1).join('').replace(/^[\s、。,.!?！？]+/, '');
      if (!word.startsWith(got)) break;
    }
  }
  return undefined;
};

const YES = /^(はい|うん|ええ|おくって|送って|おくる|送る|おねがい|お願い|おっけー|ok|いいよ|どうぞ)/;
const NO = /^(いいえ|いや|やめ|止め|おくらない|送らない|きゃんせる|すとっぷ|だめ|駄目)/;

/** 送る指示を確かめたときの答え。送るなら true、送らないなら false、どちらでもなければ undefined。 */
export const answerOf = (text: string): boolean | undefined => {
  const f = fold(text);
  return YES.test(f) ? true : NO.test(f) ? false : undefined;
};

let mic = load(MIC_KEY, false);
let words = load(WORDS_KEY, DEFAULT_WORDS);
let preparing = false;
let rec: Recognizer | undefined;
let starting = false;
let speaking: SpeechSynthesisUtterance | undefined;
let awake = false;
let awakeTimer: ReturnType<typeof setTimeout> | undefined;
const handlers = new Set<(e: VoiceEvent) => void>();
const watchers = new Set<() => void>();

const emit = (e: VoiceEvent): void => handlers.forEach((fn) => fn(e));
const changed = (): void => watchers.forEach((fn) => fn());

/** 続きを話せるようにする。黙ったまま FOLLOW_UP_MS が過ぎたら、呼びかけを待つ状態に戻る。 */
const wake = (text = ''): void => {
  clearTimeout(awakeTimer);
  awake = true;
  awakeTimer = setTimeout(() => {
    awake = false;
    emit({ kind: 'idle' });
  }, FOLLOW_UP_MS);
  emit({ kind: 'listening', text });
};

const hear = (text: string, final: boolean): void => {
  // 続きを待っている間は、呼びかけを付けずに話してよい。
  const rest = afterWake(text, words) ?? (awake ? text.trim() : undefined);
  if (rest === undefined) return;
  if (!final || !rest) return wake(rest);
  clearTimeout(awakeTimer);
  awake = false;
  emit({ kind: 'ask', text: rest });
};

const resume = (r: Recognizer): void => {
  try {
    r.start();
  } catch {
    // 止めきる前なら、止まったときの onend で聞き直す。
  }
};

const create = (SR: RecognizerClass): Recognizer => {
  const r = Object.assign(new SR(), { lang: LANG, continuous: true, interimResults: true, processLocally: true });
  r.onresult = (e) => {
    for (let i = e.resultIndex; i < e.results.length; i++) {
      const result = e.results.item(i);
      hear(result.item(0).transcript, result.isFinal);
    }
  };
  r.onerror = (e) => {
    const message = FATAL[e.error];
    if (message) fail(message);
  };
  // Chrome は、黙っている間などに聞き続けるのをやめるので、ON の間は聞き直す。読み上げている間は、イルカの声を聞かないよう止めておく。
  r.onend = () => {
    if (rec === r && !speaking) setTimeout(() => rec === r && !speaking && resume(r), RESTART_MS);
  };
  return r;
};

const stop = (): void => {
  const r = rec;
  rec = undefined;
  r?.abort();
  clearTimeout(awakeTimer);
  awake = false;
  emit({ kind: 'idle' });
};

const fail = (message: string): void => {
  setMic(false);
  emit({ kind: 'error', message: `イルカのマイクを OFF にしました（${message}）` });
};

const setPreparing = (on: boolean): void => {
  preparing = on;
  changed();
};

/** 日本語の認識データが無ければ Chrome にダウンロードさせてから、聞き始める。signal は、準備している間にマイクを手放したら立つ。 */
const start = async (signal?: AbortSignal): Promise<void> => {
  const w = window as unknown as { SpeechRecognition?: RecognizerClass };
  const SR = w.SpeechRecognition;
  if (!SR?.available || !SR.install) return fail('この Chrome は、Mac の中で音声を認識できません');
  const options = { langs: [LANG], processLocally: true };
  starting = true;
  try {
    const status = await SR.available(options);
    if (status === 'unavailable') return fail(NO_JAPANESE);
    if (status !== 'available') {
      setPreparing(true);
      const installed = await SR.install(options);
      setPreparing(false);
      if (!installed) return fail('日本語の認識データをダウンロードできませんでした');
    }
    // 準備している間に OFF にされたか、ほかのタブにマイクを渡していたら、聞き始めない。
    if (!mic || handlers.size === 0 || signal?.aborted) return;
    // 読み上げに使う声は、初めて読んだときに読み込み始めるので、答えるときまでに読み込んでおく。
    speechSynthesis.getVoices();
    rec = create(SR);
    rec.start();
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
  } finally {
    starting = false;
    setPreparing(false);
  }
};

let lock: AbortController | undefined;

const drop = (): void => {
  const l = lock;
  lock = undefined;
  l?.abort();
};

/**
 * マイクを使うのは 1 つのタブだけにする。どのタブも同じ呼びかけを聞くので、タブごとに聞くと同じ頼みを重ねて送るため。
 * steal なら、ほかのタブが使っていても引き取る。引き取られたタブは並び直し、引き取ったタブが閉じたら引き継ぐ。
 */
const hold = (steal: boolean): void => {
  drop();
  const abort = new AbortController();
  lock = abort;
  const listen = () =>
    new Promise<void>((release) => {
      void start(abort.signal);
      abort.signal.addEventListener('abort', () => {
        stop();
        release();
      });
    });
  navigator.locks.request(MIC_KEY, steal ? { steal } : { signal: abort.signal }, listen).catch(() => {
    // ほかのタブに引き取られた。並んでいる間に手放したときは、もう次を握っているか、手放したままにする。
    if (lock !== abort) return;
    stop();
    hold(false);
  });
};

/** イルカが居て、マイクが ON の間だけ聞く。 */
const sync = (): void => {
  if (!mic || handlers.size === 0) {
    drop();
    stop();
  } else if (!lock && !rec && !starting) {
    // localhost 以外を http で開くと Web Locks が無い。マイクも使えないので、聞き始めたときに start が知らせる。
    if (navigator.locks) hold(document.hasFocus());
    else void start();
  }
};

// ほかのタブで ON と OFF や呼びかけを変えたら、このタブもそろえる。
const onStorage = (e: StorageEvent): void => {
  if (e.key === MIC_KEY) mic = load(MIC_KEY, false);
  else if (e.key === WORDS_KEY) words = load(WORDS_KEY, DEFAULT_WORDS);
  else return;
  sync();
  changed();
};

// 人間が前に出したタブで聞く。答えや開いた画面を、人間が見ているタブに出すため。
const onFocus = (): void => {
  if (mic && handlers.size > 0 && navigator.locks && !rec && !starting) hold(true);
};

export const setMic = (on: boolean): void => {
  mic = on;
  save(MIC_KEY, on);
  sync();
  changed();
};

/** 「、」や空白で区切った呼びかけを保存する。空にしたら、元の呼びかけに戻す。 */
export const setWakeWords = (text: string): void => {
  const list = text.split(/[、,，\s]+/).filter(Boolean);
  words = list.length ? list : DEFAULT_WORDS;
  save(WORDS_KEY, words);
  changed();
};

export const useMic = (): { on: boolean; preparing: boolean; words: string[] } => {
  const [, render] = useState(0);
  useEffect(() => {
    const fn = () => render((n) => n + 1);
    watchers.add(fn);
    return () => {
      watchers.delete(fn);
    };
  }, []);
  return { on: mic, preparing, words };
};

/** 聞こえた頼みを受け取る。受け取る関数がある間だけ、マイクを使う。止める関数を返す。 */
export const onVoice = (fn: (e: VoiceEvent) => void): (() => void) => {
  if (handlers.size === 0) {
    addEventListener('storage', onStorage);
    addEventListener('focus', onFocus);
  }
  handlers.add(fn);
  sync();
  return () => {
    handlers.delete(fn);
    if (handlers.size === 0) {
      removeEventListener('storage', onStorage);
      removeEventListener('focus', onFocus);
    }
    sync();
  };
};

/**
 * イルカの答えを Mac の日本語の声で読み上げ、読み終えたら続きを聞く。読み上げている間は、イルカの声を頼みと聞き違えないよう聞くのを止める。
 * Google の声は文を Google へ送るので使わない。Mac の声が無ければ読み上げず、続きを聞くだけにする。
 */
export const say = (text: string): void => {
  const ja = speechSynthesis.getVoices().filter((v) => v.localService && v.lang.replace('_', '-') === LANG);
  const voice = ja.find((v) => v.name.startsWith('Kyoko')) ?? ja[0];
  if (!voice) return listenAgain();
  const u = Object.assign(new SpeechSynthesisUtterance(text), { voice, lang: LANG });
  // 読み上げの途中で次を読み始めると、前の文の終わりも届くので、今の文が終わったときだけ聞き直す。
  const done = () => {
    if (speaking !== u) return;
    speaking = undefined;
    listenAgain();
  };
  u.onend = done;
  u.onerror = done;
  speaking = u;
  rec?.abort();
  speechSynthesis.cancel();
  speechSynthesis.speak(u);
};

const listenAgain = (): void => {
  if (!rec) return;
  resume(rec);
  wake();
};
