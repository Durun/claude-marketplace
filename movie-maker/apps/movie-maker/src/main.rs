//! movie-maker のアプリ。Tauri のウィンドウに wgpu のサーフェス（子 HWND。Program と Timeline）と WebView2 を並べ、
//! プロジェクトを開いて入れ子のコンポジションを再生する。ウィンドウの組み立ての経緯は docs/design/spikes/window.md。
//!
//! `movie-maker [PROJECT] [--composition ID] [--no-audio] [--play-test SECS --report OUT.json [--screenshot OUT.png]]`

#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("このアプリは Windows 専用（WebView2 と Win32 の子 HWND を使う）");
}
