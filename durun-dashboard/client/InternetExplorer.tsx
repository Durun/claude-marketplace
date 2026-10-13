// Web のサイトをウィンドウの中の iframe で開く「Internet Explorer」。メモ帳と同じく、スキンによらず Windows XP の見た目で描く。
// 編集中に設定で URL を決めると、そのサイトのショートカットになり、サイトの favicon に矢印を重ねて描く。
import { useRef, useState } from 'preact/hooks';
import { GOOGLE, frameOf } from '../shared/retro.ts';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import type { WidgetProps } from './Dashboard.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';

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
        <DesktopIcon icon={`/api/favicon?${new URLSearchParams({ site: site.origin })}`} title={site.hostname} hint={`${site.href} を開きます。ダブルクリックで開きます`} shortcut onLaunch={() => openInternetExplorer(site.href)} />
      ) : (
        <DesktopIcon icon={IE_ICON} title={IE_TITLE} hint="Web のサイトを見ます。ダブルクリックで開きます" onLaunch={() => openInternetExplorer()} />
      )}
      <button class="btn btn-sm desktop-icon-settings" data-icon="settings" onClick={configure}>設定</button>
    </>
  );
};

export const InternetExplorerFloat = ({ f }: { f: Float }) => {
  const url = f.tab.settings?.url || GOOGLE;
  const [address, setAddress] = useState(url);
  // iframe に読み込ませる URL。ページの中のリンクで移ったときは url だけが変わり、iframe は読み込み直さない。
  const [src, setSrc] = useState(url);
  // 同じ URL を開き直すときも iframe を作り直して読み込ませる。
  const [reloads, setReloads] = useState(0);
  // 戻る・進むのための、このウィンドウで開いたページの並び。ダッシュボードを開き直すと消える。
  // away は、ほかのサイトの中で移って、今のページが並びに無いこと。
  const [history, setHistory] = useState({ list: [url], at: 0, away: false });
  const show = (next: string) => {
    setAddress(next);
    setFloatSettings(f, { ...f.tab.settings, url: next });
  };
  const visit = (next: string) => {
    setHistory((h) => (h.list[h.at] === next ? { ...h, away: false } : { list: [...h.list.slice(0, h.at + 1), next], at: h.at + 1, away: false }));
    show(next);
  };
  // load で開かせたページの読み込みか。並びにはもう入っているので、onLoad で数えない。
  const loading = useRef(true);
  const load = (next: string) => {
    loading.current = true;
    setSrc(next);
    setReloads((n) => n + 1);
    show(next);
  };
  const go = (input: string) => {
    const next = URL.parse(input)?.href ?? input;
    visit(next);
    load(next);
  };
  // 並びに無いページから戻るときは、並びの今の位置のページを開く。
  const step = (by: number) => {
    const at = history.away && by < 0 ? history.at : history.at + by;
    setHistory({ ...history, at, away: false });
    load(history.list[at] ?? GOOGLE);
  };
  // 中継が最後に描いたページの seq。これが増えずに読み込みが起きたら、ほかのサイトの中で移ったとみなす。
  const seen = useRef(-1);
  const onLoad = async () => {
    const loaded = loading.current;
    loading.current = false;
    const last = await api.lastFrame().catch(() => undefined);
    if (!last) return;
    const moved = last.seq !== seen.current;
    seen.current = last.seq;
    if (loaded) return;
    // 進む先は、ブラウザと同じく捨てる。
    if (moved && last.url) visit(last.url);
    else setHistory((h) => ({ list: h.list.slice(0, h.at + 1), at: h.at, away: true }));
  };
  const submit = (e: Event) => {
    e.preventDefault();
    if (address.trim()) go(addressOf(address));
  };
  const title = siteOf(url)?.hostname ?? url;
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={`${title} - ${IE_TITLE}`} icon={IE_ICON} controls={[close]}>
      <div class="ie">
        <form class="ie-bar" onSubmit={submit}>
          <button type="button" class="ie-btn" disabled={history.at === 0 && !history.away} onClick={() => step(-1)}>戻る</button>
          <button type="button" class="ie-btn" disabled={history.at === history.list.length - 1} onClick={() => step(1)}>進む</button>
          <button type="button" class="ie-btn" onClick={() => go(GOOGLE)}>ホーム</button>
          <button type="button" class="ie-btn" onClick={() => load(url)}>更新</button>
          <label class="ie-address">
            アドレス(D)
            <input value={address} spellcheck={false} onInput={(e) => setAddress(e.currentTarget.value)} onFocus={(e) => e.currentTarget.select()} />
          </label>
          <button class="ie-btn">移動</button>
          {/* 埋め込みを断るサイトは iframe に出せないので、ブラウザの別のタブで開けるようにする。 */}
          <button type="button" class="ie-btn" onClick={() => window.open(url, '_blank', 'noopener')}>別のタブで開く</button>
        </form>
        {/* sandbox で、開いたサイトがダッシュボードの画面ごと移動させるのを止める。 */}
        <iframe key={reloads} class="ie-page" title={title} src={frameOf(src)} onLoad={onLoad} sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox" />
      </div>
    </FloatFrame>
  );
};
