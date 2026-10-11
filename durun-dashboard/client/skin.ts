// スキンは画面の見た目を切り替える設定。メニューから選び、選んだものはブラウザに保存する。
import { useEffect, useState } from 'preact/hooks';
import { XP_SCROLLBAR_STYLE } from './docFrame.ts';

export const SKINS = { default: 'デフォルト', xp: 'Windows XP' } as const;
export type Skin = keyof typeof SKINS;
const SKIN_KEY = 'dashboard-skin';

const load = (): Skin => {
  const v = localStorage.getItem(SKIN_KEY) ?? '';
  return Object.hasOwn(SKINS, v) ? (v as Skin) : 'default';
};

let skin = load();
const listeners = new Set<(skin: Skin) => void>();

// スクロールバーの見た目は資料の iframe にも同じものを入れるので、style.css には書かず、style 要素で当てる。
const scrollbarStyle = document.createElement('style');
scrollbarStyle.textContent = XP_SCROLLBAR_STYLE;

// スキンは html の data-skin で CSS を切り替える。最初の描画から保存したスキンで描くよう、読み込んだ時点で当てる。
const apply = (): void => {
  document.documentElement.dataset.skin = skin;
  if (skin === 'xp') document.head.append(scrollbarStyle);
  else scrollbarStyle.remove();
};
apply();

export const setSkin = (next: Skin): void => {
  skin = next;
  apply();
  localStorage.setItem(SKIN_KEY, next);
  listeners.forEach((fn) => fn(next));
};

export const useSkin = (): Skin => {
  const [current, setCurrent] = useState(skin);
  useEffect(() => {
    listeners.add(setCurrent);
    return () => {
      listeners.delete(setCurrent);
    };
  }, []);
  return current;
};
