//! Windows 専用の本体。新しいパネルの処理は、名前の順の位置に mod を 1 行足し、`app.rs` の `route` に前置きを 1 行足す。

mod app;
mod capture;
mod cli;
mod clock;
mod deliver;
mod deliver_ipc;
mod edit;
mod edit_actions;
mod history;
mod ipc;
mod keys;
mod media;
mod media_ipc;
mod mixer;
mod mixer_ipc;
mod pack_ipc;
mod playback;
mod project;
mod render;
mod review;
mod review_ipc;
mod sequence_edit;
mod source;
mod source_ipc;
mod state;
mod story;
mod story_ipc;
mod surface;
mod theme;
mod timebase;
mod transition;
mod transition_ipc;
mod verify;
mod verify_edit;

pub use app::run;
