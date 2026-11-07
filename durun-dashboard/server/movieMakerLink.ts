// movie-maker の mm-serve（HTTP、SSE、MCP）とやりとりする部品。動いているアプリの見つけ方は mm-store の lock.rs と token.rs、
// HTTP の形は movie-maker/docs/design/05-ops.md の「HTTP サーバー」に従う。ここの関数はダッシュボードの状態に触れないので、テストで確かめる。
import fs from 'node:fs';
import path from 'node:path';
import type { MmChange } from '../shared/types.ts';

/** 書き手が `.mm/lock.info` に書く情報。トークンそのものは持たず、置き場所の識別子だけを持つ。 */
export type LockInfo = { pid: number; http_port?: number | null; token_id?: string | null };

/** 動いている書き手につなぐのに要るもの。 */
export type LinkTarget = { pid: number; port: number; token: string };

export const MM_DIR = '.mm';
export const STORE_FILE = 'project.mmdb';
const INFO_FILE = 'lock.info';

const readText = (file: string): string | undefined => {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return undefined;
  }
};

export const readLockInfo = (projectDir: string): LockInfo | undefined => {
  const text = readText(path.join(projectDir, MM_DIR, INFO_FILE));
  if (!text) return undefined;
  try {
    const v = JSON.parse(text) as LockInfo;
    return typeof v.pid === 'number' ? v : undefined;
  } catch {
    return undefined;
  }
};

/** pid のプロセスが生きているか。シグナル 0 は送らずに確かめるだけ。権限が無くて断られたときも、プロセスはある。 */
export const pidAlive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
};

/**
 * プロジェクトのフォルダーから、動いている書き手の HTTP サーバーを探す。つなげなければ、その理由を返す。
 * クラッシュで残った lock.info を信じないよう、書き手の pid が生きていることを先に確かめる（mm-serve の Client::for_project と同じ順）。
 */
export const findLink = (projectDir: string, tokenDir: string | undefined, alive: (pid: number) => boolean = pidAlive): LinkTarget | { reason: string } => {
  const info = readLockInfo(projectDir);
  if (!info) return { reason: 'movie-maker がこのプロジェクトを開いていません。' };
  if (!alive(info.pid)) return { reason: 'movie-maker が終了しています（前の lock.info が残っています）。' };
  if (!info.http_port || !info.token_id) return { reason: 'movie-maker は開いていますが、HTTP サーバーがまだ動いていません。' };
  const token = tokenDir && readText(path.join(tokenDir, info.token_id))?.trim();
  if (!token) return { reason: 'movie-maker の HTTP のトークンが読めません。' };
  return { pid: info.pid, port: info.http_port, token };
};

/** change を送ったのが人間か。mm-serve は WebView とサーフェスの操作を `gui` で送り、HTTP の操作経路（mcp、cli）は人間ではない。 */
export const isHuman = (change: MmChange): boolean => change.actor === 'gui';

/** フックとツールで返す、1 件の生の値。 */
export const changeLine = (change: MmChange): string => JSON.stringify(change);

export const CHANGE_FORMAT = '1 行に 1 件、movie-maker の GET /events の change のまま。kind は apply、undo、redo で、undo と redo の of は戻した元のトランザクション。actor の gui は人間、mcp と cli はエージェント';

/** 人間の操作を、フックの additionalContext の文にする。limit を超えた分は数だけ添える。 */
export const hookText = (changes: MmChange[], limit: number): string | undefined => {
  if (!changes.length) return undefined;
  const shown = changes.slice(-limit);
  const rest = changes.length > shown.length ? [`ほかに ${changes.length - shown.length} 件あった。mm_changes に all: true を渡すと読める。`] : [];
  return [`ムービーメーカー: 前に知らせてから、人間が movie-maker で次の操作をした（${CHANGE_FORMAT}）。`, ...shown.map(changeLine), ...rest].join('\n');
};

type Rational = { num: number; den: number };
const sec = (r: unknown): string => {
  const v = r as Rational | undefined;
  return v && typeof v.num === 'number' && v.den ? String(Math.round((v.num / v.den) * 1000) / 1000) : '?';
};

type Layer = { id: string; source?: { kind?: string }; in?: Rational; out?: Rational; lock?: boolean };
type Composition = { id: string; width?: number; height?: number; fps?: Rational; duration?: Rational; layers?: Layer[] };
type ProjectBody = { compositions?: Composition[]; sequences?: { id: string; cuts?: unknown[] }[]; assets?: { id: string; path?: string }[] };
export type ProjectView = { version: number; undo_depth: number; project: ProjectBody };

/** GET /project の中身を、mm_state で返す短い文にする。全部の値は mm_tool の project で読める。 */
export const projectSummary = (view: ProjectView): string => {
  const p = view.project ?? {};
  const lines = [`version ${view.version}、取り消せるトランザクション ${view.undo_depth}`];
  for (const c of p.compositions ?? []) {
    lines.push(`composition ${c.id}: ${c.width}x${c.height}、${sec(c.fps)} fps、${sec(c.duration)} 秒`);
    for (const l of c.layers ?? []) lines.push(`  layer ${l.id}: ${l.source?.kind ?? '?'}、${sec(l.in)}〜${sec(l.out)} 秒${l.lock ? '、ロック' : ''}`);
  }
  for (const s of p.sequences ?? []) lines.push(`sequence ${s.id}: カット ${s.cuts?.length ?? 0} 個`);
  const assets = p.assets ?? [];
  if (assets.length) lines.push(`素材 ${assets.length} 個: ${assets.map((a) => `${a.id}（${a.path ?? '?'}）`).join('、')}`);
  return lines.join('\n');
};

/** MCP の tools/call の結果。content は text と image をそのまま持つ。 */
export type ToolContent = { type: string; text?: string; data?: string; mimeType?: string };
export type ToolResult = { content: ToolContent[]; isError?: boolean };
export type ToolInfo = { name: string; description?: string; inputSchema?: { properties?: Record<string, unknown> } };

/** SSE の流れを、イベントの名前とデータの組に分ける。途中で切れた行は次の塊まで持つ。 */
export class SseParser {
  private buffer = '';

  push(chunk: string): { event: string; data: string }[] {
    this.buffer += chunk.replaceAll('\r\n', '\n');
    const out: { event: string; data: string }[] = [];
    let end = this.buffer.indexOf('\n\n');
    while (end >= 0) {
      const block = this.buffer.slice(0, end);
      this.buffer = this.buffer.slice(end + 2);
      let event = 'message';
      const data: string[] = [];
      for (const line of block.split('\n')) {
        if (line.startsWith('event:')) event = line.slice(6).trim();
        else if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
      }
      if (data.length) out.push({ event, data: data.join('\n') });
      end = this.buffer.indexOf('\n\n');
    }
    return out;
  }
}

/** 動いている mm-serve への接続。トークンは Authorization: Bearer で渡す。 */
export class MmClient {
  private nextId = 1;
  readonly port: number;
  private readonly token: string;

  constructor(port: number, token: string) {
    this.port = port;
    this.token = token;
  }

  private url(p: string): string {
    return `http://127.0.0.1:${this.port}${p}`;
  }

  private headers(extra: Record<string, string> = {}): Record<string, string> {
    return { authorization: `Bearer ${this.token}`, ...extra };
  }

  async project(signal?: AbortSignal): Promise<ProjectView> {
    const res = await fetch(this.url('/project'), { headers: this.headers(), signal });
    if (!res.ok) throw new Error(`GET /project が ${res.status} を返しました`);
    return (await res.json()) as ProjectView;
  }

  /** mm-serve の MCP（POST /mcp）を JSON-RPC で呼ぶ。mm-serve の MCP は状態を持たないので、initialize を挟まない。 */
  async rpc<T>(method: string, params: Record<string, unknown>, signal?: AbortSignal): Promise<T> {
    const res = await fetch(this.url('/mcp'), {
      method: 'POST',
      headers: this.headers({ 'content-type': 'application/json', accept: 'application/json, text/event-stream' }),
      body: JSON.stringify({ jsonrpc: '2.0', id: this.nextId++, method, params }),
      signal,
    });
    if (!res.ok) throw new Error(`movie-maker の MCP が ${res.status} を返しました: ${(await res.text()).slice(0, 500)}`);
    const body = (await res.json()) as { result?: T; error?: { message?: string } };
    if (body.error) throw new Error(`movie-maker の MCP のエラー: ${body.error.message ?? JSON.stringify(body.error)}`);
    return body.result as T;
  }

  async tools(signal?: AbortSignal): Promise<ToolInfo[]> {
    return (await this.rpc<{ tools: ToolInfo[] }>('tools/list', {}, signal)).tools ?? [];
  }

  callTool(name: string, args: Record<string, unknown>, signal?: AbortSignal): Promise<ToolResult> {
    return this.rpc<ToolResult>('tools/call', { name, arguments: args }, signal);
  }

  /** GET /events を購読する。流れが終わるか切れたら返る。 */
  async events(onEvent: (event: string, data: unknown) => void, signal: AbortSignal): Promise<void> {
    const res = await fetch(this.url('/events'), { headers: this.headers({ accept: 'text/event-stream' }), signal });
    if (!res.ok || !res.body) throw new Error(`GET /events が ${res.status} を返しました`);
    const parser = new SseParser();
    const decoder = new TextDecoder();
    for await (const chunk of res.body) {
      for (const { event, data } of parser.push(decoder.decode(chunk as Uint8Array, { stream: true }))) {
        try {
          onEvent(event, JSON.parse(data));
        } catch {
          onEvent(event, data);
        }
      }
    }
  }
}
