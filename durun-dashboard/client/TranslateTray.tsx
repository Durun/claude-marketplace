// 翻訳アプリを最小化している間は、通知領域に常駐する。その間、チャット・PR・JIRA・用語集・概要の文を右クリックすると「翻訳」が出て、選ぶとその文を窓で選んだ訳し方の訳文に置き換える。
// 訳文は画面の中だけに持ち、原文で覚える。同じ文はどこに出ても訳文で出し、読み込み直すと原文に戻る。
import type { ComponentChildren } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { attempt } from './api.ts';
import { type Float, useFloats } from './Float.tsx';
import { useWaitCursor } from './hooks.ts';
import { AUTO, TRANSLATE_ICON, TRANSLATE_KEY, openTranslate, translateIn } from './Translate.tsx';
import { XpContextMenu } from './XpMenu.tsx';

type Shown = { text?: string; busy?: boolean };

const shown = new Map<string, Shown>();
const listeners = new Set<() => void>();

const setShown = (source: string, next: Shown | undefined): void => {
  if (next) shown.set(source, next);
  else shown.delete(source);
  listeners.forEach((fn) => fn());
};

/** 自分の原文の表示が変わったときだけ描き直す。チャットの発言の数だけ並ぶので、ほかの文の翻訳で描き直さない。 */
const useShown = (source: string): Shown | undefined => {
  const [cur, setCur] = useState(() => shown.get(source));
  useEffect(() => {
    const fn = () => setCur(shown.get(source));
    fn();
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, [source]);
  return cur;
};

/** 常駐している翻訳の窓。最小化していなければ undefined。 */
let resident: Float | undefined;

type Menu = { x: number; y: number; source: string };
let openMenu: ((m: Menu) => void) | undefined;

const MENU_ID = 'translate-menu';

const translate = async (source: string): Promise<void> => {
  const prev = shown.get(source);
  setShown(source, { ...prev, busy: true });
  const text = await attempt('翻訳', () => translateIn(resident?.tab.settings?.target ?? AUTO, source));
  setShown(source, text === undefined ? prev : { text });
};

/**
 * 右クリックで訳せる文。children に、今見せる文（訳していれば訳文）を渡す。
 * 常駐していない間は、ブラウザの右クリックのメニューをそのまま出す。
 */
export const Translatable = ({ text, children }: { text: string; children: (text: string) => ComponentChildren }) => {
  const s = useShown(text);
  useWaitCursor(!!s?.busy);
  return (
    <span
      class="translatable"
      onContextMenu={(e) => {
        if (!resident || !openMenu || !text.trim()) return;
        e.preventDefault();
        openMenu({ x: e.clientX, y: e.clientY, source: text });
      }}
    >
      {children(s?.text ?? text)}
    </span>
  );
};

/** 通知領域のアイコンと、右クリックのメニュー。翻訳の窓を最小化している間だけ出す。アイコンを押すと窓を戻す。 */
export const TranslateTray = () => {
  const f = useFloats((list) => list.find((x) => x.key === TRANSLATE_KEY && x.min));
  const [menu, setMenu] = useState<Menu>();
  useEffect(() => {
    resident = f;
    openMenu = f ? setMenu : undefined;
  }, [f]);
  if (!f) return null;
  const translated = menu && shown.get(menu.source)?.text !== undefined;
  return (
    <>
      <button class="translate-tray" title="翻訳: 文を右クリックして訳します。押すと窓を開きます" aria-label="翻訳の窓を開く" onClick={openTranslate}>
        <img src={TRANSLATE_ICON} alt="" width={18} height={18} />
      </button>
      {menu && (
        <XpContextMenu
          id={MENU_ID}
          x={menu.x}
          y={menu.y}
          items={[{ label: '翻訳', onClick: () => void translate(menu.source) }, ...(translated ? [{ label: '原文に戻す', onClick: () => setShown(menu.source, undefined) }] : [])]}
          onClose={() => setMenu(undefined)}
        />
      )}
    </>
  );
};
