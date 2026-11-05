//! 自己診断が使う Win32 の入力の送り出しと、フォーカスの読み出し。

use std::mem::size_of;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEINPUT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowRect, GetWindowThreadProcessId,
    SetCursorPos, SetForegroundWindow, GUITHREADINFO,
};

fn hwnd(h: isize) -> HWND {
    HWND(h as *mut _)
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, size_of::<INPUT>() as i32);
    }
}

/// 仮想キー 1 つを押して離す。
pub fn tap_key(vk: u16) {
    let make = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[make(false), make(true)]);
}

/// 画面の座標（物理ピクセル）を左クリックする。
pub fn click_screen(x: i32, y: i32) {
    unsafe {
        let _ = SetCursorPos(x, y);
    }
    let make = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[make(MOUSEEVENTF_LEFTDOWN), make(MOUSEEVENTF_LEFTUP)]);
}

/// HWND の画面上の矩形（左、上、右、下）。
pub fn window_rect(h: isize) -> (i32, i32, i32, i32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd(h), &mut r);
    }
    (r.left, r.top, r.right, r.bottom)
}

/// 親のクライアント座標を画面の座標にする。
pub fn client_to_screen(parent: isize, x: i32, y: i32) -> (i32, i32) {
    let mut p = POINT { x, y };
    unsafe {
        let _ = ClientToScreen(hwnd(parent), &mut p);
    }
    (p.x, p.y)
}

/// 自分のプロセスの窓が前面にあるか。
pub fn is_foreground_ours() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

/// 窓を前面に出す。OS が断ることがあるので、結果は `is_foreground_ours` で確かめる。
pub fn bring_to_front(h: isize) {
    unsafe {
        let _ = SetForegroundWindow(hwnd(h));
    }
}

/// 窓のスレッドでキーボードフォーカスを持つ HWND とそのクラス名。
pub fn focused(top: isize) -> (isize, String) {
    unsafe {
        let tid = GetWindowThreadProcessId(hwnd(top), None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(tid, &mut info).is_err() {
            return (0, String::new());
        }
        let mut buf = [0u16; 128];
        let n = GetClassNameW(info.hwndFocus, &mut buf).max(0) as usize;
        (
            info.hwndFocus.0 as isize,
            String::from_utf16_lossy(&buf[..n]),
        )
    }
}
