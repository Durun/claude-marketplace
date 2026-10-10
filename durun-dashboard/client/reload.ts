import { useEffect, useRef, useState } from 'preact/hooks';
import type { Snapshot } from '../shared/types.ts';

// 書きかけの文章、開いている要望のフォーム、イルカとの会話は、読み込み直すと消える。
const isEditing = (): boolean =>
  !!document.querySelector('.fb-panel, .dolphin-chat') ||
  [...document.querySelectorAll('textarea')].some((t) => t.value.trim() !== '') ||
  !!document.activeElement?.matches('input:not([type="checkbox"]), textarea, select');

/**
 * 画面のファイルが作り直されたら反映する。CSS は差し替え、JS は読み込み直す。
 * 入力の途中で読み込み直せないときは true を返し、人間が押して読み込み直せるようにする。
 */
export const useLiveReload = (assets: Snapshot['assets'] | undefined): boolean => {
  const loaded = useRef(assets);
  const [stale, setStale] = useState(false);
  useEffect(() => {
    if (!assets) return;
    loaded.current ??= assets;
    if (assets.css !== loaded.current.css) {
      document.querySelector('link[rel="stylesheet"]')?.setAttribute('href', `/style.css?v=${assets.css}`);
      loaded.current = { ...loaded.current, css: assets.css };
    }
    if (assets.js === loaded.current.js) return;
    if (isEditing()) setStale(true);
    else location.reload();
  }, [assets?.js, assets?.css]);
  return stale;
};
