//! `web.cef` のノードの種類。HTML と CSS と JS のページを、別プロセスの CEF（`mm-host-api` のホスト）で描き、
//! 共有テクスチャを wgpu のレイヤーにする。
//!
//! - 時刻: ページの `window.mmSetTime(t, data)` に、レイヤーの始まりからの秒 `t` とパラメータ `data` を渡す。
//!   ホストは JS の反映を待ってからフレームを描くので、同じ入力なら同じ画素になる（プレビューと書き出しで一致）。
//! - 倍率: ページは 1 倍の画素で組む。プレビューの縮小は CSS の `zoom` で掛ける。
//! - キャッシュ: ページ、`data`、時刻、大きさ、倍率の組で描いたフレームをキャッシュする。`static` のページは時刻を 0 に揃え、
//!   パラメータの組ごとに 1 回だけ描く（動きは合成の側の不透明度と変形で付ける）。
//! - ホスト: ページごとに読み込んだホストを置いておき、クラッシュしたら 1 回だけ起こし直す。
//! - CEF が無い環境: [`WebCefKind::mock`] がページとパラメータで決まる色の矩形を描く。
//!
//! ホストの実行ファイルとランチャーは組み立て（`mm-runtime`）が選んで渡す（実装は実装に依存しない）。

mod cache;
mod gpu;
mod hosts;
mod kind;
mod mock;
mod page;
#[cfg(windows)]
mod shared;

pub use kind::{Stats, WebCefKind};
pub use page::{inject, DATA_PARAM, HTML_PARAM, STATIC_PARAM};

/// 種類の名前。
pub const KIND_ID: &str = "web.cef";
/// 種類のバージョン。パラメータの形を変えたら上げる。
pub const KIND_VERSION: u32 = 1;
