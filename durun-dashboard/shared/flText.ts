// FL Connect の記録を、人間が読む文に訳す。記録は FL が返した生の値のまま持ち、訳すのはウィンドウの仕事にする。
// 知らない項目は、FL が付けた名前と値の文字か、生の値をそのまま見せる。
import type { FlAgentEntry, FlChange, FlSection, FlValue } from './types.ts';

const KIND: Record<FlSection, string> = { channels: 'channel', mixer: 'mixer', playlist: 'playlist', patterns: 'pattern', sends: 'mixer', song: 'song', undo: 'undo' };

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
  stereo: 'ステレオの広がり',
  enabled: '有効',
  polarity: '極性の反転',
  swap: '左右の入れ替え',
  armed: '録音待ち',
  fx: 'エフェクトのスロット',
};

const EQ_PARTS = ['Gain', 'Frequency', 'Bandwidth'];

/** EQ の変わった帯と値。eq は帯ごとの [Gain, Frequency, Bandwidth] を順に並べた、0 から 1 の値。 */
const eqText = (before: FlValue, after: FlValue): string => {
  const b = Array.isArray(before) ? before : [];
  const a = Array.isArray(after) ? after : [];
  const parts = a.flatMap((v, k) => (b[k] === v ? [] : [`帯 ${Math.floor(k / 3) + 1} の ${EQ_PARTS[k % 3] ?? '?'} ${String(b[k] ?? '?')} → ${String(v)}`]));
  return parts.length ? parts.join('、') : `${JSON.stringify(before)} → ${JSON.stringify(after)}`;
};

/** FL がウィンドウの名前に入れるアイコンの文字（Unicode の私用領域）。例: Sampler (Master) の前後。 */
const ICONS = new RegExp(`[${String.fromCodePoint(0xe000)}-${String.fromCodePoint(0xf8ff)}]`, 'g');

/** ウィンドウの名前から、アイコンの文字を外す。 */
export const windowName = (caption: string | null | undefined): string => (caption ?? '').replace(ICONS, '').replace(/\s+/g, ' ').trim();

/** 記録の対象。番号は MIDI スクリプトの API の番号。 */
export const targetOf = (c: Pick<FlChange, 'section' | 'index' | 'name'>): string => (c.section === 'song' ? 'song' : `${KIND[c.section]} ${c.index}${c.name ? `「${c.name}」` : ''}`);

const hex = (v: number): string => `#${(v & 0xffffff).toString(16).toUpperCase().padStart(6, '0')}`;

const panText = (v: number): string => (Math.abs(v) < 0.005 ? '中央' : v < 0 ? `左 ${Math.round(-v * 100)}%` : `右 ${Math.round(v * 100)}%`);

const dbText = (v: number | null): string => (v === null ? '-∞ dB' : `${String(v)} dB`);

/** 生の値を、人間が読める形にする。 */
export const formatValue = (field: string, v: FlValue): string => {
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

/** 元に戻す履歴の前と後（[数, 戻した数]）から、増えた操作の数、Ctrl+Z で戻した数、やり直した数を出す。 */
export const undoDelta = (before: FlValue, after: FlValue): { added: number; undone: number; redone: number } => {
  const [count0 = 0, undone0 = 0] = Array.isArray(before) ? before : [];
  const [count1 = 0, undone1 = 0] = Array.isArray(after) ? after : [];
  if (count1 === count0 && undone1 !== undone0) return undone1 > undone0 ? { added: 0, undone: undone1 - undone0, redone: 0 } : { added: 0, undone: 0, redone: undone0 - undone1 };
  // 戻したあとで新しく操作すると、FL は戻した分を履歴から消してから足す。プロジェクトを読み込むと履歴が減るが、それは操作ではない。
  return { added: Math.max(0, count1 - undone1 - (count0 - undone0)), undone: 0, redone: 0 };
};

const describeUndo = (c: FlChange): string => {
  const where = windowName(c.name);
  const inWindow = where ? `「${where}」で` : 'FL で';
  const d = undoDelta(c.before, c.after);
  if (d.undone) return `${inWindow}元に戻した（${d.undone} 回）`;
  if (d.redone) return `${inWindow}やり直した（${d.redone} 回）`;
  return `${inWindow}、FL Connect が読まない値を変えた（FL の元に戻す履歴が ${d.added} 件増えた）`;
};

/** FL の値の文字が空のときは、生の値を見せる。 */
const shown = (text: string | undefined, raw: FlValue): string => (text ? text : String(raw));

/** プラグインのパラメーターの変化。field は param:番号 か slot:スロット:param:番号（スロットは 0 から）。 */
const describePlugin = (c: FlChange, target: string): string => {
  const slot = /^slot:(\d+):/.exec(c.field)?.[1];
  const where = `${target}の${slot === undefined ? ' ' : `スロット ${Number(slot) + 1} の `}${c.plugin ?? '?'} `;
  if (c.field.endsWith('bulk')) return `${where}で、一度に ${String(c.after)} 件のパラメーターが変わった（プリセットを読み込んだなど）`;
  return `${where}の「${c.label ?? c.field}」: ${shown(c.text?.[0], c.before)} → ${shown(c.text?.[1], c.after)}`;
};

export const describeChange = (c: FlChange): string => {
  if (c.section === 'undo') return describeUndo(c);
  if (c.field === 'project') return 'プロジェクトを読み込み直した（ここより前の番号と名前は、今のプロジェクトのものではない）';
  if (c.field === 'bulk' && !c.plugin) return `一度に ${String(c.after)} 件の値が変わった（プロジェクトを読み込んだか、まとめて変えた）`;
  const target = targetOf(c);
  if (c.plugin) return describePlugin(c, target);
  if (c.field === 'added') return `${target}を足した`;
  if (c.field === 'removed') return `${target}を消した`;
  // FL が名前と値の文字を付けた項目は、FL の言葉のまま見せる。
  if (c.label) return `${target}の「${c.label}」: ${shown(c.text?.[0], c.before)} → ${shown(c.text?.[1], c.after)}`;
  if (c.field === 'vol' && c.db) return `${target}の音量: ${dbText(c.db[0])} → ${dbText(c.db[1])}`;
  if (c.field === 'eq') return `${target}の EQ: ${eqText(c.before, c.after)}`;
  const level = /^level:(\d+)$/.exec(c.field);
  if (level) return `${target}の mixer ${level[1]} への送りの量: ${String(c.before)} → ${String(c.after)}`;
  return `${target}の${FIELD_LABEL[c.field] ?? c.field}: ${formatValue(c.field, c.before)} → ${formatValue(c.field, c.after)}`;
};

/** エージェントがしたことの 1 行。ツールの名前と、引数の要るところだけを見せる。 */
export const describeAgent = (e: FlAgentEntry): string => {
  const args = e.args;
  const firstLine = (v: unknown): string => (typeof v === 'string' ? (v.trim().split('\n')[0] ?? '').slice(0, 80) : '');
  if (e.tool === 'fl_python' || e.tool === 'fl_pianoroll') return `${e.tool}: ${firstLine(args.code)}`;
  if (e.tool === 'fl_gopher') return `fl_gopher: ${String(args.tool ?? '')}`;
  if (e.tool === 'fl_undo') return `fl_undo: ${String(args.steps ?? 1)} 回分の操作を戻す`;
  if (e.tool === 'fl_backup') {
    // detail は写したファイルを 1 行に 1 つ、output は省いた手順とその理由。
    const count = e.detail ? e.detail.split('\n').length : 0;
    return count ? `エージェントが書き込む前に、プロジェクトのバックアップを取った（${count} 個のファイル）` : `バックアップを取らなかった: ${firstLine(e.output)}`;
  }
  return e.tool;
};
