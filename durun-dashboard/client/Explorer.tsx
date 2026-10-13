// PC の中のファイルを一覧する「エクスプローラー」。ウィンドウに浮かべて開き、開いているフォルダーはウィンドウごとに保存する。
import { useEffect, useState } from 'preact/hooks';
import type { ExplorerEntry } from '../shared/types.ts';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { openFile } from './Docs.tsx';
import { baseName, toggleFavorite, useFavorites } from './explorerFavorites.ts';
import { FILE_DRAG_TYPE, mediaTypeOf } from './fileRef.ts';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { clamp } from './layout.ts';
import { dateTime, fmtBytes } from './format.ts';
import { useFetch } from './hooks.ts';
import { currentSessionId } from './router.ts';
import { Empty } from './ui.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「explorer:開いた時刻」にし、いくつも開けるようにする。 */
export const EXPLORER_KEY = 'explorer';
export const EXPLORER_TITLE = 'エクスプローラー';
export const EXPLORER_ICON = '/explorer.svg';
const SIZE = { w: 760, h: 500 };

/** 新しいウィンドウで dir を開く。dir を省くと、ホームディレクトリを開く。 */
export const openExplorer = (dir?: string): void =>
  floatUp(EXPLORER_KEY, '', { id: `${EXPLORER_KEY}:${Date.now()}`, settings: dir ? { path: dir } : undefined }, SIZE);

/** ダッシュボードに置くアイコン。セッションに置いたものは、そのセッションの作業ディレクトリを開く。 */
export const ExplorerIcon = ({ cwd }: { cwd?: string }) => (
  <DesktopIcon icon={EXPLORER_ICON} title={EXPLORER_TITLE} hint="PC の中のファイルを一覧します。ダブルクリックで開きます" onLaunch={() => openExplorer(cwd)} />
);

/**
 * ドラッグで変えられる欄の幅。どのウィンドウでも同じ幅にするため、ウィンドウごとではなくブラウザに 1 つだけ保存する。
 * drag は仕切りの pointerdown に渡す。sign は、仕切りを右へ動かしたときに欄が広がるなら 1、狭まるなら -1。
 */
const useWidth = (key: string, initial: number) => {
  const [width, setWidth] = useState(() => Number(localStorage.getItem(key)) || initial);
  const drag = (e: PointerEvent, sign: 1 | -1) => {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const x0 = e.clientX;
    let next = width;
    const move = (ev: PointerEvent) => setWidth((next = clamp(width + sign * (ev.clientX - x0), 80, 800)));
    el.addEventListener('pointermove', move);
    el.addEventListener(
      'pointerup',
      () => {
        el.removeEventListener('pointermove', move);
        try {
          localStorage.setItem(key, String(next));
        } catch {
          // 保存できなくても、開いている間はその幅で使える。
        }
      },
      { once: true },
    );
  };
  return [width, drag] as const;
};

const iconOf = (e: ExplorerEntry): string => {
  if (e.dir) return '📁';
  const type = mediaTypeOf(e.name);
  return type?.startsWith('video/') ? '🎬' : type ? '🖼️' : '📄';
};

type RowProps = { e: ExplorerEntry; onOpen: () => void };

/** ダブルクリックか Enter で開く。ファイルは、資料や送信欄へドラッグして渡せる。 */
const Row = ({ e, onOpen }: RowProps) => (
  <li
    class="explorer-row"
    tabIndex={0}
    title={e.path}
    draggable={!e.dir}
    onDragStart={(ev) => {
      if (!ev.dataTransfer) return;
      ev.dataTransfer.effectAllowed = 'copy';
      ev.dataTransfer.setData(FILE_DRAG_TYPE, e.path);
      ev.dataTransfer.setData('text/plain', e.path);
    }}
    onDblClick={onOpen}
    onKeyDown={(ev) => ev.key === 'Enter' && onOpen()}
  >
    <span class="explorer-name"><span aria-hidden="true">{iconOf(e)}</span>{e.name}</span>
    <span class="explorer-meta">{e.modifiedAt !== undefined && dateTime(e.modifiedAt)}</span>
    <span class="explorer-meta explorer-size">{e.size !== undefined && fmtBytes(e.size)}</span>
  </li>
);

type PlaceProps = { icon: string; label: string; path: string; current: boolean; onGo: (path: string) => void; onRemove?: () => void };

/** 左に並べる、すぐに開ける場所。お気に入りは × で登録を外せる。 */
const Place = ({ icon, label, path, current, onGo, onRemove }: PlaceProps) => (
  <li class="explorer-place-item">
    <button class="explorer-place" title={path} aria-current={current} onClick={() => onGo(path)}>
      <span aria-hidden="true">{icon}</span>{label}
    </button>
    {onRemove && <button class="side-doc-action" title="お気に入りから外す" aria-label={`${label}をお気に入りから外す`} onClick={onRemove}>×</button>}
  </li>
);

/**
 * フォルダーの中身を並べる。フォルダーはその場で開き、ファイルは、チャットのファイルパスを押したときと同じ開き方で開く。
 * 開いているフォルダーはウィンドウの設定に持ち、読み込み直しても同じフォルダーを出す。
 */
export const ExplorerFloat = ({ f }: { f: Float }) => {
  const path = f.tab.settings?.path;
  const fetched = useFetch(() => api.explorer(path), [path]);
  const dir = fetched.data;
  const favorites = useFavorites();
  // パスの欄に打ちかけた文字。フォルダーを移ったら、そのフォルダーのパスに戻す。
  const [typed, setTyped] = useState<string>();
  useEffect(() => setTyped(undefined), [dir?.path]);
  const go = (next: string) => setFloatSettings(f, { ...f.tab.settings, path: next });
  const open = (e: ExplorerEntry) => (e.dir ? go(e.path) : void openFile(currentSessionId(), e.path));
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  const starred = !!dir && favorites.includes(dir.path);
  const [sideWidth, dragSide] = useWidth('explorer-side-width', 160);
  return (
    <FloatFrame f={f} title={dir ? baseName(dir.path) : EXPLORER_TITLE} icon={EXPLORER_ICON} controls={[close]}>
      <div class="explorer">
        <form
          class="explorer-bar"
          onSubmit={(e) => {
            e.preventDefault();
            if (typed?.trim()) go(typed.trim());
          }}
        >
          <button type="button" class="btn btn-sm" title="1 つ上のフォルダーへ" aria-label="1 つ上のフォルダーへ" disabled={!dir?.parent} onClick={() => dir?.parent && go(dir.parent)}>↑</button>
          <input class="input explorer-path" aria-label="フォルダーのパス" spellcheck={false} value={typed ?? dir?.path ?? path ?? ''} onInput={(e) => setTyped(e.currentTarget.value)} />
          <button type="button" class="btn btn-sm" aria-pressed={starred} disabled={!dir} title={starred ? 'お気に入りから外す' : 'お気に入りに登録する。スタートメニューのエクスプローラーから開けます'} onClick={() => dir && toggleFavorite(dir.path)}>
            {starred ? '★' : '☆'}
          </button>
          <button type="button" class="btn btn-sm" data-icon="refresh" disabled={fetched.loading} onClick={fetched.reload}>更新</button>
        </form>
        <div class="explorer-main" style={{ '--side-w': `${sideWidth}px` }}>
          <nav class="explorer-side" aria-label="場所">
            {favorites.length > 0 && (
              <>
                <h3 class="explorer-heading">お気に入り</h3>
                <ul class="explorer-places">
                  {favorites.map((p) => <Place key={p} icon="⭐" label={baseName(p)} path={p} current={p === dir?.path} onGo={go} onRemove={() => toggleFavorite(p)} />)}
                </ul>
              </>
            )}
            <h3 class="explorer-heading">場所</h3>
            <ul class="explorer-places">
              {/* フォルダーを読めていないときも、ホームへは戻れるようにする。 */}
              <Place icon="🏠" label="ホーム" path={dir?.home ?? '~'} current={!!dir && dir.path === dir.home} onGo={go} />
              {dir?.roots.map((r) => <Place key={r} icon="💽" label={r} path={r} current={r === dir.path} onGo={go} />)}
            </ul>
          </nav>
          <div class="explorer-splitter" role="separator" aria-orientation="vertical" aria-label="場所の幅" onPointerDown={(e) => dragSide(e, 1)} />
          <div class="explorer-list-wrap">
            {fetched.error && <p class="error-note">{fetched.error}</p>}
            {!dir ? (
              !fetched.error && <Empty>読み込んでいます…</Empty>
            ) : dir.entries.length === 0 ? (
              <Empty>このフォルダーは空です。</Empty>
            ) : (
              <ul class="explorer-list">
                {dir.entries.map((e) => <Row key={e.path} e={e} onOpen={() => open(e)} />)}
              </ul>
            )}
            {dir?.truncated && <p class="muted">中身が多いので、途中までを並べています。</p>}
          </div>
        </div>
      </div>
    </FloatFrame>
  );
};
