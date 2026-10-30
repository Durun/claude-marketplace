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
