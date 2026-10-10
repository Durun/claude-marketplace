// HTML の資料を、ヘッドレスの Chrome で PDF と PNG に書き出す。全体を 1 枚に撮る PNG は Chrome の起動オプションでは作れないので、DevTools Protocol で操作する。
import { type ChildProcess, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { UpstreamError } from './haiku.ts';
import { platform } from './platform.ts';

export type ExportFormat = 'pdf' | 'png';

const CHROME = process.env.CHROME_PATH ?? platform.defaultChromePath(process.env);
const TIMEOUT_MS = 60_000;
// 画面の資料と同じ幅で組む。
const VIEWPORT = { width: 1200, height: 800 };

/** Chrome が標準エラーに書く DevTools の待ち受けポートを待つ。 */
const devtoolsPort = (chrome: ChildProcess): Promise<number> =>
  new Promise((resolve, reject) => {
    let err = '';
    chrome.stderr?.on('data', (chunk: Buffer) => {
      err += chunk.toString();
      const m = /DevTools listening on ws:\/\/[^:]+:(\d+)\//.exec(err);
      if (m) resolve(Number(m[1]));
    });
    chrome.on('error', (e) => reject(new UpstreamError(`Chrome を起動できません（${e.message}）`)));
    chrome.on('exit', () => reject(new UpstreamError(err.trim() || 'Chrome が終了しました')));
  });

/** 1 つのタブに DevTools Protocol の命令を送る。 */
const connect = async (port: number) => {
  const targets = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()) as { type: string; webSocketDebuggerUrl: string }[];
  const page = targets.find((t) => t.type === 'page');
  if (!page) throw new UpstreamError('Chrome のタブが見つかりません');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = () => reject(new UpstreamError('Chrome に接続できません'));
  });
  let next = 0;
  const pending = new Map<number, (msg: { result?: Record<string, unknown>; error?: { message: string } }) => void>();
  const events = new Map<string, () => void>();
  ws.onmessage = (e) => {
    const msg = JSON.parse(String(e.data));
    if (msg.id !== undefined) pending.get(msg.id)?.(msg);
    else events.get(msg.method)?.();
  };
  const send = (method: string, params: object = {}): Promise<Record<string, unknown>> =>
    new Promise((resolve, reject) => {
      const id = ++next;
      pending.set(id, (msg) => (msg.error ? reject(new UpstreamError(msg.error.message)) : resolve(msg.result ?? {})));
      ws.send(JSON.stringify({ id, method, params }));
    });
  const once = (method: string): Promise<void> => new Promise((resolve) => events.set(method, resolve));
  return { send, once, close: () => ws.close() };
};

const render = async (port: number, file: string, format: ExportFormat): Promise<Buffer> => {
  const tab = await connect(port);
  try {
    await tab.send('Page.enable');
    await tab.send('Emulation.setDeviceMetricsOverride', { ...VIEWPORT, deviceScaleFactor: format === 'png' ? 2 : 1, mobile: false });
    const loaded = tab.once('Page.loadEventFired');
    await tab.send('Page.navigate', { url: pathToFileURL(file).href });
    await loaded;
    // 背景色を落とすと、暗い背景に白い文字の資料が読めなくなる。
    if (format === 'pdf') return Buffer.from(String((await tab.send('Page.printToPDF', { printBackground: true })).data), 'base64');
    const { cssContentSize } = (await tab.send('Page.getLayoutMetrics')) as { cssContentSize: { width: number; height: number } };
    const clip = { x: 0, y: 0, width: cssContentSize.width, height: cssContentSize.height, scale: 1 };
    return Buffer.from(String((await tab.send('Page.captureScreenshot', { format: 'png', clip, captureBeyondViewport: true })).data), 'base64');
  } finally {
    tab.close();
  }
};

/** 資料ごとに Chrome を起動して閉じる。書き出しは人間がボタンを押したときだけなので、起動したままにはしない。 */
export const exportHtml = async (html: string, format: ExportFormat): Promise<Buffer> => {
  if (!CHROME) throw new UpstreamError('Chrome の場所が分かりません。CHROME_PATH で指してください');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-export-'));
  const file = path.join(dir, 'doc.html');
  fs.writeFileSync(file, html);
  const chrome = spawn(CHROME, ['--headless', '--remote-debugging-port=0', `--user-data-dir=${path.join(dir, 'profile')}`, '--no-first-run', 'about:blank']);
  const exited = new Promise((resolve) => chrome.once('exit', resolve));
  const timer = setTimeout(() => chrome.kill(), TIMEOUT_MS);
  try {
    return await render(await devtoolsPort(chrome), file, format);
  } finally {
    clearTimeout(timer);
    chrome.kill();
    // 閉じ終わる前にプロファイルを消すと、Chrome が書き足したファイルが残る。
    await exited;
    fs.rmSync(dir, { recursive: true, force: true });
  }
};
