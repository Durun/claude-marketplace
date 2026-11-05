// アプリ「翻訳」。PLaMo 3 Translate 0.8B を手元の PC で動かして訳す。入れていなければ、導入ボタンを出す。
// 原文と訳文はウィンドウの設定に持ち、画面を読み込み直しても残す。
import { useEffect, useState } from 'preact/hooks';
import type { TranslateStatus } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp, latestFloat, setFloatSettings } from './Float.tsx';
import { useFetch, useWaitCursor } from './hooks.ts';
import { toast } from './toast.tsx';
import { Empty } from './ui.tsx';

export const TRANSLATE_KEY = 'translate';
export const TRANSLATE_TITLE = '翻訳';
export const TRANSLATE_ICON = '/translate.svg';

export const openTranslate = (): void => floatUp(TRANSLATE_KEY, '', { id: TRANSLATE_KEY }, { w: 720, h: 420 });

export const TranslateIcon = () => <DesktopIcon icon={TRANSLATE_ICON} title={TRANSLATE_TITLE} hint="PLaMo 3 Translate で、PC の中だけで翻訳します。ダブルクリックで開きます" onLaunch={openTranslate} />;

// 自動は、日本語の字を含めば英語へ、含まなければ日本語へ訳す。再翻訳は、英語に訳した文を日本語に訳し直す。
export const AUTO = 'auto';
const ROUND_TRIP = 'round-trip';
const TARGETS: [string, string][] = [
  [AUTO, '自動（日本語 ⇄ 英語）'],
  [ROUND_TRIP, '英語を経て日本語に再翻訳'],
  ['ja', '日本語'],
  ['en', '英語'],
  ['zh-hans', '中国語（簡体字）'],
  ['zh-hant', '中国語（繁体字）'],
  ['ko', '韓国語'],
];

const JAPANESE = /[぀-ヿ㐀-鿿]/;
export const targetOf = (choice: string, text: string): string => (choice !== AUTO ? choice : JAPANESE.test(text) ? 'en' : 'ja');

/** choice は訳す先の選択肢の値。窓と、右クリックの「翻訳」が同じ訳し方をする。 */
export const translateIn = async (choice: string, text: string): Promise<string> =>
  (choice === ROUND_TRIP ? await api.translate((await api.translate(text, 'en')).text, 'ja') : await api.translate(text, targetOf(choice, text))).text;

const Install = ({ status, onDone }: { status: TranslateStatus; onDone: () => void }) => {
  const [busy, setBusy] = useState(false);
  const install = async () => {
    setBusy(true);
    const r = await attempt('翻訳のモデルの導入', api.translateInstall);
    setBusy(false);
    if (r) toast(r.message);
    onDone();
  };
  // 読み込み直す前から入れていたら、その導入の終わりを待つ。
  useEffect(() => {
    if (status.installing) void install();
  }, []);
  const missing = [!status.runtime && `llama.cpp（${status.install}）`, !status.model && 'PLaMo 3 Translate 0.8B のモデル（0.57 GB）'].filter(Boolean);
  return (
    <div class="translate-install">
      <p>翻訳に使う次のものが入っていません。</p>
      <ul>{missing.map((m) => <li key={m}>{m}</li>)}</ul>
      <p class="translate-license">モデルは PLaMo Community License で配布されています。商用で使うときは登録が要ります。</p>
      <button type="button" class="btn" disabled={busy} onClick={() => void install()}>{busy ? '入れています…' : 'インストール'}</button>
    </div>
  );
};

export const TranslateFloat = ({ f }: { f: Float }) => {
  const status = useFetch(() => api.translateStatus(), []);
  const s = f.tab.settings ?? {};
  const source = s.source ?? '';
  const choice = s.target ?? AUTO;
  const [busy, setBusy] = useState(false);
  useWaitCursor(busy);
  const update = (patch: Record<string, string>) => {
    const cur = latestFloat(f) ?? f;
    setFloatSettings(cur, { ...cur.tab.settings, ...patch });
  };
  const run = async () => {
    if (busy || !source.trim()) return;
    setBusy(true);
    const r = await attempt('翻訳', () => translateIn(choice, source));
    setBusy(false);
    if (r !== undefined) update({ result: r });
  };
  const ready = status.data?.runtime && status.data.model;
  return (
    <FloatFrame f={f} title={TRANSLATE_TITLE} icon={TRANSLATE_ICON} tray controls={[{ kind: 'close', label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) }]}>
      <div class="widget-body translate">
        {!status.data ? (
          <Empty>{status.error ?? '確かめています…'}</Empty>
        ) : !ready ? (
          <Install status={status.data} onDone={status.reload} />
        ) : (
          <>
            <div class="translate-bar">
              <select class="input input-sm" value={choice} aria-label="訳す先の言語" onChange={(e) => update({ target: e.currentTarget.value })}>
                {TARGETS.map(([v, label]) => <option key={v} value={v}>{label}</option>)}
              </select>
              <button type="button" class="btn btn-sm" disabled={busy || !source.trim()} onClick={() => void run()}>{busy ? '訳しています…' : '翻訳'}</button>
              <span class="translate-hint">⌘/Ctrl + Enter で翻訳</span>
            </div>
            <div class="translate-panes">
              <textarea
                class="input"
                value={source}
                placeholder="訳す文章"
                aria-label="原文"
                onInput={(e) => update({ source: e.currentTarget.value })}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    void run();
                  }
                }}
              />
              <textarea class="input" value={s.result ?? ''} readOnly placeholder="訳文" aria-label="訳文" />
            </div>
          </>
        )}
      </div>
    </FloatFrame>
  );
};
