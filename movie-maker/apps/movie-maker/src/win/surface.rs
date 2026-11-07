//! wgpu のサーフェスを張る子 HWND。Tauri のウィンドウの下に自前で作り、フォーカスとキーとポインターを自分で受ける。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use mm_ui_layout::{Mods, PointerEvent};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
    VK_CONTROL, VK_F4, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, IsWindow, LoadCursorW, PostMessageW, RegisterClassW,
    SetWindowPos, CS_HREDRAW, CS_VREDRAW, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_NOPARENTNOTIFY, WS_VISIBLE,
};

use crate::win::keys;
use crate::win::state::Shared;

/// 1 ノッチあたりのホイールの値。
const WHEEL_DELTA: f32 = 120.0;
/// Ctrl キーの修飾（WM_MOUSEWHEEL の wParam の下位語）。
const MK_CONTROL: usize = 0x0008;
/// マウスがウィンドウから出た通知（`TrackMouseEvent` で頼んだもの）。
const WM_MOUSELEAVE: u32 = 0x02A3;
/// WM_KEYDOWN の lParam で、直前もキーが押されていた（押し続けの繰り返し）ことを示すビット。
const KEY_REPEAT_BIT: isize = 1 << 30;
/// 描画が止まっているときに入力が溜まり続けない上限。
const INPUT_LIMIT: usize = 512;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static TRACKING: AtomicBool = AtomicBool::new(false);

/// 矩形（x、y、幅、高さ。物理ピクセル）。
pub type PixelRect = (i32, i32, i32, i32);

/// 親の HWND の下に子 HWND を作る。位置と大きさは物理ピクセル。
pub fn create(parent: isize, shared: Arc<Shared>, rect: PixelRect) -> windows::core::Result<isize> {
    // wndproc から共有状態に届く道はこの静的変数だけなので、作る前に置く。
    let _ = SHARED.set(shared);
    let (x, y, w, h) = rect;
    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("MmSurface"),
            ..Default::default()
        };
        // 2 回目以降の登録は失敗するが、同じクラスが既にあるだけなので続ける。
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_NOPARENTNOTIFY,
            w!("MmSurface"),
            w!("wgpu surface"),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            x,
            y,
            w,
            h,
            Some(HWND(parent as *mut _)),
            None,
            Some(hinstance.into()),
            None,
        )?;
        Ok(hwnd.0 as isize)
    }
}

/// 子 HWND の位置と大きさを変える（親のクライアント座標、物理ピクセル）。
pub fn set_rect(hwnd: isize, rect: PixelRect) {
    let (x, y, w, h) = rect;
    unsafe {
        // 失敗しても次の Resized で同じ値を入れ直すので、結果は破棄する。
        let _ = SetWindowPos(
            HWND(hwnd as *mut _),
            None,
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// HWND がまだ生きているか。破棄された HWND への present を止めるのに使う。
pub fn is_alive(hwnd: isize) -> bool {
    unsafe { IsWindow(Some(HWND(hwnd as *mut _))).as_bool() }
}

/// サーフェスの HWND に左クリックのメッセージを投げる（自動試験用。座標はクライアントの物理ピクセル）。
pub fn post_click(hwnd: isize, x: i32, y: i32) {
    let lparam = LPARAM((((y & 0xFFFF) << 16) | (x & 0xFFFF)) as isize);
    let h = HWND(hwnd as *mut _);
    unsafe {
        let _ = PostMessageW(Some(h), WM_LBUTTONDOWN, WPARAM(0), lparam);
        let _ = PostMessageW(Some(h), WM_LBUTTONUP, WPARAM(0), lparam);
    }
}

/// サーフェスの HWND にキー押下のメッセージを投げる（自動試験用）。
pub fn post_key(hwnd: isize, vk: u32) {
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_KEYDOWN,
            WPARAM(vk as usize),
            LPARAM(0),
        );
    }
}

/// 押している間のキーか（`GetKeyState` の上位ビット）。
fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

/// いま押している修飾キー。ドラッグの種類（ripple か roll、slip か slide）を選ぶ。
fn pointer_mods() -> Mods {
    Mods {
        shift: key_down(VK_SHIFT.0),
        alt: key_down(VK_MENU.0),
    }
}

/// 修飾キーを前に付けたキー名。
fn key_name(vk: u32) -> String {
    keys::with_modifiers(
        &keys::vk_name(vk),
        key_down(VK_CONTROL.0),
        key_down(VK_SHIFT.0),
        key_down(VK_MENU.0),
    )
}

/// サーフェスの HWND に、左ボタンを押して `from` から `to` まで動かして離すメッセージを投げる（自動試験用）。
pub fn post_drag(hwnd: isize, from: (i32, i32), to: (i32, i32)) {
    let lparam = |(x, y): (i32, i32)| LPARAM((((y & 0xFFFF) << 16) | (x & 0xFFFF)) as isize);
    let h = HWND(hwnd as *mut _);
    unsafe {
        let _ = PostMessageW(Some(h), WM_LBUTTONDOWN, WPARAM(0), lparam(from));
        let _ = PostMessageW(Some(h), WM_MOUSEMOVE, WPARAM(0), lparam(to));
        let _ = PostMessageW(Some(h), WM_LBUTTONUP, WPARAM(0), lparam(to));
    }
}

/// 押し続けの繰り返しか。Space の押し続けで再生と停止を繰り返さないよう、最初の押下だけをキー操作にする。
fn is_key_repeat(lparam: LPARAM) -> bool {
    lparam.0 & KEY_REPEAT_BIT != 0
}

fn push(ev: PointerEvent) {
    if let Some(shared) = SHARED.get() {
        let mut q = shared.input.lock().expect("input");
        if q.len() < INPUT_LIMIT {
            q.push(ev);
        }
    }
}

fn client_xy(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (f32::from(x), f32::from(y))
}

fn track_leave(hwnd: HWND) {
    if TRACKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let mut tme = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut tme);
    }
}

fn wheel_event(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> PointerEvent {
    let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
    // WM_MOUSEWHEEL の座標は画面の座標なので、クライアントの座標に直す。
    let (sx, sy) = client_xy(lparam);
    let mut p = POINT {
        x: sx as i32,
        y: sy as i32,
    };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut p);
    }
    PointerEvent::Wheel {
        x: p.x as f32,
        y: p.y as f32,
        dy: f32::from(delta) / WHEEL_DELTA,
        ctrl: wparam.0 & MK_CONTROL != 0,
    }
}

/// Alt と一緒に押しても既定の処理に渡すキー。F4（ウィンドウを閉じる）だけ。
fn passes_to_system(vk: u32) -> bool {
    vk == u32::from(VK_F4.0)
}

/// キーの押下を、押し続けの繰り返しを除いてキーの表に渡す。
fn key_pressed(wparam: WPARAM, lparam: LPARAM) {
    if is_key_repeat(lparam) {
        return;
    }
    if let Some(shared) = SHARED.get() {
        keys::dispatch(shared, "surface", &key_name(wparam.0 as u32));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // 子 HWND は自動ではフォーカスを取らないので、クリックで自分に移す。ドラッグ中は外に出ても追う。
        WM_LBUTTONDOWN => {
            unsafe {
                let _ = SetFocus(Some(hwnd));
                SetCapture(hwnd);
            }
            let (x, y) = client_xy(lparam);
            push(PointerEvent::Down {
                x,
                y,
                mods: pointer_mods(),
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            push(PointerEvent::Up);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            track_leave(hwnd);
            let (x, y) = client_xy(lparam);
            push(PointerEvent::Move { x, y });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING.store(false, Ordering::SeqCst);
            push(PointerEvent::Leave);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            push(wheel_event(hwnd, wparam, lparam));
            LRESULT(0)
        }
        WM_KEYDOWN => {
            key_pressed(wparam, lparam);
            LRESULT(0)
        }
        // Alt を押している間のキーは WM_SYSKEYDOWN で届く。Alt+F4 だけは既定の処理（ウィンドウを閉じる）に渡す。
        WM_SYSKEYDOWN if !passes_to_system(wparam.0 as u32) => {
            key_pressed(wparam, lparam);
            LRESULT(0)
        }
        // Alt を離したときの既定の処理はメニューモード（SC_KEYMENU）に入り、次のキーを奪うので渡さない。
        WM_SYSKEYUP if !passes_to_system(wparam.0 as u32) => LRESULT(0),
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 押し続けの繰り返しは最初の押下と見分ける() {
        assert!(!is_key_repeat(LPARAM(1)), "最初の押下は直前のビットが 0");
        assert!(is_key_repeat(LPARAM((1 << 30) | 1)));
    }

    #[test]
    fn alt_のキーは_f4_だけを既定の処理に渡す() {
        assert!(passes_to_system(u32::from(VK_F4.0)));
        assert!(
            !passes_to_system(u32::from(VK_MENU.0)),
            "Alt を離してもメニューモードに入らない"
        );
        assert!(!passes_to_system(0x4B), "Alt+K はキーの表へ");
    }
}
