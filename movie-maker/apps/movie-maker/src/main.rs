//! Tauri のウィンドウに wgpu のサーフェス（子 HWND）と WebView2（子 WebView）を並べる試作。
//! 結果は docs/design/spikes/window.md。`--selftest <out.json>` で自己診断を回す。

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod gpu;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod selftest;
#[cfg(windows)]
mod shared;
#[cfg(windows)]
mod surface;
#[cfg(windows)]
mod sync;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("この試作は Windows 専用（WebView2 と Win32 の子 HWND を使う）");
}
