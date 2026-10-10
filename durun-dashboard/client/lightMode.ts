// 軽量モードは、通信と描画を減らして見る設定。回線が細い端末や非力な端末で開くときに、メニューから入れる。入れたかどうかは、ブラウザに保存する。
import { useEffect, useState } from 'preact/hooks';

const STORAGE_KEY = 'light-mode';

const load = (): boolean => {
  try {
    return localStorage.getItem(STORAGE_KEY) === '1';
  } catch {
    return false;
  }
};

let light = load();
const listeners = new Set<(on: boolean) => void>();

// CSS は html の data-light でアニメーションを止める。最初の描画から止めるよう、読み込んだ時点で当てる。
const apply = (): void => void document.documentElement.toggleAttribute('data-light', light);
apply();

export const setLightMode = (on: boolean): void => {
  light = on;
  apply();
  try {
    localStorage.setItem(STORAGE_KEY, on ? '1' : '0');
  } catch {
    // 保存できなくても、開いている間は切り替えられる。
  }
  listeners.forEach((fn) => fn(on));
};

export const useLightMode = (): boolean => {
  const [on, setOn] = useState(light);
  useEffect(() => {
    listeners.add(setOn);
    return () => {
      listeners.delete(setOn);
    };
  }, []);
  return on;
};
