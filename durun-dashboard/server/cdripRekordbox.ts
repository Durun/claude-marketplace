// 書き出した mp3 を rekordbox に取り込む XML。プレイリストは、リストの見出し行（グループ）ごとに作る。rbxml.py の移植。
import type { CdRipItem } from '../shared/types.ts';

export type RekordboxEntry = { name: string; artist: string; album: string; path: string; playlist: string };

/** グループの無い行を入れるプレイリスト。 */
export const DEFAULT_PLAYLIST = '追加';

/** Python の xml.sax.saxutils.quoteattr と同じ属性値の囲み方。" を含み ' を含まなければ ' で囲み、両方含めば " を &quot; にする。 */
export const quoteAttr = (value: string): string => {
  const data = value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
  if (!data.includes('"')) return `"${data}"`;
  return data.includes("'") ? `"${data.replaceAll('"', '&quot;')}"` : `'${data}'`;
};

const SAFE = /[A-Za-z0-9_.~\-/:]/;

/** rekordbox の Location。/ と : 以外の記号は UTF-8 のバイトごとに % でエスケープする（urllib.parse.quote と同じ）。 */
export const fileLocation = (file: string): string => {
  const p = file.replaceAll('\\', '/');
  const escaped = [...p].map((c) => (SAFE.test(c) ? c : [...Buffer.from(c)].map((b) => `%${b.toString(16).toUpperCase().padStart(2, '0')}`).join(''))).join('');
  return `file://localhost${escaped.startsWith('/') ? '' : '/'}${escaped}`;
};

const playlistXml = (name: string, ids: number[]): string =>
  `    <NODE Name=${quoteAttr(name)} Type="1" KeyType="0" Entries="${ids.length}">\n${ids.map((k) => `      <TRACK Key="${k}"/>`).join('\n')}\n    </NODE>`;

/** entries を、プレイリストの出現順に並べた XML にする。 */
export const rekordboxXml = (entries: RekordboxEntry[]): string => {
  const tracks = entries.map((e, i) => `    <TRACK TrackID="${i + 1}" Name=${quoteAttr(e.name)} Artist=${quoteAttr(e.artist)} Album=${quoteAttr(e.album)} Kind="MP3 File" Location=${quoteAttr(fileLocation(e.path))}/>`);
  const lists = new Map<string, number[]>();
  entries.forEach((e, i) => lists.set(e.playlist, [...(lists.get(e.playlist) ?? []), i + 1]));
  return `<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="7.2.18" Company="AlphaTheta"/>
  <COLLECTION Entries="${tracks.length}">
${tracks.join('\n')}
  </COLLECTION>
  <PLAYLISTS>
  <NODE Type="0" Name="ROOT" Count="${lists.size}">
${[...lists].map(([name, ids]) => playlistXml(name, ids)).join('\n')}
  </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
`;
};

/** 書き出し済みの行から、XML に載せる曲を集める。new は、まだ rekordbox 用に渡していない行だけ。 */
export const rekordboxEntries = (items: CdRipItem[], scope: 'new' | 'all'): { entries: RekordboxEntry[]; itemIds: string[] } => {
  const picked = items.filter((i) => i.status === 'done' && i.meta && i.exported && (scope === 'all' || !i.exported.rekordboxAt));
  const entries = picked.flatMap((i) => {
    const meta = i.meta;
    // 書き出したファイルは、FLAC イメージを先頭に、mp3 を曲の順に並べてある。
    const mp3 = (i.exported?.files ?? []).filter((f) => f.toLowerCase().endsWith('.mp3'));
    return mp3.map((path, k) => ({ name: meta?.tracks[k]?.title ?? '', artist: meta?.tracks[k]?.artist || meta?.artist || '', album: meta?.album ?? '', path, playlist: i.group ?? DEFAULT_PLAYLIST }));
  });
  return { entries, itemIds: picked.filter((i) => i.exported?.files.some((f) => f.toLowerCase().endsWith('.mp3'))).map((i) => i.id) };
};
