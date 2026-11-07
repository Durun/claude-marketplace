//! 操作と描画を外に出す HTTP サーバー。アプリが書き手として開いたプロジェクトに、CLI と Claude が HTTP で操作を送る。
//!
//! - [`Core`]: 書き手の `Store` を `Mutex` で直列に扱い、操作を適用して書き、変更を購読者に送る。
//!   アプリの IPC も同じ `Core` を通る。
//! - [`Server`]: `Core` を HTTP（axum）で公開する。127.0.0.1 だけで待ち、`Host`・`Origin`・
//!   起動ごとのトークン（プロジェクトの外の持ち主専用のフォルダー）を確かめる。
//! - 提案: Claude の編集を適用せずに保留し、人が承認・却下する（[`Core::propose`]、[`Core::approve`]、`/proposals`）。
//! - [`Client`]: `mm` と結合テストが使うクライアント。
//! - [`FrameSource`]: `GET /frame` の描画。[`EngineFrames`] が専用のスレッドの `Engine` で描く。

mod client;
mod core;
mod frames;
mod http;
mod proposals_http;
mod review;
mod server;
mod transcripts;

pub use client::{Client, EventStream, Response, SseEvent};
pub use core::{now_ms, Caller, Change, ChangeOf, Core, ProjectView, ServeError};
pub use frames::{encode_png, parse_time, render_png, EngineFrames, FrameRequest, FrameSource};
pub use review::{Approval, ProposalEvent, ProposalOpView, ProposalView};
pub use server::Server;
