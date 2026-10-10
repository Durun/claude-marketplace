// XP スキンの壁紙の置き場所。画像のファイルか画像の URL のどちらか 1 つを、データディレクトリに置く。
import fs from 'node:fs';
import path from 'node:path';

/** 壁紙にできる画像の種類と、保存するときの拡張子。SVG は、直接開かれると中のスクリプトが動くので受けない。 */
export const WALLPAPER_TYPES: Record<string, string> = { 'image/jpeg': '.jpg', 'image/png': '.png', 'image/webp': '.webp', 'image/gif': '.gif' };
const URL_EXT = '.url';

const fileOf = (dir: string, ext: string): string => path.join(dir, `wallpaper${ext}`);

/** file は保存した画像のパス、url は画面に直接読ませる画像の URL。 */
export type Wallpaper = { file: string; type: string } | { url: string };

export const readWallpaper = (dir: string): Wallpaper | undefined => {
  for (const [type, ext] of Object.entries(WALLPAPER_TYPES)) {
    const file = fileOf(dir, ext);
    if (fs.existsSync(file)) return { file, type };
  }
  try {
    const url = fs.readFileSync(fileOf(dir, URL_EXT), 'utf8').trim();
    return url ? { url } : undefined;
  } catch {
    return undefined;
  }
};

/** 壁紙を外し、スキンの初期の絵に戻す。 */
export const clearWallpaper = (dir: string): void => {
  for (const ext of [...Object.values(WALLPAPER_TYPES), URL_EXT]) fs.rmSync(fileOf(dir, ext), { force: true });
};

/** 前の壁紙を外してから置く。画像と URL が両方残ると、どちらを敷くかが決まらないため。 */
export const saveWallpaper = (dir: string, next: { type: string; bytes: Buffer } | { url: string }): void => {
  clearWallpaper(dir);
  fs.mkdirSync(dir, { recursive: true });
  if ('url' in next) fs.writeFileSync(fileOf(dir, URL_EXT), next.url);
  else fs.writeFileSync(fileOf(dir, WALLPAPER_TYPES[next.type] ?? '.jpg'), next.bytes);
};
