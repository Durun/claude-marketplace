// FL Connect の、FL の値の比べ方と記録の作り方。FL の中のスクリプトが読んだ生の値（行）を比べ、変わった値を記録にする。
// ここの関数は FL にも MIDI にも触れないので、テストで確かめる。
import type { FlChange, FlRecord, FlSection, FlValue } from '../shared/types.ts';

/** FL の中のスクリプトが返す、チャンネルやトラックの 1 行。i は MIDI スクリプトの API の番号。 */
export type Row = { i: number } & Record<string, FlValue | undefined>;

export type RowSection = 'channels' | 'mixer' | 'playlist' | 'patterns';

/** 比べる項目。sel（選んでいるか）は変わりやすく、人間の操作として記録すると多すぎるので比べない。 */
const FIELDS: Record<RowSection, string[]> = {
  channels: ['name', 'color', 'vol', 'pan', 'mute', 'solo', 'mixer', 'plugin'],
  mixer: ['name', 'color', 'vol', 'pan', 'mute', 'solo'],
  playlist: ['name', 'color', 'mute', 'solo'],
  patterns: ['name', 'color', 'length'],
};

const KIND: Record<FlSection, string> = { channels: 'channel', mixer: 'mixer', playlist: 'playlist', patterns: 'pattern', sends: 'mixer', song: 'song' };

const FIELD_LABEL: Record<string, string> = {
  name: '名前',
  color: '色',
  vol: '音量',
  pan: 'パン',
  mute: 'ミュート',
  solo: 'ソロ',
  mixer: '送り先のミキサー',
  plugin: 'プラグイン',
  length: '長さ',
  tempo: 'テンポ',
  playing: '再生',
  recording: '録音',
  sends: '送り先',
};

/** 同じ対象の同じ項目が、この時間のうちに続けて変わったら、1 件にまとめる。つまみを回す操作が何十件にもならないように。 */
export const COALESCE_MS = 1500;
const MAX_RECORDS = 1000;

export const same = (a: FlValue | undefined, b: FlValue | undefined): boolean => (Array.isArray(a) || Array.isArray(b) ? JSON.stringify(a) === JSON.stringify(b) : a === b);

/** 記録や画面に出す対象の名前。番号は MIDI スクリプトの API の番号。 */
export const targetOf = (section: FlSection, index: number, name?: FlValue): string =>
  section === 'song' ? 'song' : `${KIND[section]} ${index}${typeof name === 'string' && name ? `「${name}」` : ''}`;

/**
 * 2 つの並びの対応（前の位置 → 後の位置）。チャンネルやパターンの数が変わったときに、足されたものと消えたものを見分けるため、key の最長共通部分列で合わせる。
 * 数が同じなら、名前を変えたものも同じ位置どうしで合わせたいので、位置をそのまま対応させる。
 */
export const align = (before: string[], after: string[]): Map<number, number> => {
  const n = before.length;
  const m = after.length;
  if (n === m) return new Map(before.map((_, k) => [k, k]));
  const width = m + 1;
  const dp = new Int32Array((n + 1) * width);
  const at = (i: number, j: number): number => dp[i * width + j] ?? 0;
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) dp[i * width + j] = before[i] === after[j] ? at(i + 1, j + 1) + 1 : Math.max(at(i + 1, j), at(i, j + 1));
  }
  const map = new Map<number, number>();
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (before[i] === after[j]) {
      map.set(i, j);
      i += 1;
      j += 1;
    } else if (at(i + 1, j) >= at(i, j + 1)) i += 1;
    else j += 1;
  }
  return map;
};

const rowKey = (r: Row): string => `${String(r.name)}|${String(r.plugin ?? '')}`;

const fieldChange = (section: RowSection, b: Row, a: Row, field: string): FlChange | undefined => {
  if (field === 'vol') {
    // 音量は、見せるのは dB で、書き戻すのは 0 から 1 の値。dB は丸めてあるので、比べるのは書き戻す値にする。
    if (same(b.vol, a.vol)) return undefined;
    return { section, index: a.i, target: targetOf(section, a.i, a.name), field, before: b.db ?? null, after: a.db ?? null, raw: [b.vol ?? null, a.vol ?? null] };
  }
  if (same(b[field], a[field])) return undefined;
  return { section, index: a.i, target: targetOf(section, a.i, a.name), field, before: b[field] ?? null, after: a[field] ?? null };
};

/** 並び（チャンネル、ミキサー、パターン）を比べ、変わった値と、足されたものと消えたものを返す。 */
export const diffRows = (section: Exclude<RowSection, 'playlist'>, before: Row[], after: Row[]): FlChange[] => {
  const map = align(before.map(rowKey), after.map(rowKey));
  const changes: FlChange[] = [];
  const matched = new Set<number>();
  for (const [bi, ai] of map) {
    const b = before[bi];
    const a = after[ai];
    if (!b || !a) continue;
    matched.add(ai);
    for (const field of FIELDS[section]) {
      const c = fieldChange(section, b, a, field);
      if (c) changes.push(c);
    }
  }
  before.forEach((b, bi) => {
    if (!map.has(bi)) changes.push({ section, index: b.i, target: targetOf(section, b.i, b.name), field: 'removed', before: b.name ?? null, after: null });
  });
  after.forEach((a, ai) => {
    if (!matched.has(ai)) changes.push({ section, index: a.i, target: targetOf(section, a.i, a.name), field: 'added', before: null, after: a.name ?? null });
  });
  return changes;
};

/**
 * プレイリストのトラックを比べる。スクリプトは末尾の触っていないトラックを返さないので、無い行は既定のトラックとして比べる。
 * 既定の色は分からないので、無い行の色は null にする。
 */
export const diffPlaylist = (before: Row[], after: Row[]): FlChange[] => {
  const plain = (i: number): Row => ({ i, name: `Track ${i}`, color: null, mute: false, solo: false });
  const byIndex = (rows: Row[]): Map<number, Row> => new Map(rows.map((r) => [r.i, r]));
  const b = byIndex(before);
  const a = byIndex(after);
  const indices = [...new Set([...b.keys(), ...a.keys()])].sort((x, y) => x - y);
  return indices.flatMap((i) => FIELDS.playlist.flatMap((field) => fieldChange('playlist', b.get(i) ?? plain(i), a.get(i) ?? plain(i), field) ?? []));
};

/** ミキサーのトラックの送り先を比べる。names は、記録に添えるトラックの名前。 */
export const diffSends = (before: Record<string, number[]>, after: Record<string, number[]>, names: Map<number, FlValue | undefined>): FlChange[] =>
  Object.keys(after).flatMap((key) => {
    const b = before[key];
    const a = after[key];
    if (!b || !a || same(b, a)) return [];
    const index = Number(key);
    return [{ section: 'sends' as const, index, target: targetOf('sends', index, names.get(index)), field: 'sends', before: b, after: a }];
  });

export type SongState = { tempo?: number | null; playing?: boolean; recording?: boolean };

/** テンポと、再生と録音の状態を比べる。 */
export const diffSong = (before: SongState, after: SongState): FlChange[] =>
  (['tempo', 'playing', 'recording'] as const).flatMap((field) =>
    before[field] === undefined || same(before[field], after[field]) ? [] : [{ section: 'song' as const, index: 0, target: 'song', field, before: before[field] ?? null, after: after[field] ?? null }],
  );

const hex = (v: number): string => `#${(v & 0xffffff).toString(16).toUpperCase().padStart(6, '0')}`;

const panText = (v: number): string => (Math.abs(v) < 0.005 ? '中央' : v < 0 ? `左 ${Math.round(-v * 100)}%` : `右 ${Math.round(v * 100)}%`);

/** 記録の値を、人間とエージェントが読める形にする。 */
export const formatValue = (field: string, v: FlValue): string => {
  if (field === 'vol') return v === null ? '-∞ dB' : `${String(v)} dB`;
  if (v === null) return '（既定）';
  if (typeof v === 'boolean') return v ? 'オン' : 'オフ';
  if (Array.isArray(v)) return v.length ? v.map((d) => `mixer ${d}`).join('、') : 'なし';
  if (field === 'color' && typeof v === 'number') return hex(v);
  if (field === 'pan' && typeof v === 'number') return panText(v);
  if (field === 'mixer') return `mixer ${String(v)}`;
  if (field === 'length') return `${String(v)} 拍`;
  if (field === 'tempo') return `${String(v)} BPM`;
  return typeof v === 'string' ? `「${v}」` : String(v);
};

/** まとめずに 1 件ずつ残す項目。足した、消した、プロジェクトを読み込み直した、は前と後の値で比べられない。 */
const EVENT_FIELDS = new Set(['added', 'removed', 'project']);

export const describeChange = (c: FlChange): string => {
  if (c.field === 'project') return 'プロジェクトを読み込み直した（ここより前の番号と名前は、今のプロジェクトのものではない）';
  if (c.field === 'added') return `${c.target}を足した`;
  if (c.field === 'removed') return `${c.target}を消した`;
  return `${c.target}の${FIELD_LABEL[c.field] ?? c.field}: ${formatValue(c.field, c.before)} → ${formatValue(c.field, c.after)}`;
};

/** 操作の記録。番号（seq）は増え続けるので、どこまで読んだかを番号で覚えられる。 */
export class RecordLog {
  records: FlRecord[] = [];
  #seq = 0;

  get last(): number {
    return this.#seq;
  }

  /**
   * 同じ人の、同じ対象の同じ項目の変化が COALESCE_MS のうちに続いたら、前の記録の後の値を書き換えて、末尾に移す。
   * 移すときに番号を振り直すので、前の記録を読んだ人にも、まとめた後の値がもう一度届く。元の値に戻ったら、記録を消す。
   */
  add(change: FlChange, who: FlRecord['who'], via?: string, at = Date.now()): void {
    if (!EVENT_FIELDS.has(change.field)) {
      for (let k = this.records.length - 1; k >= 0; k--) {
        const r = this.records[k];
        if (!r || at - r.at > COALESCE_MS) break;
        if (r.section !== change.section || r.index !== change.index || r.field !== change.field || r.who !== who || r.via !== via) continue;
        this.records.splice(k, 1);
        const raw: FlChange['raw'] = r.raw && change.raw ? [r.raw[0], change.raw[1]] : change.raw;
        const merged = { ...r, target: change.target, after: change.after, raw, at };
        if (!(raw ? same(raw[0], raw[1]) : same(merged.before, merged.after))) this.#push(merged);
        return;
      }
    }
    this.#push({ ...change, seq: 0, at, who, via });
  }

  #push(r: FlRecord): void {
    this.#seq += 1;
    this.records.push({ ...r, seq: this.#seq });
    if (this.records.length > MAX_RECORDS) this.records.splice(0, this.records.length - MAX_RECORDS);
  }

  since(seq: number): FlRecord[] {
    return this.records.filter((r) => r.seq > seq);
  }
}

/** Python の値の書き方。文字は JSON の書き方が、そのまま Python の文字の書き方になる。 */
const py = (v: FlValue | undefined): string => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'boolean') return v ? 'True' : 'False';
  if (v === null || v === undefined) return 'None';
  if (Array.isArray(v)) return `[${v.join(', ')}]`;
  return String(v);
};

const onOff = (v: FlValue): number => (v ? 1 : 0);

/** 変化を前の値に戻す Python の行。① の道で戻せない変化（足したチャンネル、テンポ、既定の色など）は空。 */
export const restoreLines = (c: FlChange): string[] => {
  const i = c.index;
  const v = c.raw ? c.raw[0] : c.before;
  if (c.section === 'sends' && Array.isArray(c.before) && Array.isArray(c.after)) {
    const before = c.before;
    const after = c.after;
    const lines = [...after.filter((d) => !before.includes(d)).map((d) => `mixer.setRouteTo(${i}, ${d}, False)`), ...before.filter((d) => !after.includes(d)).map((d) => `mixer.setRouteTo(${i}, ${d}, True)`)];
    return lines.length ? [...lines, 'mixer.afterRoutingChanged()'] : [];
  }
  if (v === null && c.field !== 'name') return [];
  const setters: Record<string, string> = {
    'channels.name': `channels.setChannelName(${i}, ${py(v)})`,
    'channels.color': `channels.setChannelColor(${i}, ${py(v)})`,
    'channels.vol': `channels.setChannelVolume(${i}, ${py(v)})`,
    'channels.pan': `channels.setChannelPan(${i}, ${py(v)})`,
    'channels.mute': `channels.muteChannel(${i}, ${onOff(v)})`,
    // ソロは切り替えしかできないので、今の状態と違うときだけ切り替える。
    'channels.solo': `if channels.isChannelSolo(${i}) != ${py(!!v)}: channels.soloChannel(${i})`,
    'channels.mixer': `channels.setTargetFxTrack(${i}, ${py(v)})`,
    'mixer.name': `mixer.setTrackName(${i}, ${py(v)})`,
    'mixer.color': `mixer.setTrackColor(${i}, ${py(v)})`,
    'mixer.vol': `mixer.setTrackVolume(${i}, ${py(v)})`,
    'mixer.pan': `mixer.setTrackPan(${i}, ${py(v)})`,
    'mixer.mute': `mixer.muteTrack(${i}, ${onOff(v)})`,
    'mixer.solo': `mixer.soloTrack(${i}, ${onOff(v)})`,
    // 名前を空にすると、FL は既定の名前に戻す。
    'playlist.name': `playlist.setTrackName(${i}, ${py(v ?? '')})`,
    'playlist.color': `playlist.setTrackColor(${i}, ${py(v)})`,
    'playlist.mute': `playlist.muteTrack(${i}, ${onOff(v)})`,
    'playlist.solo': `playlist.soloTrack(${i}, ${onOff(v)})`,
    'patterns.name': `patterns.setPatternName(${i}, ${py(v)})`,
    'patterns.color': `patterns.setPatternColor(${i}, ${py(v)})`,
  };
  const line = setters[`${c.section}.${c.field}`];
  return line ? [line] : [];
};

/** remove_channels の target_channels（1 からの番号、名前、all、selected をカンマで区切る）が指すチャンネルの番号（0 から）。名前は大文字と小文字を区別しない。指せない語は unknown に返す。 */
export const resolveChannels = (spec: string, rows: Row[]): { indices: number[]; unknown: string[] } => {
  const indices = new Set<number>();
  const unknown: string[] = [];
  for (const token of spec.split(',').map((t) => t.trim()).filter(Boolean)) {
    const lower = token.toLowerCase();
    const hits =
      lower === 'all' ? rows : lower === 'selected' ? rows.filter((r) => r.sel) : /^\d+$/.test(token) ? rows.filter((r) => r.i === Number(token) - 1) : rows.filter((r) => String(r.name).toLowerCase() === lower);
    if (!hits.length && lower !== 'selected') unknown.push(token);
    for (const r of hits) indices.add(r.i);
  }
  return { indices: [...indices].sort((x, y) => x - y), unknown };
};

/** 番号で覚えたチャンネルを、チャンネルの数が変わった後の番号に付け替える。消えたチャンネルは外す。 */
export const followChannels = (tracked: number[], before: Row[], after: Row[]): number[] => {
  const map = align(before.map(rowKey), after.map(rowKey));
  return tracked.flatMap((i) => {
    const j = map.get(i);
    return j === undefined ? [] : [j];
  });
};
