// Web のサイトをウィンドウの中の iframe で開く「Internet Explorer」。メモ帳と同じく、スキンによらず Windows XP の見た目で描く。
// 編集中に設定で URL を決めると、そのサイトのショートカットになり、サイトの favicon に矢印を重ねて描く。
import { useState } from 'preact/hooks';
import { GOOGLE, frameOf } from '../shared/retro.ts';
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
  // 同じ URL を開き直すときも iframe を作り直して読み込ませる。
  const [reloads, setReloads] = useState(0);
  const go = (next: string) => {
    setAddress(next);
    setReloads((n) => n + 1);
    setFloatSettings(f, { ...f.tab.settings, url: next });
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
          <button type="button" class="ie-btn" onClick={() => go(GOOGLE)}>ホーム</button>
          <button type="button" class="ie-btn" onClick={() => setReloads((n) => n + 1)}>更新</button>
          <label class="ie-address">
            アドレス(D)
            <input value={address} spellcheck={false} onInput={(e) => setAddress(e.currentTarget.value)} onFocus={(e) => e.currentTarget.select()} />
          </label>
          <button class="ie-btn">移動</button>
          {/* 埋め込みを断るサイトは iframe に出せないので、ブラウザの別のタブで開けるようにする。 */}
          <button type="button" class="ie-btn" onClick={() => window.open(url, '_blank', 'noopener')}>別のタブで開く</button>
        </form>
        {/* sandbox で、開いたサイトがダッシュボードの画面ごと移動させるのを止める。 */}
        <iframe key={reloads} class="ie-page" title={title} src={frameOf(url)} sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox" />
      </div>
    </FloatFrame>
  );
};
