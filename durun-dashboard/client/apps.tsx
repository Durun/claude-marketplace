// 開いている画面のダッシュボードが持つアプリ。ダッシュボードが届け、スタートメニューの「アプリ」に並べる。
import { useEffect, useState } from 'preact/hooks';

export type AppEntry = { kind: string; title: string; icon: string; launch: () => void };

let apps: AppEntry[] = [];
const listeners = new Set<(list: AppEntry[]) => void>();

export const setApps = (next: AppEntry[]): void => {
  apps = next;
  listeners.forEach((fn) => fn(next));
};

export const useApps = (): AppEntry[] => {
  const [list, setList] = useState(apps);
  useEffect(() => {
    // 描いてから待ち受けるまでの間に届いた分を取りこぼさない。
    setList(apps);
    listeners.add(setList);
    return () => {
      listeners.delete(setList);
    };
  }, []);
  return list;
};

type DesktopIconProps = { icon: string; title: string; hint: string; busy?: boolean; shortcut?: boolean; onLaunch: () => void };

/** ダッシュボードに置くアプリのアイコン。デスクトップのアプリケーションアイコンのように、ダブルクリックか Enter で起動する。icon は画像の URL。shortcut なら、アイコンの右下にショートカットの矢印を重ねる。 */
export const DesktopIcon = ({ icon, title, hint, busy, shortcut, onLaunch }: DesktopIconProps) => (
  <button class="desktop-icon" aria-busy={busy} title={hint} onDblClick={onLaunch} onKeyDown={(e) => e.key === 'Enter' && onLaunch()}>
    {/* 画像をつかむとブラウザのドラッグが始まり、アイコンを動かせなくなるので止める。 */}
    <img src={icon} alt="" width={32} height={32} draggable={false} />
    {shortcut && <i class="desktop-icon-shortcut" aria-hidden="true" />}
    <span>{title}</span>
  </button>
);

/** メニューや一覧に添えるアイコン。icon は絵文字か、/ で始まる画像の URL。 */
export const AppIcon = ({ icon, size = 16 }: { icon: string; size?: number }) =>
  icon.startsWith('/') ? (
    <img class="app-icon" src={icon} alt="" width={size} height={size} />
  ) : (
    <span class="app-icon" style={`font-size: ${size}px`} aria-hidden="true">{icon}</span>
  );
