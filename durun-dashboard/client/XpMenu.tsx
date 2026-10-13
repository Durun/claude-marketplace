// Windows XP のアプリのメニュー。押したボタンの真下に、項目を並べた popover を開く。メモ帳と Internet Explorer が使う。
import type { ComponentChildren } from 'preact';

/** メニューの項目。onClick が無い項目は、XP と同じく灰色にして押せなくする。'-' は区切り線。checked を持つ項目は、印を付けて選んでいることを示す。 */
export type MenuItem = '-' | { label: string; key?: string; checked?: boolean; onClick?: () => void };

type XpMenuProps = { id: string; class: string; label: ComponentChildren; title?: string; disabled?: boolean; items: MenuItem[] };

/** id は popover の要素の id で、画面の中で一意にする。class は押すボタンの見た目。 */
export const XpMenu = ({ id, class: cls, label, title, disabled, items }: XpMenuProps) => (
  <>
    <button class={cls} title={title} aria-label={title} disabled={disabled} popovertarget={id}>{label}</button>
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
  </>
);
