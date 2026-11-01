// 人間がセッションと資料を開いた時刻。key は検索窓の候補と同じく「session:id」「doc:id」にする。
// どのタブで開いたものも同じ並びにするため、ブラウザに保存する。
const STORAGE_KEY = 'opened';
// 保存する量が増え続けないよう、古いものから捨てる。
const MAX = 200;

export type Opened = Record<string, number>;

export const loadOpened = (): Opened => {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') as Opened;
  } catch {
    return {};
  }
};

export const markOpened = (key: string): void => {
  const next = Object.entries({ ...loadOpened(), [key]: Date.now() })
    .sort((a, b) => b[1] - a[1])
    .slice(0, MAX);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(next)));
  } catch {
    // 保存できなくても、開くことはできる。
  }
};

/** keys のうち、最後に開いたもの。どれも開いていなければ undefined。 */
export const lastOpened = (keys: string[]): string | undefined => {
  const opened = loadOpened();
  return keys.filter((k) => opened[k]).sort((a, b) => (opened[b] ?? 0) - (opened[a] ?? 0))[0];
};
