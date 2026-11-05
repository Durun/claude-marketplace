//! 別プロセスのホスト（CEF、OFX、VST3/CLAP）のプロトコル。
//! アプリ側（`mm-nodes-cef` など）が頼み、実行ファイル（`mm-host-cef` など）が描く。
//!
//! 制御はパイプで [`HostRequest`]/[`HostResponse`] の JSON を 1 行ずつ。絵は共有テクスチャの
//! ハンドル（[`SharedFrame::SharedTexture`]）で渡し、ハンドルを開く所は実装のクレートに閉じる。
//!
//! 共有テクスチャの同期（RFC 0001）: ホストは [`HostSpec::gpu`] のアダプターでテクスチャを作り、
//! [`HostSpec::client_pid`] のプロセスへ複製したハンドルを返す。`fence` が `None` ならホストが
//! 描画の完了を待ってから返しており、アプリはすぐ開ける。`Some` ならアプリが `submit` の前に
//! そのフェンスの値を待つ。
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

use mm_types::{GpuAdapterId, RationalTime, Result, RgbaImage, Size};

/// プロトコルの版。要求と応答の形を変えたら上げる。
/// - 2: `SharedFrame` のタグを `type` から `kind` に（`HostResponse::Frame` で重なっていた）。
pub const PROTOCOL_VERSION: u32 = 2;

/// ホストの起動の指定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSpec {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    /// 1 要求の既定の期限。
    pub default_timeout: Duration,
    /// 使う GPU。`None` なら高性能のアダプター。アプリと同じでないと共有テクスチャを開けない。
    #[serde(default)]
    pub gpu: Option<GpuAdapterId>,
    /// ハンドルを複製して渡す相手（アプリ）のプロセス ID。`None` なら CPU の絵で返す。
    #[serde(default)]
    pub client_pid: Option<u32>,
}

/// 共有テクスチャの描き終わりを知らせるフェンス。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SharedFence {
    /// D3D12 の共有フェンスの NT ハンドル（受け取る側のプロセスで有効）。
    pub handle: u64,
    /// この値になれば描き終わっている。
    pub value: u64,
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

/// ホストが描いたコマ。`HostResponse::Frame` の中身で外側のタグが `type` なので、こちらは
/// `kind` にする（同じ名前だと JSON のキーが重なって読めない）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SharedFrame {
    /// CPU の絵（パイプで渡す逃げ道。遅い）。
    Cpu {
        size: Size,
        /// RGBA f32、premultiplied、線形。
        data: Vec<f32>,
    },
    /// D3D12 の共有テクスチャの NT ハンドル（受け取る側のプロセスで有効）。受け取った側がコピーして閉じる。
    SharedTexture {
        handle: u64,
        size: Size,
        /// `None` ならホストが完了を待ってから返している。
        #[serde(default)]
        fence: Option<SharedFence>,
    },
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

    fn roundtrip(res: &HostResponse) -> String {
        let s = serde_json::to_string(res).unwrap();
        let back: HostResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(*res, back, "{s}");
        s
    }

    #[test]
    fn 応答は全部の形が_json_を往復する() {
        roundtrip(&HostResponse::Pong {
            protocol_version: PROTOCOL_VERSION,
        });
        roundtrip(&HostResponse::Prepared);
        roundtrip(&HostResponse::Error {
            message: "x".into(),
        });
        let cpu = SharedFrame::Cpu {
            size: Size::new(1, 1),
            data: vec![0.0, 0.5, 1.0, 1.0],
        };
        let s = roundtrip(&HostResponse::Frame(cpu));
        assert!(
            s.contains("\"type\":\"frame\"") && s.contains("\"kind\":\"cpu\""),
            "{s}"
        );
        for fence in [
            None,
            Some(SharedFence {
                handle: 7,
                value: 3,
            }),
        ] {
            roundtrip(&HostResponse::Frame(SharedFrame::SharedTexture {
                handle: 42,
                size: Size::new(16, 9),
                fence,
            }));
        }
    }

    #[test]
    fn 要求は全部の形が_json_を往復する() {
        for req in [
            HostRequest::Ping,
            HostRequest::Prepare {
                params: serde_json::json!({"html": "x"}),
            },
            HostRequest::Shutdown,
        ] {
            let s = serde_json::to_string(&req).unwrap();
            assert_eq!(serde_json::from_str::<HostRequest>(&s).unwrap(), req);
        }
    }
}
