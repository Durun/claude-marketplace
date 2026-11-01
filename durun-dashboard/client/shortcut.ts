// キーボードショートカットの判定と、メニューの項目に添える表記。
// Mac の入力欄では Ctrl と文字の組み合わせが行の編集の操作なので、Mac では Cmd と組み合わせる。
const IS_MAC = /Mac|iPhone|iPad/.test(navigator.userAgent);

/** key は小文字で書く。 */
type Shortcut = { key: string; shift?: boolean };

export const SEARCH_SHORTCUT: Shortcut = { key: 'k' };
export const EDIT_SHORTCUT: Shortcut = { key: 'e', shift: true };
export const OPEN_SHORTCUT: Shortcut = { key: 'o' };
export const SAVE_SHORTCUT: Shortcut = { key: 's' };
export const HOME_SHORTCUT: Shortcut = { key: 'h' };
export const FULLSCREEN_SHORTCUT: Shortcut = { key: 'f' };

export const shortcutLabel = ({ key, shift }: Shortcut): string =>
  IS_MAC ? `${shift ? '⇧' : ''}⌘${key.toUpperCase()}` : `Ctrl+${shift ? 'Shift+' : ''}${key.toUpperCase()}`;

/** Chrome は、自動入力や datalist の候補を選んだときに、key の無い keydown を送る。 */
export const isShortcut = (e: KeyboardEvent, { key, shift = false }: Shortcut): boolean =>
  e.key?.toLowerCase() === key && (IS_MAC ? e.metaKey : e.ctrlKey) && e.shiftKey === shift && !e.isComposing;

// 修飾キーを素早く 2 回叩いたときだけ受ける。押し続けたときと、間にほかのキーやクリックを挟んだときは数えず、ショートカットの打ち始めと見分ける。
const TAP_MS = 300;
const DOUBLE_TAP_MS = 400;

/** Cmd（Mac 以外では Ctrl）を 2 回続けて叩いたら fn を呼ぶ。戻り値の関数を呼ぶと受けるのをやめる。 */
export const onDoubleTap = (fn: () => void): (() => void) => {
  const modifier = IS_MAC ? 'Meta' : 'Control';
  // 押した時刻と、前に叩き終えた時刻。数えないものを挟んだら 0 に戻す。
  let down = 0;
  let tapped = 0;
  const reset = () => {
    down = 0;
    tapped = 0;
  };
  const onDown = (e: KeyboardEvent) => {
    if (e.key !== modifier) reset();
    else if (!e.repeat) down = e.timeStamp;
  };
  const onUp = (e: KeyboardEvent) => {
    if (e.key !== modifier || !down) return;
    const tap = e.timeStamp - down < TAP_MS;
    down = 0;
    if (tap && tapped && e.timeStamp - tapped < DOUBLE_TAP_MS) {
      tapped = 0;
      fn();
    } else tapped = tap ? e.timeStamp : 0;
  };
  // 端末やエディターがキーを止めても数えられるよう、先に受ける。Cmd + Tab でほかのアプリへ移ったら、離したことが届かないので数え直す。
  addEventListener('keydown', onDown, true);
  addEventListener('keyup', onUp, true);
  addEventListener('pointerdown', reset, true);
  addEventListener('blur', reset);
  return () => {
    removeEventListener('keydown', onDown, true);
    removeEventListener('keyup', onUp, true);
    removeEventListener('pointerdown', reset, true);
    removeEventListener('blur', reset);
  };
};
