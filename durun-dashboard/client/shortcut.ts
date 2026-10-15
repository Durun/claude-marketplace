// キーボードショートカットの判定と、メニューの項目に添える表記。
// Mac の入力欄では Ctrl と文字の組み合わせが行の編集の操作なので、Mac では Cmd と組み合わせる。
const IS_MAC = /Mac|iPhone|iPad/.test(navigator.userAgent);

/** key は小文字で書く。 */
type Shortcut = { key: string; shift?: boolean };

export const SEARCH_SHORTCUT: Shortcut = { key: 'k' };
export const EDIT_SHORTCUT: Shortcut = { key: 'e', shift: true };
export const OPEN_SHORTCUT: Shortcut = { key: 'o' };
export const SAVE_SHORTCUT: Shortcut = { key: 's' };

export const shortcutLabel = ({ key, shift }: Shortcut): string =>
  IS_MAC ? `${shift ? '⇧' : ''}⌘${key.toUpperCase()}` : `Ctrl+${shift ? 'Shift+' : ''}${key.toUpperCase()}`;

/** Chrome は、自動入力や datalist の候補を選んだときに、key の無い keydown を送る。 */
export const isShortcut = (e: KeyboardEvent, { key, shift = false }: Shortcut): boolean =>
  e.key?.toLowerCase() === key && (IS_MAC ? e.metaKey : e.ctrlKey) && e.shiftKey === shift && !e.isComposing;
