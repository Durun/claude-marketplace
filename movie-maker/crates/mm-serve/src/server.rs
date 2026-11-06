//! HTTP の入口を起こす。127.0.0.1 の空いているポートで待ち、ポートと合い言葉を
//! プロジェクトの `.mm/` に書いて、`mm` が見つけられるようにする。

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;

use tokio::runtime::{Builder, Runtime};

use crate::core::{Core, ServeError};
use crate::http::{router, AppState};

/// 待ち受けの非同期の実行環境のスレッド数。重い仕事は `spawn_blocking` に出すので少なくてよい。
const WORKER_THREADS: usize = 2;
/// 合い言葉の長さ（バイト）。16 進で 2 倍の文字数になる。
const TOKEN_BYTES: usize = 32;

/// 動いている入口。落とすと待ち受けを止め、`.mm/` のポートと合い言葉を消す。
pub struct Server {
    port: u16,
    token: String,
    core: Arc<Core>,
    runtime: Option<Runtime>,
}

impl Server {
    /// 127.0.0.1 だけで待ち始め、ポートと合い言葉を公開する。
    pub fn start(core: Arc<Core>) -> Result<Self, ServeError> {
        let token = new_token()?;
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|e| ServeError::Internal(format!("待ち受けを始められない: {e}")))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| ServeError::Internal(e.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|e| ServeError::Internal(e.to_string()))?
            .port();
        let runtime = Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .thread_name("mm-serve")
            .enable_all()
            .build()
            .map_err(|e| ServeError::Internal(format!("実行環境を作れない: {e}")))?;
        let state = Arc::new(AppState {
            core: core.clone(),
            token: token.clone(),
            hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        });
        let listener = {
            let _enter = runtime.enter();
            tokio::net::TcpListener::from_std(listener)
                .map_err(|e| ServeError::Internal(e.to_string()))?
        };
        runtime.spawn(async move {
            if let Err(e) = axum::serve(listener, router(state)).await {
                tracing::error!("HTTP の待ち受けが止まった: {e}");
            }
        });
        // 待ち受けが整ってから公開する。見つけた呼び手がすぐ接続できる。
        core.publish(port, &token)?;
        Ok(Self {
            port,
            token,
            core,
            runtime: Some(runtime),
        })
    }

    /// 待っているポート。
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 起動ごとの合い言葉。
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.core.unpublish();
        if let Some(rt) = self.runtime.take() {
            // 開きっぱなしの SSE を待たずに止める。
            rt.shutdown_background();
        }
    }
}

fn new_token() -> Result<String, ServeError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|e| ServeError::Internal(format!("乱数を取れない: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
