// Web のサイトをウィンドウの中の iframe で開く「Internet Explorer」。メモ帳と同じく、スキンによらず Windows XP の IE8 の見た目で描く。
// 編集中に設定で URL を決めると、そのサイトのショートカットになり、サイトの favicon に矢印を重ねて描く。
import { useEffect, useRef, useState } from 'preact/hooks';
import { GOOGLE, frameOf } from '../shared/retro.ts';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import type { WidgetProps } from './Dashboard.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { type MenuItem, XpMenu } from './XpMenu.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「ie:開いた時刻」にし、いくつも開けるようにする。 */
export const IE_KEY = 'ie';
export const IE_TITLE = 'Internet Explorer';
export const IE_ICON = '/ie.svg';
const SIZE = { w: 960, h: 640 };

/** url を開いたウィンドウを出す。url が無ければ、Google を開く。 */
export const openInternetExplorer = (url = GOOGLE): void => floatUp(IE_KEY, '', { id: `${IE_KEY}:${Date.now()}`, settings: { url } }, SIZE);

/** 入力を http(s) の URL に限る。javascript: などを開かせない。 */
const siteOf = (input: string): URL | undefined => {
  const url = URL.parse(input.trim());
  return url?.protocol === 'https:' || url?.protocol === 'http:' ? url : undefined;
};

const faviconOf = (site: URL): string => `/api/favicon?${new URLSearchParams({ site: site.origin })}`;

/** アドレスバーの入力を開く URL にする。スキームを省いたドメインは https で開き、それ以外は Google で検索する。 */
const addressOf = (input: string): string => {
  const text = input.trim();
  if (siteOf(text)) return text;
  if (/^[^\s/]+\.[^\s/]{2,}(\/\S*)?$/.test(text)) return `https://${text}`;
  return `${GOOGLE}search?${new URLSearchParams({ q: text })}`;
};

export const InternetExplorerIcon = ({ settings, onSettings }: WidgetProps<object>) => {
  const site = settings.url ? siteOf(settings.url) : undefined;
  const configure = () => {
    const input = prompt('ショートカットにするサイトの URL（空にすると Internet Explorer に戻します）', settings.url ?? '');
    if (input === null) return;
    if (!input.trim()) return onSettings({ ...settings, url: '' });
    const next = siteOf(input);
    if (next) onSettings({ ...settings, url: next.href });
    else alert('http:// か https:// で始まる URL を入れてください');
  };
  return (
    <>
      {site ? (
        <DesktopIcon icon={faviconOf(site)} title={site.hostname} hint={`${site.href} を開きます。ダブルクリックで開きます`} shortcut onLaunch={() => openInternetExplorer(site.href)} />
      ) : (
        <DesktopIcon icon={IE_ICON} title={IE_TITLE} hint="Web のサイトを見ます。ダブルクリックで開きます" onLaunch={() => openInternetExplorer()} />
      )}
      <button class="btn btn-sm desktop-icon-settings" data-icon="settings" onClick={configure}>設定</button>
    </>
  );
};

/** お気に入りバーに並べるページ。ブラウザに保存し、どの IE のウィンドウにも同じものを並べる。 */
type Favorite = { url: string; title: string };
const FAVORITES_KEY = 'ie-favorites';

const loadFavorites = (): Favorite[] => {
  try {
    return JSON.parse(localStorage.getItem(FAVORITES_KEY) ?? '[]') as Favorite[];
  } catch {
    return [];
  }
};

let favorites = loadFavorites();
const favoriteListeners = new Set<(list: Favorite[]) => void>();

const setFavorites = (next: Favorite[]): void => {
  favorites = next;
  try {
    localStorage.setItem(FAVORITES_KEY, JSON.stringify(next));
  } catch {
    // 保存できなくても、開いている間はお気に入りを使える。
  }
  favoriteListeners.forEach((fn) => fn(next));
};

const useFavorites = (): Favorite[] => {
  const [list, setList] = useState(favorites);
  useEffect(() => {
    // 描いてから待ち受けるまでの間に変わった分を取りこぼさない。
    setList(favorites);
    favoriteListeners.add(setList);
    return () => {
      favoriteListeners.delete(setList);
    };
  }, []);
  return list;
};

/**
 * ウィンドウの中のタブ 1 枚。list と at は、戻る・進むのための開いたページの並びと今の位置で、ダッシュボードを開き直すと消える。
 * away は、ほかのサイトの中で移って、今のページが並びに無いこと。src は iframe に読み込ませる URL で、ページの中のリンクで移ったときは変えない。
 * reloads は、同じ URL を開き直すときも iframe を作り直すための数。title は中継が描いたページの題で、ほかのサイトでは分からない。zoom は拡大率（%）。
 */
type Page = { id: number; list: string[]; at: number; away: boolean; src: string; reloads: number; title?: string; zoom: number };

let lastPageId = 0;
const pageOf = (url: string): Page => ({ id: ++lastPageId, list: [url], at: 0, away: false, src: url, reloads: 0, zoom: 100 });
const urlOf = (p: Page): string => p.list[p.at] ?? GOOGLE;
const titleOf = (p: Page): string => p.title || (siteOf(urlOf(p))?.hostname ?? urlOf(p));

/** 並びの今の位置の次に next を足す。進む先は、ブラウザと同じく捨てる。 */
const visited = (p: Page, next: string): Page =>
  urlOf(p) === next ? { ...p, away: false } : { ...p, list: [...p.list.slice(0, p.at + 1), next], at: p.at + 1, away: false };

// IE8 のズームのメニューに並ぶ拡大率（%）。
const ZOOMS = [400, 300, 200, 150, 125, 100, 75, 50];
/** ステータスバーのズームのボタンは、IE8 と同じく、押すたびに 100、125、150 % を巡る。 */
const cycled = (zoom: number): number => (zoom === 100 ? 125 : zoom === 125 ? 150 : 100);

/** public/ie-toolbar.svg に描いた、ツールバーのアイコン。 */
const Icon = ({ name, size = 16 }: { name: string; size?: number }) => (
  <svg class="ie-icon" width={size} height={size} aria-hidden="true">
    <use href={`/ie-toolbar.svg#${name}`} />
  </svg>
);

type NavBarProps = { page: Page; recentId: string; recent: MenuItem[]; onJump: (at: number) => void; onGo: (url: string) => void; onReload: () => void };

/** いちばん上の、戻る・進む、アドレスバー、更新と中止、検索欄の並び。 */
const NavBar = ({ page, recentId, recent, onJump, onGo, onReload }: NavBarProps) => {
  const url = urlOf(page);
  const site = siteOf(url);
  const field = (e: SubmitEvent, name: string): string => {
    e.preventDefault();
    return String(new FormData(e.currentTarget as HTMLFormElement).get(name) ?? '').trim();
  };
  return (
    <div class="ie-nav">
      <button class="ie-travel" title="戻る" aria-label="戻る" disabled={page.at === 0 && !page.away} onClick={() => onJump(page.away ? page.at : page.at - 1)}><Icon name="back" size={27} /></button>
      <button class="ie-travel is-forward" title="進む" aria-label="進む" disabled={page.at === page.list.length - 1} onClick={() => onJump(page.at + 1)}><Icon name="back" size={27} /></button>
      <XpMenu id={recentId} class="ie-drop is-recent" title="最近表示したページ" label="" items={recent} />
      <form class="ie-field ie-address" onSubmit={(e) => {
        const text = field(e, 'address');
        if (text) onGo(addressOf(text));
      }}>
        {site && <img src={faviconOf(site)} alt="" width={16} height={16} />}
        {/* 開いたページが変わるたびに作り直し、打ちかけの文字を開いたページの URL に戻す。 */}
        <input key={`${page.id} ${url}`} name="address" aria-label="アドレス" defaultValue={url} spellcheck={false} onFocus={(e) => e.currentTarget.select()} />
        <button type="button" class="ie-combo" title="最近表示したページ" aria-label="最近表示したページ" popovertarget={recentId} />
      </form>
      {/* IE8 の互換表示のボタンの場所と絵柄で、埋め込みを断るサイトをブラウザの別のタブで開く。 */}
      <button class="ie-tool" title="別のタブで開く" aria-label="別のタブで開く" onClick={() => window.open(url, '_blank', 'noopener')}><Icon name="compat" /></button>
      <button class="ie-tool" title="最新の情報に更新" aria-label="最新の情報に更新" onClick={onReload}><Icon name="refresh" /></button>
      {/* 中のページは別のオリジンなので、読み込みを止められない。 */}
      <button class="ie-tool" title="中止" aria-label="中止" disabled><Icon name="stop" /></button>
      <form class="ie-search" onSubmit={(e) => {
        const q = field(e, 'q');
        if (q) onGo(`${GOOGLE}search?${new URLSearchParams({ q })}`);
      }}>
        <label class="ie-field">
          <Icon name="google" />
          <input name="q" placeholder="Google" aria-label="Google で検索" spellcheck={false} />
        </label>
        <button class="ie-tool" title="検索" aria-label="検索"><Icon name="magnifier" /></button>
        <button type="button" class="ie-tool ie-drop" title="検索のオプション" aria-label="検索のオプション" disabled />
      </form>
    </div>
  );
};

type TabRowProps = { pages: Page[]; active: Page; onSelect: (id: number) => void; onClose: () => void; onNew: () => void };

/** ページの上に並ぶタブ。閉じるボタンは、IE と同じく開いているタブにだけ出す。 */
const TabRow = ({ pages, active, onSelect, onClose, onNew }: TabRowProps) => (
  <div class="ie-tabs" role="tablist">
    {pages.map((p) => {
      const site = siteOf(urlOf(p));
      return (
        <div key={p.id} class={`ie-tab${p === active ? ' is-active' : ''}`}>
          <button class="ie-tab-label" role="tab" aria-selected={p === active} title={titleOf(p)} onClick={() => onSelect(p.id)}>
            {site && <img src={faviconOf(site)} alt="" width={16} height={16} />}
            <span>{titleOf(p)}</span>
          </button>
          {p === active && pages.length > 1 && <button class="ie-tab-close" title="タブを閉じる" aria-label="タブを閉じる" onClick={onClose} />}
        </div>
      );
    })}
    <button class="ie-newtab" title="新しいタブ" aria-label="新しいタブ" onClick={onNew} />
  </div>
);

type StatusBarProps = { zoom: number; zoomId: string; zoomItems: MenuItem[]; onZoom: (zoom: number) => void };

/** いちばん下の、ゾーンとズームを出す欄。左の空いた欄は、IE で読み込みの進み具合や鍵のアイコンを出す場所。 */
const StatusBar = ({ zoom, zoomId, zoomItems, onZoom }: StatusBarProps) => (
  <div class="ie-status">
    <span class="ie-pane is-text" />
    {Array.from({ length: 6 }, (_, i) => <span key={i} class="ie-pane" />)}
    <span class="ie-pane is-zone"><Icon name="globe" />インターネット</span>
    <span class="ie-pane is-zoom">
      <button class="ie-zoom" title="ズームのレベルを変更します" onClick={() => onZoom(cycled(zoom))}><Icon name="zoom" />{zoom}%</button>
      <XpMenu id={zoomId} class="ie-drop" title="ズームのレベルを選ぶ" label="" items={zoomItems} />
    </span>
  </div>
);

export const InternetExplorerFloat = ({ f }: { f: Float }) => {
  const [pages, setPages] = useState(() => [pageOf(f.tab.settings?.url || GOOGLE)]);
  const [active, setActive] = useState(() => pages[0]!.id);
  const page = pages.find((p) => p.id === active) ?? pages[0]!;
  const url = urlOf(page);
  const title = titleOf(page);
  const favs = useFavorites();
  // ダッシュボードを開き直したら、開いていたタブのページから始める。
  useEffect(() => {
    if (url !== f.tab.settings?.url) setFloatSettings(f, { ...f.tab.settings, url });
  }, [url]);
  const update = (id: number, fn: (p: Page) => Page) => setPages((ps) => ps.map((p) => (p.id === id ? fn(p) : p)));
  /** 開いているタブの iframe を作り直し、並びの今の位置のページを読み込ませる。題は読み込みを終えるまで分からない。 */
  const load = (fn: (p: Page) => Page) =>
    update(page.id, (p) => {
      const q = fn(p);
      return { ...q, src: urlOf(q), reloads: q.reloads + 1, title: undefined };
    });
  const go = (input: string) => {
    const next = URL.parse(input)?.href ?? input;
    load((p) => visited(p, next));
  };
  // 並びに無いページから戻るときは、並びの今の位置のページを開く。
  const jump = (at: number) => load((p) => ({ ...p, at, away: false }));
  const reload = () => load((p) => p);
  // 中継が最後に描いたページの seq。これが増えずに読み込みが起きたら、ほかのサイトの中で移ったとみなす。
  const seen = useRef(-1);
  const onLoad = async (id: number, frame: HTMLIFrameElement) => {
    // iframe はページを開かせるたびに作り直すので、作ってから最初の読み込みは、並びに入れ済みのページになる。
    const started = !frame.dataset.loaded;
    frame.dataset.loaded = 'true';
    const last = await api.lastFrame().catch(() => undefined);
    if (!last) return;
    const moved = last.seq !== seen.current;
    seen.current = last.seq;
    update(id, (p) => {
      const q = started ? p : moved && last.url ? visited(p, last.url) : { ...p, list: p.list.slice(0, p.at + 1), away: true };
      // 題は、中継が描いたページのものしか分からない。
      return { ...q, title: last.url === urlOf(q) ? last.title : undefined };
    });
  };
  const newTab = () => {
    const p = pageOf(GOOGLE);
    setPages((ps) => [...ps, p]);
    setActive(p.id);
  };
  // 開いているタブを閉じたら、右隣のタブを開く。右端なら左隣を開く。
  const closeTab = () => {
    const at = pages.indexOf(page);
    const rest = pages.filter((p) => p !== page);
    setPages((ps) => ps.filter((p) => p.id !== page.id));
    setActive(rest[Math.min(at, rest.length - 1)]!.id);
  };
  const addFavorite = () => {
    if (!favorites.some((fv) => fv.url === url)) setFavorites([...favorites, { url, title }]);
  };
  const removeFavorite = (fv: Favorite) => {
    if (confirm(`「${fv.title}」をお気に入りバーから削除しますか？`)) setFavorites(favorites.filter((x) => x.url !== fv.url));
  };
  const setZoom = (zoom: number) => update(page.id, (p) => ({ ...p, zoom }));
  const menuId = (name: string) => `ie-${f.tab.id}-${name}`;
  const newWindow = () => openInternetExplorer(url);
  const closeWindow = () => dock(f.key, f.scope, f.tab.id);
  const recent: MenuItem[] = page.list.map((u, i) => ({ label: u, checked: i === page.at && !page.away, onClick: () => jump(i) })).reverse();
  const zoomItems: MenuItem[] = ZOOMS.map((z) => ({ label: `${z}%`, checked: z === page.zoom, onClick: () => setZoom(z) }));
  const favoriteItems: MenuItem[] = [{ label: 'お気に入りバーに追加(B)', onClick: addFavorite }, ...(favs.length ? ['-' as const] : []), ...favs.map((fv) => ({ label: fv.title, onClick: () => go(fv.url) }))];
  const toolItems: MenuItem[] = [{ label: '別のタブで開く(B)', onClick: () => window.open(url, '_blank', 'noopener') }, '-', { label: 'インターネット オプション(O)' }];
  const helpItems: MenuItem[] = [{ label: 'バージョン情報(A)' }];
  // 働かせられない項目は、XP と同じく灰色にして押せなくする。中のページは別のオリジンなので、切り取りやコピーのようにページを操作する項目も働かせられない。
  const menus: { name: string; label: string; items: MenuItem[] }[] = [
    { name: 'file', label: 'ファイル(F)', items: [{ label: '新しいタブ(T)', onClick: newTab }, { label: '新しいウィンドウ(N)', onClick: newWindow }, '-', { label: '閉じる(C)', onClick: closeWindow }] },
    { name: 'edit', label: '編集(E)', items: [{ label: '切り取り(T)' }, { label: 'コピー(C)' }, { label: '貼り付け(P)' }] },
    { name: 'view', label: '表示(V)', items: [{ label: '最新の情報に更新(R)', onClick: reload }, '-', ...zoomItems] },
    { name: 'favorites', label: 'お気に入り(A)', items: favoriteItems },
    { name: 'tools', label: 'ツール(T)', items: toolItems },
    { name: 'help', label: 'ヘルプ(H)', items: helpItems },
  ];
  const close = { kind: 'close' as const, label: '閉じる', onClick: closeWindow };
  return (
    <FloatFrame f={f} title={`${title} - Windows ${IE_TITLE}`} icon={IE_ICON} controls={[close]}>
      <div class="ie">
        <NavBar page={page} recentId={menuId('recent')} recent={recent} onJump={jump} onGo={go} onReload={reload} />
        <nav class="ie-menubar">
          <span class="ie-grip" />
          {menus.map((m) => <XpMenu key={m.name} id={menuId(m.name)} class="xp-menu" label={m.label} items={m.items} />)}
        </nav>
        <div class="ie-favbar">
          <span class="ie-grip" />
          <XpMenu id={menuId('center')} class="ie-cmd" label={<><Icon name="star" />お気に入り</>} items={favoriteItems} />
          <span class="ie-grip" />
          <button class="ie-cmd" title="お気に入りバーに追加" aria-label="お気に入りバーに追加" onClick={addFavorite}><Icon name="star-add" /></button>
          {favs.map((fv) => {
            const site = siteOf(fv.url);
            return (
              <button
                key={fv.url}
                class="ie-cmd ie-link"
                title={`${fv.url}\n右クリックで削除します`}
                onClick={() => go(fv.url)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  removeFavorite(fv);
                }}
              >
                {site && <img src={faviconOf(site)} alt="" width={16} height={16} />}
                <span>{fv.title}</span>
              </button>
            );
          })}
          <span class="ie-grip is-command" />
          <button class="ie-cmd is-split" title="ホーム" aria-label="ホーム" onClick={() => go(GOOGLE)}><Icon name="home" /></button>
          <button class="ie-cmd is-split" title="フィード" aria-label="フィード" disabled><Icon name="feed" /></button>
          <a class="ie-cmd" title="ページをメールで送信" aria-label="ページをメールで送信" href={`mailto:?subject=${encodeURIComponent(title)}&body=${encodeURIComponent(url)}`}><Icon name="mail" /></a>
          <button class="ie-cmd is-split" title="印刷" aria-label="印刷" disabled><Icon name="print" /></button>
          <XpMenu id={menuId('page')} class="ie-cmd is-split" label="ページ(P)" items={[{ label: '新しいウィンドウ(N)', onClick: newWindow }, '-', ...zoomItems]} />
          <XpMenu id={menuId('safety')} class="ie-cmd is-split" label="セーフティ(S)" items={[{ label: '閲覧の履歴の削除(D)' }, { label: 'InPrivate ブラウズ(I)' }]} />
          <XpMenu id={menuId('cmd-tools')} class="ie-cmd is-split" label="ツール(O)" items={toolItems} />
          <XpMenu id={menuId('cmd-help')} class="ie-cmd is-split" title="ヘルプ" label={<Icon name="help" />} items={helpItems} />
        </div>
        <TabRow pages={pages} active={page} onSelect={setActive} onClose={closeTab} onNew={newTab} />
        <div class="ie-view">
          {pages.map((p) => (
            // sandbox で、開いたサイトがダッシュボードの画面ごと移動させるのを止める。
            <iframe
              key={`${p.id} ${p.reloads}`}
              class="ie-page"
              hidden={p !== page}
              style={`--zoom: ${p.zoom / 100}`}
              title={titleOf(p)}
              src={frameOf(p.src)}
              onLoad={(e) => onLoad(p.id, e.currentTarget)}
              sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox"
            />
          ))}
        </div>
        <StatusBar zoom={page.zoom} zoomId={menuId('zoom')} zoomItems={zoomItems} onZoom={setZoom} />
      </div>
    </FloatFrame>
  );
};
