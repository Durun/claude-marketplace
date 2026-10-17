// 7 日の使用ペースを、土日祝日を除いて数えるかの設定。使用制限のアイコンを押して開くメニューで切り替え、ブラウザに保存する。
import { useEffect, useState } from 'preact/hooks';

const STORAGE_KEY = 'rate-weekdays';

// 保存していなければ、土日祝日には使わない前提で数える。
const load = (): boolean => {
  try {
    return localStorage.getItem(STORAGE_KEY) !== '0';
  } catch {
    return true;
  }
};

let weekdays = load();
const listeners = new Set<(on: boolean) => void>();

export const setRateWeekdays = (on: boolean): void => {
  weekdays = on;
  try {
    localStorage.setItem(STORAGE_KEY, on ? '1' : '0');
  } catch {
    // 保存できなくても、開いている間は切り替えられる。
  }
  listeners.forEach((fn) => fn(on));
};

export const useRateWeekdays = (): boolean => {
  const [on, setOn] = useState(weekdays);
  useEffect(() => {
    listeners.add(setOn);
    return () => {
      listeners.delete(setOn);
    };
  }, []);
  return on;
};
