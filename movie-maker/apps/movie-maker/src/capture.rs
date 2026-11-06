//! ウィンドウのスクリーンショット。画面の同じ矩形を GDI で写して PNG にする。
//! DWM が合成した後の画面を写すので、子 HWND のサーフェスも子 WebView も入る（ほかのウィンドウに隠れていないこと）。

use std::path::Path;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

/// 1 画素のバイト数（BGRA）。
const BYTES_PER_PIXEL: usize = 4;

/// `hwnd` の画面上の矩形を `path` に PNG で書く。
pub fn capture_window(hwnd: isize, path: &Path) -> Result<(), String> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut rect) }.map_err(|e| e.to_string())?;
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w <= 0 || h <= 0 {
        return Err("ウィンドウの大きさが 0".into());
    }
    let bgra = read_screen(rect.left, rect.top, w, h)?;
    write_png(path, w as u32, h as u32, &bgra)
}

fn read_screen(x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>, String> {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bitmap.into());
        let blit = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // 負の高さで、上の行から並べる。
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; w as usize * h as usize * BYTES_PER_PIXEL];
        let lines = GetDIBits(
            mem,
            bitmap,
            0,
            h as u32,
            Some(buf.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        blit.map_err(|e| e.to_string())?;
        if lines == 0 {
            return Err("GetDIBits が 0 行".into());
        }
        Ok(buf)
    }
}

fn write_png(path: &Path, w: u32, h: u32, bgra: &[u8]) -> Result<(), String> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(BYTES_PER_PIXEL) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_を_rgba_の_png_にする() {
        let dir = std::env::temp_dir().join(format!("mm_capture_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.png");
        write_png(&path, 2, 1, &[10, 20, 30, 0, 40, 50, 60, 0]).unwrap();
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(buf, [30, 20, 10, 255, 60, 50, 40, 255]);
    }
}
