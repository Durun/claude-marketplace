// HTML の資料が相対パスで埋め込んだ音声、画像、動画を、資料のファイルがあるディレクトリから探す。
// 音声は ogg を基本にする。wav は 1 分で 10MB を超え、ダウンロードする HTML に入れると重い。
import fs from 'node:fs';
import path from 'node:path';
import { MEDIA_TYPES } from '../shared/types.ts';

const ASSET_TYPES: Record<string, string> = {
  ...MEDIA_TYPES,
  '.wav': 'audio/wav',
  '.mp3': 'audio/mpeg',
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
  '.ogg': 'audio/ogg',
  '.opus': 'audio/ogg',
  '.flac': 'audio/flac',
};

/** 資料からの相対パスが指すファイルと、その MIME タイプ。資料のディレクトリの外と、メディアでないファイルは返さない。 */
export const docAsset = (docPath: string, rel: string): { full: string; type: string } | undefined => {
  const dir = path.dirname(docPath);
  const full = path.resolve(dir, rel);
  const type = ASSET_TYPES[path.extname(full).toLowerCase()];
  // Windows で別のドライブを指すと、path.relative は絶対パスを返す。
  const fromDir = path.relative(dir, full);
  if (!type || fromDir.startsWith('..') || path.isAbsolute(fromDir)) return undefined;
  return { full, type };
};

// これより大きいファイルは HTML に入れない。base64 にすると 3 割ほど増え、開けないほど重い HTML になる。
const MAX_INLINE_BYTES = 32 * 1024 * 1024;

/**
 * ダウンロードした HTML を、資料のディレクトリから離れた場所で開いても音が鳴り画像が出るよう、相対パスで埋め込んだファイルを data URL にして HTML の中に入れる。
 * 見るのは src と poster の属性だけで、スクリプトが組み立てたパスは置き換えない。
 */
export const inlineAssets = (html: string, docPath: string): string =>
  html.replace(/\b(src|poster)=(["'])([^"']+)\2/g, (whole: string, attr: string, quote: string, ref: string) => {
    // スキームや / で始まるものは、資料のディレクトリからの相対パスではない。
    if (/^(?:[a-z][a-z\d+.-]*:|\/|#)/i.test(ref) || !URL.canParse(ref, 'file:///')) return whole;
    const asset = docAsset(docPath, decodeURIComponent(new URL(ref, 'file:///').pathname.slice(1)));
    const stat = asset && fs.statSync(asset.full, { throwIfNoEntry: false });
    if (!asset || !stat?.isFile() || stat.size > MAX_INLINE_BYTES) return whole;
    return `${attr}=${quote}data:${asset.type};base64,${fs.readFileSync(asset.full).toString('base64')}${quote}`;
  });
