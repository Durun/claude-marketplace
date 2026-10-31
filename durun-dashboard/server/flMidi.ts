// FL Connect の ① の道。ループバックの MIDI ポート（既定は FL Connect (B)）を開き、FL の中のスクリプト（fl/device_FLConnect.py）と SysEx でやりとりする。
// メッセージは F0 7D 'F' 'C' <向き> <番号> <何通目> <全部で何通> <JSON> F7。番号などの数は 7 ビットずつの 2 バイト。
// FL のスクリプトと形をそろえるので、片方を変えたらもう片方も変える。
import type { Input, Output } from '@julusian/midi';

/** MIDI の規格で、商用でない使い方のために空けてあるメーカーの番号（0x7D）と、FL Connect の印の 'F' 'C'。 */
export const HEADER = [0x7d, 0x46, 0x43] as const;
export const DIRECTION = { request: 1, response: 2, heartbeat: 3, notice: 4 } as const;
/** Windows の MIDI は長い SysEx を捨てることがあるので、1 通の中身はこの大きさまでにする。 */
export const CHUNK = 240;
/** 番号と通の数は 7 ビットずつの 2 バイトで表すので、この数まで。 */
const MAX_U14 = 0x3fff;
/** 分けて届いたものを、最後の 1 通を待つのをあきらめるまでの時間。 */
const PARTIAL_MS = 5000;

const u14 = (n: number): [number, number] => [(n >> 7) & 0x7f, n & 0x7f];

/** SysEx のデータは 0x80 未満でなければならないので、JSON の ASCII でない文字（と DEL）を \u の形にする。 */
export const asciiJson = (value: unknown): string =>
  JSON.stringify(value).replace(/[\u007f-￿]/g, (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, '0')}`);

/** payload を JSON にし、chunk バイトずつの SysEx に分ける。 */
export const encodeMessages = (direction: number, id: number, payload: unknown, chunk = CHUNK): number[][] => {
  const body = Buffer.from(asciiJson(payload), 'ascii');
  const total = Math.max(1, Math.ceil(body.length / chunk));
  if (total > MAX_U14) throw new Error(`送る中身が大きすぎます（${body.length} バイト）`);
  return Array.from({ length: total }, (_, i) => [0xf0, ...HEADER, direction, ...u14(id), ...u14(i), ...u14(total), ...body.subarray(i * chunk, (i + 1) * chunk), 0xf7]);
};

export type Frame = { direction: number; id: number; index: number; total: number; body: Buffer };

/** FL Connect の SysEx の 1 通を読む。ほかの MIDI のメッセージなら undefined。 */
export const parseFrame = (bytes: readonly number[] | Buffer): Frame | undefined => {
  if (bytes.length < 12 || bytes[0] !== 0xf0 || bytes[1] !== HEADER[0] || bytes[2] !== HEADER[1] || bytes[3] !== HEADER[2]) return undefined;
  const at = (i: number): number => bytes[i] ?? 0;
  const end = bytes[bytes.length - 1] === 0xf7 ? bytes.length - 1 : bytes.length;
  return { direction: at(4), id: (at(5) << 7) | at(6), index: (at(7) << 7) | at(8), total: (at(9) << 7) | at(10), body: Buffer.from(bytes.slice(11, end)) };
};

export type Assembled = { direction: number; id: number; value: unknown };

/**
 * 分けて届いた SysEx を、向きと番号ごとに溜め、全部そろったら JSON を読んで返す。
 * FL は 1 つのメッセージを続けて送るので、別のメッセージと混ざらない。先頭の 1 通がまた届いたら、前の溜まりは抜けがあったものとして捨てる。
 */
export class Assembler {
  #partial = new Map<string, { at: number; chunks: Buffer[]; got: number; total: number }>();

  push(frame: Frame, now = Date.now()): Assembled | undefined {
    for (const [key, p] of this.#partial) if (now - p.at > PARTIAL_MS) this.#partial.delete(key);
    const key = `${frame.direction}:${frame.id}`;
    let p = this.#partial.get(key);
    if (!p || frame.index === 0 || p.total !== frame.total) {
      p = { at: now, chunks: [], got: 0, total: frame.total };
      this.#partial.set(key, p);
    }
    if (!p.chunks[frame.index]) p.got += 1;
    p.chunks[frame.index] = frame.body;
    p.at = now;
    if (p.got < p.total) return undefined;
    this.#partial.delete(key);
    return { direction: frame.direction, id: frame.id, value: JSON.parse(Buffer.concat(p.chunks).toString('ascii')) };
  }
}

export type MidiPorts = { input: string; output: string };

export type MidiListener = {
  /** FL のスクリプトからの、変化の印や名乗りの知らせ。 */
  onNotice: (value: Record<string, unknown>) => void;
  onHeartbeat: () => void;
};

export type MidiLink = {
  /** 要求を送り、応答を待つ。timeoutMs のうちに返らなければ例外を投げる。 */
  request: (payload: Record<string, unknown>, timeoutMs: number) => Promise<Record<string, unknown>>;
  close: () => void;
};

/** 名前が name で始まるポートの番号。Windows の MIDI は、同じ名前のポートに番号を添えることがある。 */
const portIndex = (names: string[], name: string): number => {
  const exact = names.indexOf(name);
  return exact >= 0 ? exact : names.findIndex((n) => n.startsWith(name));
};

/** ポートが無ければ、その名前を添えた例外を投げる。ネイティブのモジュールは、ほかの機能を巻き込まないよう、開くときに読む。 */
export const openMidi = async (ports: MidiPorts, listener: MidiListener): Promise<MidiLink> => {
  const midi = await import('@julusian/midi');
  const input: Input = new midi.Input();
  const output: Output = new midi.Output();
  const close = (): void => {
    input.closePort();
    output.closePort();
    input.destroy();
    output.destroy();
  };
  try {
    const inIndex = portIndex(midi.Input.getPortNames(), ports.input);
    const outIndex = portIndex(midi.Output.getPortNames(), ports.output);
    if (inIndex < 0 || outIndex < 0) throw new Error(`MIDI のポート「${inIndex < 0 ? ports.input : ports.output}」がありません`);
    // SysEx は既定で読み捨てられるので、受け取るようにする。
    input.ignoreTypes(false, true, true);
    input.openPort(inIndex);
    output.openPort(outIndex);
  } catch (e) {
    close();
    throw e;
  }
  const assembler = new Assembler();
  const waiting = new Map<number, (value: Record<string, unknown>) => void>();
  input.on('message', (_dt, message) => {
    const frame = parseFrame(message);
    if (!frame) return;
    let done: Assembled | undefined;
    try {
      done = assembler.push(frame);
    } catch {
      // 読めない JSON は、抜けや壊れたメッセージなので捨てる。待っている要求は時間切れになる。
      return;
    }
    if (!done) return;
    const value = (done.value ?? {}) as Record<string, unknown>;
    if (done.direction === DIRECTION.response) waiting.get(done.id)?.(value);
    else if (done.direction === DIRECTION.heartbeat) listener.onHeartbeat();
    else if (done.direction === DIRECTION.notice) listener.onNotice(value);
  });
  let seq = 0;
  const request = (payload: Record<string, unknown>, timeoutMs: number): Promise<Record<string, unknown>> =>
    new Promise((resolve, reject) => {
      seq = (seq % MAX_U14) + 1;
      const id = seq;
      const timer = setTimeout(() => {
        waiting.delete(id);
        reject(new Error('FL Connect のスクリプトが応答しません'));
      }, timeoutMs);
      waiting.set(id, (value) => {
        clearTimeout(timer);
        waiting.delete(id);
        resolve(value);
      });
      try {
        for (const m of encodeMessages(DIRECTION.request, id, payload)) output.sendMessage(m);
      } catch (e) {
        clearTimeout(timer);
        waiting.delete(id);
        reject(e as Error);
      }
    });
  return { request, close };
};
