// メニューの「壁紙」で開くウィンドウ。XP スキンの壁紙を、画像のファイルか画像の URL で渡す。
import { useState } from 'preact/hooks';
import { api, attempt } from './api.ts';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { toast } from './toast.tsx';

/** 壁紙のウィンドウの保存キー。ダッシュボードのウィジェットではないので、scope は空にし、タブの id にもこのキーを入れる。 */
export const WALLPAPER_KEY = 'wallpaper';
// 今の壁紙の見本と入力欄がちょうど収まる大きさ（px）。
const SIZE = { w: 460, h: 480 };
// サーバーが受ける画像の種類。server/wallpaper.ts の WALLPAPER_TYPES と揃える。
const ACCEPT = 'image/jpeg,image/png,image/webp,image/gif';

export const openWallpaper = (): void => floatUp(WALLPAPER_KEY, '', { id: WALLPAPER_KEY }, SIZE);
const close = () => dock(WALLPAPER_KEY, '', WALLPAPER_KEY);

const Wallpaper = () => {
  const [src, setSrc] = useState('/wallpaper');
  // 壁紙を渡していないか、渡した URL が画像として読めない。
  const [missing, setMissing] = useState(false);
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState(false);
  // 壁紙の URL は変わらないので、クエリを変えて、敷いている壁紙と見本を読み直させる。
  const change = async (label: string, done: string, fn: () => Promise<unknown>) => {
    setBusy(true);
    const ok = (await attempt(label, async () => (await fn(), true))) ?? false;
    setBusy(false);
    if (!ok) return;
    const next = `/wallpaper?v=${Date.now()}`;
    document.documentElement.style.setProperty('--wallpaper', `url("${next}")`);
    setSrc(next);
    setMissing(false);
    toast(done);
  };
  const pickFile = (e: Event) => {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    // 同じファイルを選び直しても送れるよう、選んだことを忘れさせる。
    input.value = '';
    if (file) void change('壁紙の変更', '壁紙を変えました', () => api.wallpaperFile(file));
  };
  const submitUrl = (e: Event) => {
    e.preventDefault();
    if (!url.trim() || busy) return;
    void change('壁紙の変更', '壁紙を変えました', () => api.wallpaperUrl(url.trim())).then(() => setUrl(''));
  };
  return (
    <div class="stack wallpaper">
      {missing ? <p class="empty wallpaper-preview">壁紙はありません。スキンの初期の絵を敷いています。</p> : <img class="wallpaper-preview" src={src} alt="今の壁紙" onError={() => setMissing(true)} />}
      <label class="inline-field">
        <span class="nowrap">ファイル</span>
        <input class="input" type="file" accept={ACCEPT} disabled={busy} onChange={pickFile} />
      </label>
      <form class="inline-field" onSubmit={submitUrl}>
        <label class="nowrap" for="wallpaper-url">画像 URL</label>
        <input id="wallpaper-url" class="input mono" type="url" placeholder="https://example.com/wallpaper.jpg" value={url} onInput={(e) => setUrl(e.currentTarget.value)} />
        <button class="btn" disabled={busy || !url.trim()}>設定</button>
      </form>
      <div class="actions">
        <span class="help">壁紙は Windows XP のスキンで敷きます。</span>
        <span class="spacer" />
        <button class="btn" disabled={busy} onClick={() => change('壁紙を戻す操作', '初期の壁紙に戻しました', api.resetWallpaper)}>初期の壁紙に戻す</button>
      </div>
    </div>
  );
};

export const WallpaperFloat = ({ f }: { f: Float }) => (
  <FloatFrame f={f} title="壁紙" icon="🖼️" controls={[{ kind: 'close', label: '閉じる', onClick: close }]}>
    <Wallpaper />
  </FloatFrame>
);
