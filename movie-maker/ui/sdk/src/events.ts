/** パネルが素材を選んだときに投げる DOM のイベント名。アプリの `web/transport.js` が受け、Source で開く。 */
export const ASSET_SELECTED_EVENT = "mm:asset-selected";

/** 素材を選んだことを知らせる（`detail.asset` が素材の ID）。 */
export const announceAsset = (asset: string): void => {
  window.dispatchEvent(new CustomEvent(ASSET_SELECTED_EVENT, { detail: { asset } }));
};
