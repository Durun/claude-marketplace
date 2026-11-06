//! wgpu のサーフェスを張る子 HWND。Tauri のウィンドウの下に自前で作り、フォーカスとキーを自分で受ける。

use std::sync::{Arc, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, LoadCursorW, RegisterClassW, SetWindowPos, CS_HREDRAW,
    CS_VREDRAW, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_NOPARENTNOTIFY, WS_VISIBLE,
};

use crate::keys;
use crate::shared::Shared;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

/// 親の HWND の下に子 HWND を作る。位置と大きさは物理ピクセル。
pub fn create(
    parent: isize,
    shared: Arc<Shared>,
    rect: (i32, i32, i32, i32),
) -> windows::core::Result<isize> {
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
pub fn set_rect(hwnd: isize, rect: (i32, i32, i32, i32)) {
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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // 子 HWND は自動ではフォーカスを取らないので、クリックで自分に移す。
        WM_LBUTTONDOWN => {
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if let Some(shared) = SHARED.get() {
                keys::dispatch(shared, "surface", &keys::vk_name(wparam.0 as u32));
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
