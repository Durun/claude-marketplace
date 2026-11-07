//! OpenFX（OFX）のプラグインを別プロセスで読み込み、1 フレームを描いて返すホスト（`mm-host-api` の実装）。
//!
//! - アプリ側: [`OfxLauncher`] が実行ファイル `mm-host-ofx serve` を起こし、パイプで 1 行ずつ JSON をやり取りする。
//!   プラグインの一覧とパラメータは [`describe_bundle`] が `mm-host-ofx describe <バンドル>` を別プロセスで走らせて受け取る。
//! - ホスト側（`src/host/`、実行ファイルだけ）: OpenFX の C API のホストを Rust で持ち、プラグインの DLL を読み込む。
//!   プラグインのクラッシュはホストのプロセスだけを落とし、アプリには `HostCrashed` で返る。
//!
//! 要求の `params` の形は [`PrepareParams`] と [`RenderParams`]。手順と先行事例は README。

pub mod desc;
mod launcher;
pub mod scan;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use mm_host_api::SharedFrame;
use mm_types::Params;

pub use launcher::{
    describe_bundle, OfxLauncher, PipeHost, DESCRIBE_COMMAND, SERVE_COMMAND, SPEC_ENV,
};

/// 時刻（秒）を OpenFX のフレームの番号に直すときの既定のフレームレート。
pub const DEFAULT_FRAME_RATE: f64 = 30.0;

/// `HostRequest::Prepare` の `params`。プラグインを読み込み、インスタンスを作る。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepareParams {
    /// `<名前>.ofx.bundle` のフォルダー。
    pub bundle: PathBuf,
    /// 使うプラグインの `id`。`None` ならバンドルの最初のプラグイン。
    #[serde(default)]
    pub plugin: Option<String>,
    /// `RenderFrame` の時刻をフレームの番号に直すフレームレート。
    #[serde(default = "default_frame_rate")]
    pub frame_rate: f64,
}

fn default_frame_rate() -> f64 {
    DEFAULT_FRAME_RATE
}

/// `HostRequest::RenderFrame` の `params`。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RenderParams {
    /// Filter の文脈の入力（`Source` のクリップ）。`None` なら透明。大きさは `RenderFrame` の `size` に合わせる。
    #[serde(default)]
    pub source: Option<SharedFrame>,
    /// 時刻で評価済みのパラメータの値。無い名前は既定値のまま。
    #[serde(default)]
    pub values: Params,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 要求の_params_は省略できる() {
        let p: PrepareParams =
            serde_json::from_value(serde_json::json!({"bundle": "a.ofx.bundle"}))
                .expect("bundle だけで読める");
        assert_eq!(p.plugin, None);
        assert_eq!(p.frame_rate, DEFAULT_FRAME_RATE);
        let r: RenderParams = serde_json::from_value(serde_json::json!({})).expect("空で読める");
        assert_eq!(r, RenderParams::default());
    }
}
