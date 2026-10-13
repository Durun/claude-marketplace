// Web のサイトを開く「Internet Explorer」。サイトはブラウザの別のタブで開く。Bing など多くのサイトは X-Frame-Options でダッシュボードの中への埋め込みを断るため。
// 編集中に設定で URL を決めると、そのサイトのショートカットになり、サイトの favicon に矢印を重ねて描く。
import { DesktopIcon } from './apps.tsx';
import type { WidgetProps } from './Dashboard.tsx';

export const IE_KEY = 'ie';
export const IE_TITLE = 'Internet Explorer';
export const IE_ICON = '/ie.svg';
const HOME = 'https://www.bing.com/';

const open = (url: string): void => void window.open(url, '_blank', 'noopener');

export const openInternetExplorer = (): void => open(HOME);

/** 入力を http(s) の URL に限る。javascript: などを開かせない。 */
const siteOf = (input: string): URL | undefined => {
  const url = URL.parse(input.trim());
  return url?.protocol === 'https:' || url?.protocol === 'http:' ? url : undefined;
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
        <DesktopIcon icon={`/api/favicon?${new URLSearchParams({ site: site.origin })}`} title={site.hostname} hint={`${site.href} を開きます。ダブルクリックで開きます`} shortcut onLaunch={() => open(site.href)} />
      ) : (
        <DesktopIcon icon={IE_ICON} title={IE_TITLE} hint="Bing を開きます。ダブルクリックで開きます" onLaunch={openInternetExplorer} />
      )}
      <button class="btn btn-sm desktop-icon-settings" data-icon="settings" onClick={configure}>設定</button>
    </>
  );
};
