// アプリ「翻訳」。PFN の PLaMo 3 Translate 0.8B を、llama.cpp の llama-server で手元の PC で動かす。
// llama.cpp とモデルは同梱せず、画面の導入ボタンで入れる。モデルは GGUF の Q4_K_M（0.57 GB）にし、モデルカードが最初に勧める精度と大きさの釣り合いを取る。
import { type ChildProcess, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { pipeline } from 'node:stream/promises';
import type { TranslateStatus } from '../shared/types.ts';
import { installTool, resolveTool } from './cdripTools.ts';
import { platform } from './platform.ts';
import { DATA_DIR } from './store.ts';

const MODEL_NAME = 'plamo-3-translate-0.8b-Q4_K_M.gguf';
const MODEL_URL = `https://huggingface.co/pfnet/plamo-3-translate-0.8b-GGUF/resolve/main/${MODEL_NAME}`;
const MODEL_FILE = path.join(DATA_DIR, 'models', MODEL_NAME);
// 入力と出力の合わせた長さの上限。学習時の上限は 32,768 トークンで、モデルカードの例に合わせて半分にし、メモリを抑える。
const CONTEXT = 16_384;
// 使わない間もメモリを 1 GB ほど占めるので、しばらく使わなければ止め、次の翻訳で起動し直す。起動は数秒で済む。
const IDLE_MS = 10 * 60_000;
const START_MS = 60_000;

export class TranslateError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

let installing: Promise<{ message: string }> | undefined;

export const translateStatus = (): TranslateStatus => ({
  runtime: !!resolveTool('llama-server'),
  model: fs.existsSync(MODEL_FILE),
  installing: !!installing,
  install: platform.toolInstall('llama-server').command,
});

// 途中で切れたファイルをモデルと取り違えないよう、別名に書き終えてから名前を変える。
const downloadModel = async (): Promise<void> => {
  const res = await fetch(MODEL_URL);
  if (!res.ok || !res.body) throw new TranslateError(502, `モデルを取得できませんでした（HTTP ${res.status}）`);
  fs.mkdirSync(path.dirname(MODEL_FILE), { recursive: true });
  const part = `${MODEL_FILE}.part`;
  await pipeline(res.body, fs.createWriteStream(part));
  fs.renameSync(part, MODEL_FILE);
};

/** 足りないものを入れる。入れている途中に呼ばれたら、同じ導入が終わるのを待つ。画面を読み込み直しても、終わりを受け取れるようにするため。 */
export const installTranslate = (): Promise<{ message: string }> =>
  (installing ??= (async () => {
    try {
      const lines: string[] = [];
      if (!resolveTool('llama-server')) {
        const { message, ran } = await installTool('llama-server');
        lines.push(message);
        if (!ran) return { message: lines.join('\n') };
      }
      if (!fs.existsSync(MODEL_FILE)) {
        await downloadModel();
        lines.push('PLaMo 3 Translate 0.8B を入れました。');
      }
      return { message: lines.join('\n') || '入れるものはありません。' };
    } finally {
      installing = undefined;
    }
  })());

type Running = { child: ChildProcess; port: number };
let server: Promise<Running> | undefined;
let idle: NodeJS.Timeout | undefined;

const freePort = (): Promise<number> =>
  new Promise((resolve, reject) => {
    const s = net.createServer();
    s.on('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address() as net.AddressInfo;
      s.close(() => resolve(port));
    });
  });

const stopServer = (): void => {
  void server?.then(({ child }) => child.kill());
  server = undefined;
};

process.on('exit', stopServer);

/** llama-server を起動し、待ち受けを始めたと出力に書くまで待つ。 */
const startServer = async (bin: string): Promise<Running> => {
  const port = await freePort();
  const child = spawn(bin, ['-m', MODEL_FILE, '-c', String(CONTEXT), '-np', '1', '--host', '127.0.0.1', '--port', String(port)], { stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new TranslateError(504, '翻訳のモデルの起動を待ちきれませんでした')), START_MS);
    const onData = (chunk: Buffer) => {
      log = (log + chunk.toString()).slice(-2000);
      if (log.includes('listening on')) {
        clearTimeout(timer);
        resolve();
      }
    };
    child.stdout?.on('data', onData);
    child.stderr?.on('data', onData);
    child.on('error', (e) => reject(new TranslateError(500, e.message)));
    child.on('exit', (code) => reject(new TranslateError(500, `llama-server が止まりました（${code}）\n${log.trim().split('\n').slice(-4).join('\n')}`)));
  }).catch((e: unknown) => {
    child.kill();
    throw e;
  });
  // 待ち受けを始めた後は出力を読まない。読まずにためると、パイプが詰まって止まる。
  child.stdout?.resume().removeAllListeners('data');
  child.stderr?.resume().removeAllListeners('data');
  return { child, port };
};

const ensureServer = (): Promise<Running> => {
  const bin = resolveTool('llama-server');
  if (!bin || !fs.existsSync(MODEL_FILE)) throw new TranslateError(409, '翻訳のモデルが入っていません');
  if (server) return server;
  const started = startServer(bin);
  server = started;
  // 止まったら次の翻訳で起動し直す。止めた後に起動し直した分は消さない。
  const forget = () => {
    if (server === started) server = undefined;
  };
  started.then(({ child }) => child.on('exit', forget), forget);
  return started;
};

/** PLaMo 3 Translate の PFML3 の形。訳す先の言語だけを指し、原文の言語はモデルに見分けさせる。 */
export const plamoPrompt = (text: string, target: string): string =>
  `<|plamo:tag|>dataset<|plamo:msg|>translation<|plamo:tag|>input<|plamo:msg|>${text}<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>${target}<|plamo:msg|>`;

const LANG = /^[a-z]{2,3}(-[a-z]+)?$/;

export const translate = async (text: string, target: string): Promise<{ text: string }> => {
  if (!LANG.test(target)) throw new TranslateError(400, `知らない言語です: ${target}`);
  if (!text.trim()) return { text: '' };
  clearTimeout(idle);
  const { port } = await ensureServer();
  try {
    const res = await fetch(`http://127.0.0.1:${port}/v1/completions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ prompt: plamoPrompt(text, target), temperature: 0, max_tokens: CONTEXT / 2, stop: ['<|plamo:tag|>', '<|plamo:bos|>'] }),
    });
    const body = (await res.json()) as { choices?: { text: string }[]; error?: { message: string } };
    if (!res.ok) throw new TranslateError(502, body.error?.message ?? `HTTP ${res.status}`);
    return { text: body.choices?.[0]?.text.trim() ?? '' };
  } finally {
    idle = setTimeout(stopServer, IDLE_MS).unref();
  }
};
