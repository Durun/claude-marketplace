//! 映像のトラック: RGBA f32 → NV12（Rec.709、tv）→ libx264。レンダラーが GPU で作った NV12 はそのまま押す。

use ff::color::{Range, Space, TransferCharacteristic};
use ff::format::context::Output;
use ff::format::Pixel;
use ff::software::scaling::{Context as Scaler, Flags};
use ff::{codec, encoder, ffi, frame, Rational};
use ffmpeg_next as ff;
use mm_decode_api::{EncodeSettings, Nv12Frame};
use mm_types::{MmError, Result, RgbaImage, Size};

use crate::transfer::EncodeTable;

/// x264 の品質（小さいほど高画質）。編集の途中の確認と配信の両方に足りる値。
const CRF: &str = "18";
const PRESET: &str = "medium";

/// 映像のエンコーダーと、出力の中のストリーム。
pub struct VideoTrack {
    encoder: codec::encoder::video::Encoder,
    scaler: Scaler,
    stream_index: usize,
    encoder_tb: Rational,
    stream_tb: Rational,
    size: Size,
    next_pts: i64,
    /// 線形 → 符号化値。スケーラーに渡す前にエンコードし直す。
    table: EncodeTable,
}

impl VideoTrack {
    /// 出力にストリームを足し、エンコーダーを開く。
    pub fn open(
        output: &mut Output,
        settings: &EncodeSettings,
        global_header: bool,
    ) -> Result<Self> {
        let size = settings.size;
        if size.width == 0
            || size.height == 0
            || !size.width.is_multiple_of(2)
            || !size.height.is_multiple_of(2)
        {
            return Err(MmError::InvalidArgument(format!(
                "NV12 は偶数の大きさが要る: {}x{}",
                size.width, size.height
            )));
        }
        let fps = settings.fps;
        let (num, den) = (
            i32::try_from(fps.num()).map_err(|_| too_large("fps"))?,
            i32::try_from(fps.den()).map_err(|_| too_large("fps"))?,
        );
        let codec = encoder::find_by_name("libx264")
            .ok_or_else(|| MmError::Unsupported("libx264 が無い FFmpeg".into()))?;
        let mut stream = output
            .add_stream(codec)
            .map_err(|e| MmError::Internal(format!("映像ストリーム: {e}")))?;
        let stream_index = stream.index();
        let encoder_tb = Rational::new(den, num);
        let opened = open_encoder(codec, size, encoder_tb, global_header)?;
        stream.set_parameters(&opened);
        stream.set_time_base(encoder_tb);

        Ok(Self {
            encoder: opened,
            scaler: make_scaler(size)?,
            stream_index,
            encoder_tb,
            stream_tb: encoder_tb,
            size,
            next_pts: 0,
            table: EncodeTable::new(),
        })
    }

    /// ヘッダーを書いた後の、muxer が決めたストリームの時間の単位を控える。
    pub fn after_header(mut self, output: &Output) -> Self {
        if let Some(stream) = output.stream(self.stream_index) {
            self.stream_tb = stream.time_base();
        }
        self
    }

    /// 1 フレームをエンコードして書く。
    pub fn push(&mut self, output: &mut Output, image: &RgbaImage) -> Result<()> {
        if image.size() != self.size {
            return Err(MmError::InvalidArgument(
                "フレームの大きさが設定と違う".into(),
            ));
        }
        let mut rgba = frame::Video::new(Pixel::RGBA, self.size.width, self.size.height);
        fill_rgba(&mut rgba, image, &self.table);
        // エンコーダーが参照を持ち続けるので、フレームごとに新しい YUV の領域を使う
        let mut yuv = frame::Video::new(Pixel::NV12, self.size.width, self.size.height);
        self.scaler
            .run(&rgba, &mut yuv)
            .map_err(|e| MmError::Internal(format!("YUV への変換: {e}")))?;
        self.send(output, yuv)
    }

    /// GPU で作った NV12 の 1 フレームを、変換せずにエンコードして書く。
    pub fn push_nv12(&mut self, output: &mut Output, nv12: &Nv12Frame) -> Result<()> {
        if nv12.size != self.size {
            return Err(MmError::InvalidArgument(
                "フレームの大きさが設定と違う".into(),
            ));
        }
        let mut yuv = frame::Video::new(Pixel::NV12, self.size.width, self.size.height);
        copy_plane(&mut yuv, 0, &nv12.y, self.size.width as usize);
        copy_plane(&mut yuv, 1, &nv12.uv, self.size.width as usize);
        self.send(output, yuv)
    }

    fn send(&mut self, output: &mut Output, mut yuv: frame::Video) -> Result<()> {
        yuv.set_pts(Some(self.next_pts));
        self.next_pts += 1;
        self.encoder
            .send_frame(&yuv)
            .map_err(|e| MmError::Internal(format!("映像の送信: {e}")))?;
        self.drain(output)
    }

    /// エンコーダーに残ったフレームを出し切る。
    pub fn finish(&mut self, output: &mut Output) -> Result<()> {
        self.encoder
            .send_eof()
            .map_err(|e| MmError::Internal(format!("映像の終端: {e}")))?;
        self.drain(output)
    }

    fn drain(&mut self, output: &mut Output) -> Result<()> {
        crate::drain_packets(
            &mut self.encoder,
            output,
            self.stream_index,
            self.encoder_tb,
            self.stream_tb,
            1,
        )
    }
}

/// 行の余白の無いプレーンを、FFmpeg のフレームのプレーン `plane`（行の余白つき）へ写す。
fn copy_plane(dst: &mut frame::Video, plane: usize, src: &[u8], row: usize) {
    let stride = dst.stride(plane);
    let data = dst.data_mut(plane);
    for (line, out) in src.chunks_exact(row).zip(data.chunks_exact_mut(stride)) {
        out[..row].copy_from_slice(line);
    }
}

/// libx264 を Rec.709・tv レンジの NV12 で開く。スレッド数は自動（0）にする。
fn open_encoder(
    codec: ff::Codec,
    size: Size,
    time_base: Rational,
    global_header: bool,
) -> Result<codec::encoder::video::Encoder> {
    let mut context = codec::context::Context::new_with_codec(codec);
    // 1 スレッドだと 1080p で数倍遅い。フレームの並列は x264 側のスレッドで効く。
    context.set_threading(ff::threading::Config {
        kind: ff::threading::Type::Frame,
        ..Default::default()
    });
    let mut enc = context
        .encoder()
        .video()
        .map_err(|e| MmError::Internal(format!("映像のエンコーダー: {e}")))?;
    enc.set_width(size.width);
    enc.set_height(size.height);
    enc.set_format(Pixel::NV12);
    enc.set_time_base(time_base);
    enc.set_frame_rate(Some(Rational::new(
        time_base.denominator(),
        time_base.numerator(),
    )));
    enc.set_colorspace(Space::BT709);
    enc.set_color_range(Range::MPEG);
    enc.set_color_primaries(ff::color::Primaries::BT709);
    enc.set_color_transfer_characteristic(TransferCharacteristic::BT709);
    if global_header {
        enc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let mut options = ff::Dictionary::new();
    options.set("crf", CRF);
    options.set("preset", PRESET);
    enc.open_with(options)
        .map_err(|e| MmError::Internal(format!("libx264 を開けない: {e}")))
}

fn too_large(what: &str) -> MmError {
    MmError::InvalidArgument(format!("{what} が大きすぎる"))
}

fn make_scaler(size: Size) -> Result<Scaler> {
    let mut scaler = Scaler::get(
        Pixel::RGBA,
        size.width,
        size.height,
        Pixel::NV12,
        size.width,
        size.height,
        Flags::BICUBIC | Flags::ACCURATE_RND | Flags::FULL_CHR_H_INT,
    )
    .map_err(|e| MmError::Internal(format!("swscale: {e}")))?;
    // RGB は 0..255 のフルレンジ、YUV は Rec.709 の tv レンジ
    unsafe {
        let bt709 = ffi::sws_getCoefficients(ffi::SWS_CS_ITU709);
        ffi::sws_setColorspaceDetails(scaler.as_mut_ptr(), bt709, 1, bt709, 0, 0, 1 << 16, 1 << 16);
    }
    Ok(scaler)
}

/// 線形の f32 の画素を Rec.1886 の 8 ビットにエンコードして RGBA のフレームへコピーする。
/// アルファは使わず不透明として書く（premultiplied なので黒の上に置いた色と同じ）。
fn fill_rgba(dst: &mut frame::Video, image: &RgbaImage, table: &EncodeTable) {
    let width = image.width() as usize;
    let stride = dst.stride(0);
    let data = dst.data_mut(0);
    for (y, row) in image.data().chunks_exact(width * 4).enumerate() {
        let out = &mut data[y * stride..y * stride + width * 4];
        for (o, px) in out.chunks_exact_mut(4).zip(row.chunks_exact(4)) {
            o[0] = table.encode(px[0]);
            o[1] = table.encode(px[1]);
            o[2] = table.encode(px[2]);
            o[3] = 255;
        }
    }
}
