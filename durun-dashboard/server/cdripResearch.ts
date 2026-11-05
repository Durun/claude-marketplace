// 曲情報（meta）の検証と、CD との突き合わせ。AI の答えも、人間が画面で直した値も、書き出す前にここを通す。
import type { CdMeta, CdTrack } from '../shared/types.ts';

const text = (v: unknown, name: string, required: boolean): string | undefined => {
  if (v === undefined || v === null || v === '') {
    if (required) throw new Error(`${name} が空です`);
    return undefined;
  }
  if (typeof v !== 'string') throw new Error(`${name} は文字列にしてください`);
  const t = v.trim();
  if (required && !t) throw new Error(`${name} が空です`);
  return t || undefined;
};

const count = (v: unknown, name: string): number | undefined => {
  if (v === undefined || v === null) return undefined;
  if (typeof v !== 'number' || !Number.isInteger(v) || v < 1) throw new Error(`${name} は 1 以上の整数にしてください`);
  return v;
};

const trackOf = (v: unknown, i: number): CdTrack => {
  if (typeof v !== 'object' || v === null) throw new Error(`${i + 1} 曲目の形が違います`);
  const t = v as Record<string, unknown>;
  const duration = t.duration;
  if (duration !== undefined && duration !== null && (typeof duration !== 'number' || !(duration >= 0))) throw new Error(`${i + 1} 曲目の duration は秒の数にしてください`);
  const artist = text(t.artist, `${i + 1} 曲目の artist`, false);
  const composer = text(t.composer, `${i + 1} 曲目の composer`, false);
  return { title: text(t.title, `${i + 1} 曲目の title`, true) ?? '', ...(artist ? { artist } : {}), ...(composer ? { composer } : {}), ...(typeof duration === 'number' ? { duration } : {}) };
};

/** meta の形と値を検める。足りない項目や型の違いは Error の理由にし、省ける項目の空文字は無いものにそろえる。 */
export const validateMeta = (value: unknown): CdMeta => {
  if (typeof value !== 'object' || value === null) throw new Error('曲情報はオブジェクトにしてください');
  const v = value as Record<string, unknown>;
  if (v.dest !== 'CLUB' && v.dest !== 'Music') throw new Error('dest は CLUB か Music にしてください');
  if (!Array.isArray(v.tracks) || !v.tracks.length) throw new Error('tracks に 1 曲以上を入れてください');
  const disc = count(v.disc, 'disc');
  const totaldiscs = count(v.totaldiscs, 'totaldiscs');
  if (disc !== undefined && totaldiscs !== undefined && disc > totaldiscs) throw new Error('disc が totaldiscs を超えています');
  const confidence = v.confidence === 'high' || v.confidence === 'medium' || v.confidence === 'low' ? v.confidence : undefined;
  const sources = Array.isArray(v.sources) ? v.sources.filter((s): s is string => typeof s === 'string') : undefined;
  const optional: [keyof CdMeta, string | undefined][] = [
    ['date', text(v.date, 'date', false)],
    ['genre', text(v.genre, 'genre', false)],
    ['catalog', text(v.catalog, 'catalog', false)],
    ['cover', text(v.cover, 'cover', false)],
    ['coverUrl', text(v.coverUrl, 'coverUrl', false)],
    ['notes', text(v.notes, 'notes', false)],
  ];
  return {
    circle: text(v.circle, 'circle', true) ?? '',
    album: text(v.album, 'album', true) ?? '',
    artist: text(v.artist, 'artist', true) ?? '',
    dest: v.dest,
    tracks: v.tracks.map(trackOf),
    ...Object.fromEntries(optional.filter(([, x]) => x !== undefined)),
    ...(disc !== undefined ? { disc } : {}),
    ...(totaldiscs !== undefined ? { totaldiscs } : {}),
    ...(confidence ? { confidence } : {}),
    ...(sources?.length ? { sources } : {}),
  };
};

/** 曲数が CD と合わなければ、確認の画面に出す理由を返す。合っていれば undefined。 */
export const matchTrackCount = (meta: Pick<CdMeta, 'tracks'>, discTracks: number | undefined): string | undefined =>
  discTracks !== undefined && meta.tracks.length !== discTracks ? `曲数が合いません（曲情報は ${meta.tracks.length} 曲、取り込んだ CD は ${discTracks} 曲）` : undefined;

const IMAGE_EXTENSIONS: Record<string, string> = { 'image/jpeg': 'jpg', 'image/png': 'png', 'image/webp': 'webp' };
const MAX_COVER_BYTES = 20 * 1024 * 1024;
const COVER_TIMEOUT_MS = 20_000;

/** ジャケットの画像を URL から読む。画像でないものと大きすぎるものは断る。 */
export const fetchCover = async (url: string, doFetch: typeof fetch = fetch): Promise<{ bytes: Buffer; ext: string }> => {
  const u = URL.parse(url.trim());
  if (!u || (u.protocol !== 'http:' && u.protocol !== 'https:')) throw new Error('http か https の URL にしてください');
  const res = await doFetch(u, { signal: AbortSignal.timeout(COVER_TIMEOUT_MS), headers: { 'user-agent': 'Mozilla/5.0 (durun-dashboard)' } });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const type = res.headers.get('content-type')?.split(';')[0]?.trim().toLowerCase() ?? '';
  const ext = IMAGE_EXTENSIONS[type];
  if (!ext) throw new Error(`画像ではありません（${type || '種類不明'}）`);
  const bytes = Buffer.from(await res.arrayBuffer());
  if (bytes.length > MAX_COVER_BYTES) throw new Error('画像が大きすぎます（20MB まで）');
  return { bytes, ext };
};

/** 取り込んだ CD から読めたこと（CD-Text）だけで作る、曲情報の下書き。人間か AI が埋める。 */
export const skeletonMeta = (disc: { tracks: number; cdText?: { album?: string; artist?: string; titles?: string[] } }): CdMeta => ({
  circle: '',
  album: disc.cdText?.album ?? '',
  artist: disc.cdText?.artist ?? '',
  dest: 'CLUB',
  tracks: Array.from({ length: disc.tracks }, (_, i) => ({ title: disc.cdText?.titles?.[i] ?? '' })),
});
