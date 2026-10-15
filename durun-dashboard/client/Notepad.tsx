// 文字を書き留める「メモ帳」。Windows XP のメモ帳に倣い、スキンによらず XP の見た目で描く。書いた文字はウィンドウの設定に持ち、閉じるまで残す。
// PC の中のテキストのファイルを開いて編集し、上書き保存できる。
import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { baseName } from './explorerFavorites.ts';
import { type Float, FloatFrame, dock, floatUp, latestFloat, setFloatSettings } from './Float.tsx';
import { highlightLine, languageOf } from './highlight.ts';
import { useFetch } from './hooks.ts';
import { OPEN_SHORTCUT, SAVE_SHORTCUT, isShortcut, shortcutLabel } from './shortcut.ts';
import { toast } from './toast.tsx';
import { XpMenu } from './XpMenu.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「notepad:開いた時刻」にし、いくつも開けるようにする。 */
export const NOTEPAD_KEY = 'notepad';
export const NOTEPAD_TITLE = 'メモ帳';
export const NOTEPAD_ICON = '/notepad.svg';
const SIZE = { w: 560, h: 400 };

export const openNotepad = (): void => floatUp(NOTEPAD_KEY, '', { id: `${NOTEPAD_KEY}:${Date.now()}` }, SIZE);

export const NotepadIcon = () => <DesktopIcon icon={NOTEPAD_ICON} title={NOTEPAD_TITLE} hint="文字を書き留めます。ダブルクリックで開きます" onLaunch={openNotepad} />;

/** XP のメモ帳の「日付と時刻」と同じく、「時:分 年/月/日」の形にする。 */
const stamp = (d: Date): string => `${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')} ${d.toLocaleDateString('ja-JP')}`;

const RECENT_KEY = 'notepad-recent';
// Windows XP の「最近使ったファイル」と同じく、新しい順に 15 件まで出す。
const MAX_RECENT = 15;

/** 最近開いたファイルのパス。ブラウザに保存し、どのメモ帳のウィンドウからも開けるようにする。 */
const recentFiles = (): string[] => {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]') as string[];
  } catch {
    return [];
  }
};

const remember = (path: string): void => {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([path, ...recentFiles().filter((p) => p !== path)].slice(0, MAX_RECENT)));
  } catch {
    // 保存できなくても、開いたファイルは編集できる。
  }
};

/** ファイルの入ったフォルダー。区切りを残し、ルートの直下のファイルでもルートを指す。 */
const folderOf = (path?: string): string | undefined => path?.slice(0, path.lastIndexOf('/') + 1) || undefined;

// 打つたびに全行を色付けし直すと、長いファイルで入力が遅れるので、行ごとの結果を覚えておく。
// ponytail: 覚えた行が増えすぎたら全部捨てる。捨てた直後の色付けが重くなるなら、古い順に捨てる形にする。
const MAX_PAINTED = 20_000;
const painted = new Map<string, string>();

const paint = (line: string, lang: string): string => {
  const key = `${lang}\n${line}`;
  let html = painted.get(key);
  if (html === undefined) {
    if (painted.size >= MAX_PAINTED) painted.clear();
    html = highlightLine(line, lang);
    painted.set(key, html);
  }
  return html;
};

type OpenDialogProps = { from?: string; onOpen: (path: string) => void; onClose: () => void };

/**
 * XP の「ファイルを開く」。一覧のフォルダーはダブルクリックか Enter で中へ進み、ファイルは同じ操作で開く。
 * 場所の欄にフォルダーのパスを打つとその中を並べ、ファイルのパスを打つとそのファイルを開く。
 */
const OpenDialog = ({ from, onOpen, onClose }: OpenDialogProps) => {
  const dialog = useRef<HTMLDialogElement>(null);
  const [dir, setDir] = useState(from);
  const listing = useFetch(() => api.explorer(dir), [dir]);
  // 場所の欄に打ちかけた文字と、一覧で選んだもののパス。フォルダーを移ったら、どちらも捨てる。
  const [typed, setTyped] = useState<string>();
  const [picked, setPicked] = useState<string>();
  useEffect(() => dialog.current?.showModal(), []);
  const go = (next: string) => {
    setDir(next);
    setTyped(undefined);
    setPicked(undefined);
  };
  const choose = (path: string) => {
    dialog.current?.close();
    onOpen(path);
  };
  const enter = (path: string) => {
    const e = listing.data?.entries.find((x) => x.path === path);
    if (e) (e.dir ? go : choose)(e.path);
  };
  return (
    <dialog ref={dialog} class="notepad-open" aria-label="ファイルを開く" closedby="any" onClose={onClose}>
      <h3 class="notepad-open-title">ファイルを開く</h3>
      <form
        class="notepad-open-bar"
        onSubmit={(e) => {
          e.preventDefault();
          const p = typed?.trim();
          if (p) api.explorer(p).then(() => go(p), () => choose(p));
        }}
      >
        <input class="notepad-open-path" aria-label="ファイルの場所" spellcheck={false} value={typed ?? listing.data?.path ?? ''} onInput={(e) => setTyped(e.currentTarget.value)} />
        <button type="button" title="1 つ上のフォルダーへ" aria-label="1 つ上のフォルダーへ" disabled={!listing.data?.parent} onClick={() => listing.data?.parent && go(listing.data.parent)}>↑</button>
      </form>
      {/* 名前の頭の文字を打って選べるよう、フォルダーはアイコンではなく末尾の / で見分ける。 */}
      <select
        class="notepad-open-list"
        size={12}
        aria-label="フォルダーの中身"
        autofocus
        onChange={(e) => setPicked(e.currentTarget.value)}
        onDblClick={(e) => enter(e.currentTarget.value)}
        onKeyDown={(e) => e.key === 'Enter' && enter(e.currentTarget.value)}
      >
        {listing.data?.entries.map((e) => <option key={e.path} value={e.path}>{e.dir ? `${e.name}/` : e.name}</option>)}
      </select>
      {listing.error && <p class="notepad-open-error">{listing.error}</p>}
      <div class="notepad-open-actions">
        <button disabled={!picked} onClick={() => picked && enter(picked)}>開く(O)</button>
        <button onClick={() => dialog.current?.close()}>キャンセル</button>
      </div>
    </dialog>
  );
};

/**
 * ファイルを開いたウィンドウは、設定の path に開いたファイルを、modifiedAt に読んだときの更新時刻を持つ。dirty は、開いた後に書き換えて、まだ保存していないとき。
 * 行番号と色付けは、textarea の後ろに同じ字並びの行を描き、スクロールの位置を写して重ねる。
 */
export const NotepadFloat = ({ f }: { f: Float }) => {
  const s = f.tab.settings ?? {};
  const text = s.text ?? '';
  const wrap = s.wrap !== 'off';
  const numbers = s.lineNumbers !== 'off';
  const colors = s.highlight !== 'off';
  const edited = !!s.path && s.dirty === 'on';
  // 消すと戻せない内容があるか。ファイルを開いていれば保存していない変更、開いていなければ書いた文字のすべてを指す。
  const unsaved = s.path ? edited : !!text;
  const area = useRef<HTMLTextAreaElement>(null);
  const view = useRef<HTMLDivElement>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [picking, setPicking] = useState(false);
  // IME で変換している間は、変換中の下線を見せるため、色付けをやめて textarea の字を見せる。
  const [composing, setComposing] = useState(false);
  // ファイルを読んだり保存したりする間に設定が変わっていても、今の設定に重ねる。
  const update = (patch: Record<string, string>) => {
    const cur = latestFloat(f);
    if (cur) setFloatSettings(cur, { ...cur.tab.settings, ...patch });
  };
  const sync = () => {
    const a = area.current;
    const v = view.current;
    if (!a || !v) return;
    v.scrollTop = a.scrollTop;
    v.scrollLeft = a.scrollLeft;
  };
  // 打った字で行が増えると、後ろに描いた行は、描き直した後でないと textarea と同じ位置まで送れない。
  useLayoutEffect(sync);

  const insertStamp = () => {
    const el = area.current;
    if (!el) return;
    el.focus();
    // setRangeText ではなく execCommand で入れ、Cmd/Ctrl + Z で取り消せるようにする。
    document.execCommand('insertText', false, stamp(new Date()));
  };
  const clear = () => {
    if (!unsaved || confirm('書いた内容を消して、新しく書き始めますか？')) update({ text: '', path: '', dirty: 'off' });
  };
  /** UTF-8 のテキストでないファイルは、読んだ字のまま上書きすると中身が壊れるので開かない。 */
  const open = async (path: string) => {
    if (unsaved && !confirm('書いた内容を消して、ほかのファイルを開きますか？')) return;
    try {
      const file = await api.localFile('', path);
      if (/[\0\uFFFD]/.test(file.content)) throw new Error('UTF-8 のテキストではありません');
      // textarea は改行を LF にそろえるので、後ろに描く行もそろえる。
      update({ path: file.path, text: file.content.replace(/\r\n?/g, '\n'), modifiedAt: String(file.modifiedAt), dirty: 'off' });
      remember(file.path);
    } catch (e) {
      toast(`「${baseName(path)}」を開けませんでした（${(e as Error).message}）`, 'error');
    }
  };
  /** 開いた後にほかで書き換えられていたら、上書きしてよいかを聞く。 */
  const write = async (path: string, modifiedAt: number): Promise<void> => {
    try {
      const saved = await api.saveLocalFile(path, text, modifiedAt);
      if (saved.changed) {
        if (confirm(`「${baseName(path)}」は、開いた後にほかで書き換えられています。上書きしますか？`)) await write(path, saved.modifiedAt);
        return;
      }
      // 保存している間に打った分は、まだ保存していないものとして残す。
      update({ modifiedAt: String(saved.modifiedAt), dirty: latestFloat(f)?.tab.settings?.text === text ? 'off' : 'on' });
    } catch (e) {
      toast(`「${baseName(path)}」を保存できませんでした（${(e as Error).message}）`, 'error');
    }
  };
  const saveFile = () => s.path && write(s.path, Number(s.modifiedAt));

  const menuId = (name: string) => `notepad-${f.tab.id}-${name}`;
  const close = {
    kind: 'close' as const,
    label: '閉じる',
    onClick: () => (!edited || confirm('保存していない変更を捨てて、閉じますか？')) && dock(f.key, f.scope, f.tab.id),
  };
  const menus = [
    { name: 'edit', label: '編集(E)', items: [{ label: '日付と時刻(D)', key: 'F5', onClick: insertStamp }] },
    { name: 'format', label: '書式(O)', items: [{ label: '右端で折り返す(W)', checked: wrap, onClick: () => update({ wrap: wrap ? 'off' : 'on' }) }] },
    {
      name: 'view',
      label: '表示(V)',
      items: [
        { label: '行番号(L)', checked: numbers, onClick: () => update({ lineNumbers: numbers ? 'off' : 'on' }) },
        { label: 'シンタックスハイライト(H)', checked: colors, onClick: () => update({ highlight: colors ? 'off' : 'on' }) },
      ],
    },
  ];
  const lang = colors && s.path ? languageOf(s.path) : undefined;
  const lines = numbers || lang ? text.split('\n') : [];
  const editorClass = `notepad-editor${wrap ? '' : ' is-nowrap'}${lang && !composing ? ' is-colored' : ''}`;
  return (
    <FloatFrame f={f} title={`${edited ? '*' : ''}${s.path ? baseName(s.path) : '無題'} - ${NOTEPAD_TITLE}`} icon={NOTEPAD_ICON} controls={[close]}>
      <div class="notepad">
        <nav class="notepad-menubar">
          <button class="xp-menu" popovertarget={menuId('file')}>ファイル(F)</button>
          {/* 最近使ったファイルは、ファイルのメニューの中から開く入れ子の popover にする。項目を押したら、入れ子ごと閉じる。 */}
          <div
            id={menuId('file')}
            class="xp-dropdown"
            popover
            onToggle={(e) => e.newState === 'open' && setRecent(recentFiles())}
            onClick={(e) => {
              const item = (e.target as Element).closest('button');
              if (item && !item.hasAttribute('popovertarget')) e.currentTarget.hidePopover();
            }}
          >
            <button class="xp-item" onClick={clear}><span>新規(N)</span></button>
            <button class="xp-item" onClick={() => setPicking(true)}><span>開く(O)...</span><kbd>{shortcutLabel(OPEN_SHORTCUT)}</kbd></button>
            <button class="xp-item" disabled={!s.path} onClick={saveFile}><span>上書き保存(S)</span><kbd>{shortcutLabel(SAVE_SHORTCUT)}</kbd></button>
            <hr class="xp-sep" />
            <button class="xp-item xp-submenu" popovertarget={menuId('recent')}><span>最近使ったファイル(F)</span></button>
            <div id={menuId('recent')} class="xp-dropdown" popover>
              {recent.map((p) => (
                <button key={p} class="xp-item" title={p} onClick={() => open(p)}>
                  <span>{baseName(p)}</span>
                  <kbd>{baseName(folderOf(p) ?? '')}</kbd>
                </button>
              ))}
              {!recent.length && <button class="xp-item" disabled><span>（なし）</span></button>}
            </div>
          </div>
          {menus.map((m) => <XpMenu key={m.name} id={menuId(m.name)} class="xp-menu" label={m.label} items={m.items} />)}
        </nav>
        <div class={editorClass} style={numbers ? `--gutter: ${String(lines.length).length + 1}ch` : undefined}>
          {lines.length > 0 && (
            <div ref={view} class="notepad-view" aria-hidden="true">
              {lines.map((line, i) =>
                lang ? (
                  <div key={i} class="notepad-line" data-n={numbers ? i + 1 : undefined} dangerouslySetInnerHTML={{ __html: paint(line, lang) }} />
                ) : (
                  <div key={i} class="notepad-line" data-n={numbers ? i + 1 : undefined}>{line}</div>
                ),
              )}
            </div>
          )}
          <textarea
            ref={area}
            class="notepad-text"
            aria-label={NOTEPAD_TITLE}
            spellcheck={false}
            wrap={wrap ? 'soft' : 'off'}
            value={text}
            onInput={(e) => update({ text: e.currentTarget.value, dirty: 'on' })}
            onScroll={sync}
            onCompositionStart={() => setComposing(true)}
            onCompositionEnd={() => setComposing(false)}
            onKeyDown={(e) => {
              const action = e.key === 'F5' ? insertStamp : isShortcut(e, SAVE_SHORTCUT) ? saveFile : isShortcut(e, OPEN_SHORTCUT) ? () => setPicking(true) : undefined;
              if (!action) return;
              // ブラウザの読み込み直し、ページの保存、ファイルを開く操作に取られないよう止める。
              e.preventDefault();
              action();
            }}
          />
        </div>
      </div>
      {picking && <OpenDialog from={folderOf(s.path || recentFiles()[0])} onOpen={open} onClose={() => setPicking(false)} />}
    </FloatFrame>
  );
};
