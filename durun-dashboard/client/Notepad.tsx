// 文字を書き留める「メモ帳」。Windows XP のメモ帳に倣い、スキンによらず XP の見た目で描く。書いた文字はウィンドウの設定に持ち、閉じるまで残す。
import { Fragment } from 'preact';
import { useRef } from 'preact/hooks';
import { DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「notepad:開いた時刻」にし、いくつも開けるようにする。 */
export const NOTEPAD_KEY = 'notepad';
export const NOTEPAD_TITLE = 'メモ帳';
export const NOTEPAD_ICON = '/notepad.svg';
const SIZE = { w: 560, h: 400 };

export const openNotepad = (): void => floatUp(NOTEPAD_KEY, '', { id: `${NOTEPAD_KEY}:${Date.now()}` }, SIZE);

export const NotepadIcon = () => <DesktopIcon icon={NOTEPAD_ICON} title={NOTEPAD_TITLE} hint="文字を書き留めます。ダブルクリックで開きます" onLaunch={openNotepad} />;

/** XP のメモ帳の「日付と時刻」と同じく、「時:分 年/月/日」の形にする。 */
const stamp = (d: Date): string => `${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')} ${d.toLocaleDateString('ja-JP')}`;

export const NotepadFloat = ({ f }: { f: Float }) => {
  const text = f.tab.settings?.text ?? '';
  const wrap = f.tab.settings?.wrap !== 'off';
  const area = useRef<HTMLTextAreaElement>(null);
  const save = (patch: Record<string, string>) => setFloatSettings(f, { ...f.tab.settings, ...patch });
  const insertStamp = () => {
    const el = area.current;
    if (!el) return;
    el.focus();
    // setRangeText ではなく execCommand で入れ、Cmd/Ctrl + Z で取り消せるようにする。
    document.execCommand('insertText', false, stamp(new Date()));
  };
  const clear = () => {
    if (!text || confirm('書いた内容を消して、新しく書き始めますか？')) save({ text: '' });
  };
  const menuId = (name: string) => `notepad-${f.tab.id}-${name}`;
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  const menus = [
    { name: 'file', label: 'ファイル(F)', items: [{ label: '新規(N)', onClick: clear }] },
    { name: 'edit', label: '編集(E)', items: [{ label: '日付と時刻(D)', key: 'F5', onClick: insertStamp }] },
    { name: 'format', label: '書式(O)', items: [{ label: '右端で折り返す(W)', checked: wrap, onClick: () => save({ wrap: wrap ? 'off' : 'on' }) }] },
  ];
  return (
    <FloatFrame f={f} title={`無題 - ${NOTEPAD_TITLE}`} icon={NOTEPAD_ICON} controls={[close]}>
      <div class="notepad">
        <nav class="notepad-menubar">
          {menus.map((m) => (
            <Fragment key={m.name}>
              <button class="notepad-menu" popovertarget={menuId(m.name)}>{m.label}</button>
              <div id={menuId(m.name)} class="notepad-dropdown" popover onClick={(e) => (e.target as Element).closest('button') && e.currentTarget.hidePopover()}>
                {m.items.map((i) => (
                  <button key={i.label} class="notepad-item" role={'checked' in i ? 'menuitemcheckbox' : 'menuitem'} aria-checked={'checked' in i ? i.checked : undefined} onClick={i.onClick}>
                    {i.label}
                    {'key' in i && <kbd>{i.key}</kbd>}
                  </button>
                ))}
              </div>
            </Fragment>
          ))}
        </nav>
        <textarea
          ref={area}
          class="notepad-text"
          aria-label={NOTEPAD_TITLE}
          spellcheck={false}
          wrap={wrap ? 'soft' : 'off'}
          value={text}
          onInput={(e) => save({ text: e.currentTarget.value })}
          onKeyDown={(e) => {
            if (e.key !== 'F5') return;
            // F5 で画面を読み込み直さないよう止める。
            e.preventDefault();
            insertStamp();
          }}
        />
      </div>
    </FloatFrame>
  );
};
