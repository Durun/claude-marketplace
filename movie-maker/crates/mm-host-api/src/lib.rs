//! 別プロセスのホスト（CEF、OFX、VST3/CLAP）のプロトコル。
//! アプリ側（`mm-nodes-cef` など）が頼み、実行ファイル（`mm-host-cef` など）が描く。
//!
//! 制御はパイプで [`HostRequest`]/[`HostResponse`] の JSON を 1 行ずつ。絵は共有テクスチャの
//! ハンドル（[`SharedFrame::SharedTexture`]）で渡し、ハンドルを開く所は実装のクレートに閉じる。
//!
//! 契約（`mm_testkit::host::contract`）:
//! - `spawn` 直後に `Ping` → `Pong`。
//! - `RenderFrame` → `Frame` で、頼んだ大きさの絵が返る。
//! - 期限を過ぎた要求は `Err(Timeout)` で戻り、ホストはそのまま使える。
//! - `kill` の後は `is_alive() == false`、`restart` の後は再び `Ping` が通る。
//! - 共有テクスチャのハンドルは応答ごとに閉じ、漏れない（実装が `open_handles` で数える）。

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use mm_types::{RationalTime, Result, RgbaImage, Size};

/// プロトコルの版。要求と応答の形を変えたら上げる。
pub const PROTOCOL_VERSION: u32 = 1;

/// ホストの起動の指定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSpec {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    /// 1 要求の既定の期限。
    pub default_timeout: Duration,
}

/// アプリからホストへ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    /// 生きているか。
    Ping,
    /// 種類の準備（HTML の読み込み、プラグインの読み込み）。
    Prepare { params: serde_json::Value },
    /// 時刻 `time` のコマを `size` で描く。
    RenderFrame {
        time: RationalTime,
        size: Size,
        params: serde_json::Value,
    },
    /// 終わる。
    Shutdown,
}

/// ホストからアプリへ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostResponse {
    Pong { protocol_version: u32 },
    Prepared,
    Frame(SharedFrame),
    Error { message: String },
}

/// ホストが描いたコマ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SharedFrame {
    /// CPU の絵（パイプで渡す逃げ道。遅い）。
    Cpu {
        size: Size,
        /// RGBA f32、premultiplied、線形。
        data: Vec<f32>,
    },
    /// D3D12 の共有テクスチャの NT ハンドル。受け取った側がコピーして閉じる。
    SharedTexture { handle: u64, size: Size },
}

impl SharedFrame {
    /// CPU の絵から。
    pub fn from_image(image: &RgbaImage) -> Self {
        Self::Cpu {
            size: image.size(),
            data: image.data().to_vec(),
        }
    }

    /// 大きさ。
    pub fn size(&self) -> Size {
        match self {
            Self::Cpu { size, .. } | Self::SharedTexture { size, .. } => *size,
        }
    }
}

/// 起動したホスト 1 つ。
pub trait HostProcess: Send {
    /// 生きているか。
    fn is_alive(&self) -> bool;
    /// プロセス ID。トレースの `pid` に付ける。
    fn pid(&self) -> Option<u32>;
    /// 要求を送り、応答を待つ。`timeout` を過ぎたら `Err(Timeout)`。
    fn request(&mut self, req: &HostRequest, timeout: Duration) -> Result<HostResponse>;
    /// 落とす。
    fn kill(&mut self) -> Result<()>;
    /// 起動し直す。`Prepare` は呼ぶ側がやり直す。
    fn restart(&mut self) -> Result<()>;
    /// 今開いている共有テクスチャのハンドルの数。漏れの検査に使う。
    fn open_handles(&self) -> usize;
}

/// ホストを起動する口。
pub trait HostLauncher: Send + Sync {
    /// 実装の名前（`cef`、`ofx`、`audio`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// 起動する。
    fn spawn(&self, spec: &HostSpec) -> Result<Box<dyn HostProcess>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 要求は_json_を往復する() {
        let req = HostRequest::RenderFrame {
            time: RationalTime::seconds(1),
            size: Size::new(16, 9),
            params: serde_json::json!({"html": "<b>x</b>"}),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: HostRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
        assert!(s.contains("\"type\":\"render_frame\""));
    }
}
