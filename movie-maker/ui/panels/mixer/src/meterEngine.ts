import type { MixerMeters } from "./api.ts";
import { dbToFraction, formatDb, METER_FLOOR_DB } from "./levels.ts";

/** ピークホールドの保持時間（ms）。 */
export const HOLD_MS = 1500;
/** 表示の下がる速さ（dB/秒）。ホールドの目印も保持の後に同じ速さで下がる。 */
export const DECAY_DB_PER_S = 24;
const MS_PER_S = 1000;
const PERCENT = 100;
/** マスターの棒の数（L、R）。 */
const MASTER_CHANNELS = 2;

interface Channel {
  target: number;
  shown: number;
  hold: number;
  holdAt: number;
}

/** 棒の DOM。`fill` を切り、`hold` の位置を動かす。 */
interface Bar {
  fill: HTMLElement;
  hold: HTMLElement;
  clip: string;
  holdLeft: string;
}

/** 数字の読み出しの種類。 */
export type ReadoutKey = "true_peak" | "short_term" | "integrated";

const dbOrFloor = (db: number | null | undefined): number => (db === null || db === undefined ? METER_FLOOR_DB : Math.max(METER_FLOOR_DB, db));

/**
 * メーターの値を受けて、棒の DOM だけを直接書き換える。
 * 60 Hz のイベントごとに state を更新して全体を描き直さないための入れ物で、rAF の 1 フレームごとに frame() を呼ぶ。
 */
export class MeterEngine {
  private channels = new Map<string, Channel>();
  private bars = new Map<string, Bar>();
  private readouts = new Map<ReadoutKey, HTMLElement>();
  private last: number | null = null;

  /** 受けた値を棒の目標にする。含まれない棒と、`playing: false` の間の全部は下端へ向かう。 */
  push(m: MixerMeters): void {
    for (const c of this.channels.values()) c.target = METER_FLOOR_DB;
    if (m.playing) {
      m.master.peak_db.slice(0, MASTER_CHANNELS).forEach((db, i) => (this.channel(`m:${i}`).target = dbOrFloor(db)));
      for (const t of m.tracks) this.channel(`t:${t.layer}`).target = dbOrFloor(t.peak_db);
    }
    this.write("true_peak", m.master.true_peak_db);
    this.write("short_term", m.master.short_term_lufs);
    this.write("integrated", m.master.integrated_lufs);
  }

  /** `id` は `m:0`、`m:1`（マスターの L、R）か `t:<layer>`。外すときは `null`。 */
  attachBar(id: string, root: HTMLElement | null): void {
    if (!root) {
      this.bars.delete(id);
      return;
    }
    const fill = root.querySelector<HTMLElement>(".mm-mixer-meter-fill");
    const hold = root.querySelector<HTMLElement>(".mm-mixer-meter-hold");
    if (!fill || !hold) return;
    this.bars.set(id, { fill, hold, clip: "", holdLeft: "" });
    this.draw(id);
  }

  attachReadout(key: ReadoutKey, el: HTMLElement | null): void {
    if (el) this.readouts.set(key, el);
    else this.readouts.delete(key);
  }

  /** rAF の 1 フレーム。`now` は ms。 */
  frame(now: number): void {
    const dt = this.last === null ? 0 : (now - this.last) / MS_PER_S;
    this.last = now;
    for (const c of this.channels.values()) {
      c.shown = Math.max(c.target, c.shown - DECAY_DB_PER_S * dt);
      if (c.shown >= c.hold) {
        c.hold = c.shown;
        c.holdAt = now;
      } else if (now - c.holdAt > HOLD_MS) {
        c.hold = Math.max(c.shown, c.hold - DECAY_DB_PER_S * dt);
      }
    }
    for (const id of this.bars.keys()) this.draw(id);
  }

  /** 棒の表示中の値（テスト用）。 */
  shownDb(id: string): number | null {
    return this.channels.get(id)?.shown ?? null;
  }

  /** ホールドの目印の値（テスト用）。 */
  holdDb(id: string): number | null {
    return this.channels.get(id)?.hold ?? null;
  }

  private channel(id: string): Channel {
    let c = this.channels.get(id);
    if (!c) {
      c = { target: METER_FLOOR_DB, shown: METER_FLOOR_DB, hold: METER_FLOOR_DB, holdAt: 0 };
      this.channels.set(id, c);
    }
    return c;
  }

  private write(key: ReadoutKey, db: number | null): void {
    const el = this.readouts.get(key);
    const text = formatDb(db);
    if (el && el.textContent !== text) el.textContent = text;
  }

  private draw(id: string): void {
    const bar = this.bars.get(id);
    if (!bar) return;
    const c = this.channels.get(id);
    const shown = dbToFraction(c?.shown ?? METER_FLOOR_DB);
    const hold = dbToFraction(c?.hold ?? METER_FLOOR_DB);
    const clip = `inset(0 ${((1 - shown) * PERCENT).toFixed(1)}% 0 0)`;
    if (clip !== bar.clip) {
      bar.clip = clip;
      bar.fill.style.clipPath = clip;
    }
    const left = `${(hold * PERCENT).toFixed(1)}%`;
    if (left !== bar.holdLeft) {
      bar.holdLeft = left;
      bar.hold.style.left = left;
      bar.hold.hidden = hold <= 0;
    }
  }
}
