import { createPortal } from 'preact/compat';
import { useEffect, useRef } from 'preact/hooks';
import { AppIcon } from './apps.tsx';

/**
 * 押し間違いでないかの確認。XP の警告のメッセージボックスに倣う。「はい」で閉じたときだけ ok を真にし、× と Esc は「いいえ」と同じに扱う。
 * ブラウザの confirm は答えるまで画面ごと止まるので、ページの中のダイアログで尋ねる。
 */
export const XpConfirm = ({ title, text, onClose }: { title: string; text: string; onClose: (ok: boolean) => void }) => {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => dialog.current?.showModal(), []);
  return (
    <dialog ref={dialog} class="xp-confirm" role="alertdialog" aria-label={title} onClose={(e) => onClose(e.currentTarget.returnValue === 'yes')}>
      <form method="dialog">
        <h3 class="xp-confirm-title">{title}</h3>
        <button class="xp-confirm-close" aria-label="閉じる" />
        <p class="xp-confirm-text"><AppIcon icon="⚠️" size={32} />{text}</p>
        <div class="xp-confirm-actions">
          <button value="yes" autofocus>はい(<u>Y</u>)</button>
          <button>いいえ(<u>N</u>)</button>
        </div>
      </form>
    </dialog>
  );
};

/**
 * ブラウザの prompt の代わりに、ページの中で 1 行の文字を尋ねる。OK で閉じたときだけ打った文字を渡し、キャンセル、×、Esc では null を渡す。
 * 編集中のアイコンは枠ごとドラッグで動くので、欄で文字を選ぶ操作がアイコンに届かないよう、body の直下に出す。
 */
export const XpPrompt = ({ title, text, value, onClose }: { title: string; text: string; value: string; onClose: (value: string | null) => void }) => {
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // ブラウザの prompt と同じく、初めの文字を選んでおき、打てばすぐ置き換わるようにする。
  useEffect(() => {
    dialog.current?.showModal();
    input.current?.select();
  }, []);
  return createPortal(
    <dialog ref={dialog} class="xp-confirm" aria-label={title} onClose={(e) => onClose(e.currentTarget.returnValue === 'ok' ? (input.current?.value ?? '') : null)}>
      <form method="dialog">
        <h3 class="xp-confirm-title">{title}</h3>
        <label class="xp-prompt">
          {text}
          <input ref={input} class="xp-prompt-input" defaultValue={value} />
        </label>
        {/* 欄で Enter を押すと form の最初のボタンを押したことになるので、OK を × より前に置く。 */}
        <div class="xp-confirm-actions">
          <button value="ok">OK</button>
          <button>キャンセル</button>
        </div>
        <button class="xp-confirm-close" aria-label="閉じる" />
      </form>
    </dialog>,
    document.body,
  );
};
