//! movie-maker のアプリ。Tauri のウィンドウに wgpu のサーフェス（子 HWND。Program と Timeline）と WebView2 を並べ、
//! プロジェクトを開いて入れ子のコンポジションを再生する。ウィンドウの組み立ての経緯は docs/design/spikes/window.md。
//!
//! `movie-maker [PROJECT] [--composition ID] [--no-audio] [--play-test SECS --report OUT.json [--screenshot OUT.png]]`

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod cli;
#[cfg(windows)]
mod clock;
#[cfg(windows)]
mod deliver;
#[cfg(windows)]
mod deliver_ipc;
#[cfg(windows)]
mod edit;
#[cfg(windows)]
mod edit_actions;
#[cfg(windows)]
mod history;
#[cfg(windows)]
mod ipc;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod media;
#[cfg(windows)]
mod media_ipc;
#[cfg(windows)]
mod playback;
#[cfg(windows)]
mod project;
#[cfg(windows)]
mod render;
#[cfg(windows)]
mod sequence_edit;
#[cfg(windows)]
mod source;
#[cfg(windows)]
mod source_ipc;
#[cfg(windows)]
mod state;
#[cfg(windows)]
mod story;
#[cfg(windows)]
mod story_ipc;
#[cfg(windows)]
mod surface;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
mod timebase;
#[cfg(windows)]
mod verify;
#[cfg(windows)]
mod verify_edit;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("このアプリは Windows 専用（WebView2 と Win32 の子 HWND を使う）");
}
