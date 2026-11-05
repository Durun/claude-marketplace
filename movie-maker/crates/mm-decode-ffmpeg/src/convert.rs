//! 復号したコマ（YUV など）を CPU の RGBA f32 にする。

use ff::color::{Range, Space};
use ff::format::Pixel;
use ff::software::scaling::{Context as Scaler, Flags};
use ff::{ffi, frame};
use ffmpeg_next as ff;
use mm_types::{ColorSpace, MmError, Result, RgbaImage, Size};

/// 変換器の作り直しが要るかを決める、入力の性質。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceKey {
    format: Pixel,
    width: u32,
    height: u32,
    matrix: Matrix,
    full_range: bool,
}

/// YUV から RGB への行列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    /// フレームの宣言から。未指定は画の高さで決める（HD 以上は 709）。
    pub fn from_frame(space: Space, height: u32) -> Self {
        match space {
            Space::BT709 => Self::Bt709,
            Space::BT2020NCL | Space::BT2020CL => Self::Bt2020,
            Space::SMPTE170M | Space::BT470BG | Space::FCC | Space::SMPTE240M => Self::Bt601,
            _ if height >= 720 => Self::Bt709,
            _ => Self::Bt601,
        }
    }

    fn sws_id(self) -> i32 {
        match self {
            Self::Bt601 => ffi::SWS_CS_ITU601,
            Self::Bt709 => ffi::SWS_CS_ITU709,
            Self::Bt2020 => ffi::SWS_CS_BT2020,
        }
    }

    /// 文書に出す色空間の名前。フルレンジには `_full` を付ける。
    pub fn color_space(self, full_range: bool) -> ColorSpace {
        let base = match self {
            Self::Bt601 => "bt601",
            Self::Bt709 => "bt709",
            Self::Bt2020 => "bt2020",
        };
        ColorSpace(if full_range {
            format!("{base}_full")
        } else {
            base.to_owned()
        })
    }
}

/// フレームのレンジの宣言。未指定は tv レンジ。
pub fn is_full_range(range: Range) -> bool {
    matches!(range, Range::JPEG)
}

/// swscale を 1 つ持ち、入力の性質が変わったときだけ作り直す。
#[derive(Default)]
pub struct RgbaConverter {
    scaler: Option<(SourceKey, Scaler)>,
    out: Option<frame::Video>,
}

// swscale の文脈は生ポインタを持つため自動では Send にならない。所有は 1 つのデコーダーだけで、
// FFmpeg の文脈はスレッドに紐づかないので、デコーダーごと別スレッドへ移してよい。
unsafe impl Send for RgbaConverter {}

impl RgbaConverter {
    /// 1 コマを RGBA の絵にする。アルファがあれば premultiplied にする。
    pub fn convert(&mut self, src: &frame::Video) -> Result<RgbaImage> {
        let key = SourceKey {
            format: src.format(),
            width: src.width(),
            height: src.height(),
            matrix: Matrix::from_frame(src.color_space(), src.height()),
            full_range: is_full_range(src.color_range()),
        };
        if self.scaler.as_ref().map(|(k, _)| *k) != Some(key) {
            self.scaler = Some((key, make_scaler(&key)?));
            self.out = None;
        }
        let (_, scaler) = self.scaler.as_mut().expect("直前に作った");
        let out = self
            .out
            .get_or_insert_with(|| frame::Video::new(Pixel::RGBA, key.width, key.height));
        scaler
            .run(src, out)
            .map_err(|e| MmError::Corrupt(format!("RGBA への変換: {e}")))?;
        to_image(out, Size::new(key.width, key.height))
    }
}

fn make_scaler(key: &SourceKey) -> Result<Scaler> {
    let mut scaler = Scaler::get(
        key.format,
        key.width,
        key.height,
        Pixel::RGBA,
        key.width,
        key.height,
        Flags::BICUBIC | Flags::ACCURATE_RND | Flags::FULL_CHR_H_INT,
    )
    .map_err(|e| MmError::Unsupported(format!("{:?} を RGBA にできない: {e}", key.format)))?;
    // swscale の既定は 601 のため、宣言した行列とレンジを明示する。RGB 入力では -1 が返るが無視してよい。
    unsafe {
        let coeffs = ffi::sws_getCoefficients(key.matrix.sws_id());
        ffi::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            coeffs,
            key.full_range as i32,
            coeffs,
            1,
            0,
            1 << 16,
            1 << 16,
        );
    }
    Ok(scaler)
}

fn to_image(frame: &frame::Video, size: Size) -> Result<RgbaImage> {
    let (w, h) = (size.width as usize, size.height as usize);
    let stride = frame.stride(0);
    let data = frame.data(0);
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &data[y * stride..y * stride + w * 4];
        for px in row.chunks_exact(4) {
            let a = px[3] as f32 / 255.0;
            out.push(px[0] as f32 / 255.0 * a);
            out.push(px[1] as f32 / 255.0 * a);
            out.push(px[2] as f32 / 255.0 * a);
            out.push(a);
        }
    }
    RgbaImage::from_raw(size, out)
}
