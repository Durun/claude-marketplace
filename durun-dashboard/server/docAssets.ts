// HTML の資料が相対パスで埋め込んだ音声、画像、動画を、資料のファイルがあるディレクトリから探す。
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
