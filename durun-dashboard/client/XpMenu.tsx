// Windows XP のアプリのメニュー。押したボタンの真下に、項目を並べた popover を開く。メモ帳と Internet Explorer と、通知領域の使用制限が使う。
import type { ComponentChildren } from 'preact';
import { useEffect } from 'preact/hooks';

/** メニューの項目。onClick が無い項目は、XP と同じく灰色にして押せなくする。'-' は区切り線。checked を持つ項目は、印を付けて選んでいることを示す。 */
export type MenuItem = '-' | { label: string; key?: string; checked?: boolean; onClick?: () => void };

/** 項目を並べた popover。開くボタンは popovertarget に id を渡す。項目を押したら閉じる。 */
export const XpDropdown = ({ id, items }: { id: string; items: MenuItem[] }) => (
  <div id={id} class="xp-dropdown" popover onClick={(e) => (e.target as Element).closest('button') && e.currentTarget.hidePopover()}>
    {items.map((i, n) =>
      i === '-' ? (
        <hr key={n} class="xp-sep" />
      ) : (
        <button key={n} class="xp-item" role={i.checked === undefined ? 'menuitem' : 'menuitemcheckbox'} aria-checked={i.checked} disabled={!i.onClick} onClick={i.onClick}>
          <span>{i.label}</span>
          {i.key && <kbd>{i.key}</kbd>}
        </button>
      ),
    )}
  </div>
);

type XpMenuProps = { id: string; class: string; label: ComponentChildren; title?: string; disabled?: boolean; items: MenuItem[] };

export const XpMenu = ({ id, class: cls, label, title, disabled, items }: XpMenuProps) => (
  <>
    <button class={cls} title={title} aria-label={title} disabled={disabled} popovertarget={id}>{label}</button>
    <XpDropdown id={id} items={items} />
  </>
);

/** 右クリックした位置に開くメニュー。開き直すたびに popover を作り直してカーソルの位置に開き、閉じたら onClose を呼ぶ。 */
export const XpContextMenu = ({ id, x, y, items, onClose }: { id: string; x: number; y: number; items: MenuItem[]; onClose: () => void }) => {
  // toggle はバブリングしないので、popover で受ける。
  useEffect(() => {
    const el = document.getElementById(id);
    if (!el) return;
    const onToggle = (e: Event) => (e as ToggleEvent).newState === 'closed' && onClose();
    el.addEventListener('toggle', onToggle);
    el.showPopover();
    return () => el.removeEventListener('toggle', onToggle);
  }, [x, y]);
  return (
    <div class="context-menu" key={`${x},${y}`} style={`--x: ${x}px; --y: ${y}px`}>
      <XpDropdown id={id} items={items} />
    </div>
  );
};
