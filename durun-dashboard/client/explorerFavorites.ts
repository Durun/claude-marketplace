// エクスプローラーでお気に入りに登録したフォルダー。ブラウザに保存し、スタートメニューとエクスプローラーの両方に並べる。
import { useEffect, useState } from 'preact/hooks';

const STORAGE_KEY = 'explorer-favorites';

const load = (): string[] => {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]') as string[];
  } catch {
    return [];
  }
};

let favorites = load();
const listeners = new Set<(list: string[]) => void>();

/** 登録していれば外し、していなければ末尾に足す。 */
export const toggled = (list: string[], path: string): string[] => (list.includes(path) ? list.filter((p) => p !== path) : [...list, path]);

/** パスの最後の区切りを、フォルダーやファイルの名前として出す。ドライブや / のように区切りが無いものは、そのまま出す。 */
export const baseName = (path: string): string => path.replace(/\/+$/, '').split('/').at(-1) || path;

export const toggleFavorite = (path: string): void => {
  favorites = toggled(favorites, path);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(favorites));
  } catch {
    // 保存できなくても、開いている間はお気に入りを使える。
  }
  listeners.forEach((fn) => fn(favorites));
};

export const useFavorites = (): string[] => {
  const [list, setList] = useState(favorites);
  useEffect(() => {
    // 描いてから待ち受けるまでの間に変わった分を取りこぼさない。
    setList(favorites);
    listeners.add(setList);
    return () => {
      listeners.delete(setList);
    };
  }, []);
  return list;
};
