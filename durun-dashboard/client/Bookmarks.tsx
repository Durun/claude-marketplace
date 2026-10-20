// チャットの ☆ でブックマークした発言を、すべてのセッションから集めて並べる「ブックマーク」。押すと、そのセッションのチャットのその発言へ移る。
import { type Bookmark, type Session, type Snapshot, bookmarkKey } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { plainClick } from './Docs.tsx';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { dateTime } from './format.ts';
import { href } from './router.ts';
import { Empty } from './ui.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。ウィンドウは 1 つだけ開くので、タブの id にもこのキーを入れる。 */
export const BOOKMARKS_KEY = 'bookmarks';
export const BOOKMARKS_TITLE = 'ブックマーク';
export const BOOKMARKS_ICON = '/bookmark.svg';
const SIZE = { w: 480, h: 520 };

export const openBookmarks = (): void => floatUp(BOOKMARKS_KEY, '', { id: BOOKMARKS_KEY }, SIZE);
const close = () => dock(BOOKMARKS_KEY, '', BOOKMARKS_KEY);

export const BookmarksIcon = () => <DesktopIcon icon={BOOKMARKS_ICON} title={BOOKMARKS_TITLE} hint="ブックマークした発言を並べます。ダブルクリックで開きます" onLaunch={openBookmarks} />;

const SPEAKER: Record<Bookmark['kind'], string> = { prompt: '人間', message: 'Claude' };

/** 押したブックマークの発言を開く関数。sessionId のセッションの画面で、bookmarkKey が key の発言へ移る。 */
type OpenAt = (sessionId: string, key: string) => void;

/** 修飾キーつきのクリックでは、ブラウザがセッションの画面を新しいタブで開く。 */
const BookmarkItem = ({ s, b, onOpen }: { s: Session; b: Bookmark; onOpen: OpenAt }) => (
  <li class="side-doc-item">
    <a
      class="side-doc"
      href={href.session(s.id)}
      onClick={(e) => {
        if (!plainClick(e)) return;
        e.preventDefault();
        onOpen(s.id, bookmarkKey(b));
      }}
    >
      <span class="bookmark-text">{b.text}</span>
      <span class="side-doc-meta">
        <span>{SPEAKER[b.kind]}</span>
        <span class="side-doc-sub">{s.archived ? `${s.name}（アーカイブ済み）` : s.name}</span>
        <time dateTime={new Date(b.at).toISOString()}>{dateTime(b.at)}</time>
      </span>
    </a>
    <button class="side-doc-action" title="ブックマークから外す" aria-label="ブックマークから外す" onClick={() => attempt('ブックマークを外す', () => api.bookmark(s.id, b, false))}>
      ×
    </button>
  </li>
);

/** 新しくブックマークしたものから並べる。 */
export const BookmarksFloat = ({ f, snapshot, onOpen }: { f: Float; snapshot: Snapshot; onOpen: OpenAt }) => {
  const items = snapshot.sessions.flatMap((s) => (s.bookmarks ?? []).map((b) => ({ s, b }))).sort((x, y) => y.b.createdAt - x.b.createdAt);
  return (
    <FloatFrame f={f} title={BOOKMARKS_TITLE} icon={BOOKMARKS_ICON} controls={[{ kind: 'close', label: '閉じる', onClick: close }]}>
      <div class="widget-body">
        {items.length === 0 ? (
          <Empty>チャットの発言の ☆ を押すと、ここに並びます。</Empty>
        ) : (
          <ul class="side-docs">
            {items.map(({ s, b }) => <BookmarkItem key={`${s.id} ${bookmarkKey(b)}`} s={s} b={b} onOpen={onOpen} />)}
          </ul>
        )}
      </div>
    </FloatFrame>
  );
};
