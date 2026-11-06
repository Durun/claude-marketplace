//! 操作と描画を外に出す入口。アプリが書き手として開いたプロジェクトに、CLI と Claude が HTTP で操作を送る。
//!
//! - [`Core`]: 書き手の `Store` を 1 本の列で持ち、操作を適用して書き、変更を購読者に送る。
//!   アプリの IPC も同じ `Core` を通る。
//! - [`Server`]: `Core` を HTTP（axum）で公開する。127.0.0.1 だけで待ち、`Host`・`Origin`・
//!   起動ごとの合い言葉（プロジェクトの外の持ち主専用のフォルダー）を確かめる。
//! - [`Client`]: `mm` と結合テストが使う呼び手。
//! - [`FrameSource`]: `GET /frame` の描画。[`EngineFrames`] が専用のスレッドの `Engine` で描く。

mod client;
mod core;
mod frames;
mod http;
mod server;
mod transcripts;

pub use client::{Client, EventStream, Response, SseEvent};
pub use core::{now_ms, Caller, Change, ChangeOf, Core, ProjectView, ServeError};
pub use frames::{encode_png, parse_time, render_png, EngineFrames, FrameRequest, FrameSource};
pub use server::Server;
