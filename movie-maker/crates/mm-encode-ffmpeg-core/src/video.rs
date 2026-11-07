//! 映像のトラック: 線形の RGBA の f32 → エンコーダーの画素形式（Rec.709、tv レンジ）→ エンコーダー。
//! レンダラーが GPU で作った NV12 は、NV12 を受けるコーデックならそのまま送る。

use std::path::PathBuf;

use ff::color::{Range, Space, TransferCharacteristic};
use ff::format::context::Output;
use ff::format::Pixel;
use ff::software::scaling::{Context as Scaler, Flags};
use ff::{codec, encoder, ffi, frame, Rational};
use ffmpeg_next as ff;
use mm_decode_api::{EncodeSettings, Nv12Frame};
use mm_types::{MmError, Result, RgbaImage, Size};

use crate::spec::{CodecSpec, Rate, VideoInput};
use crate::transfer::{EncodeTable, EncodeTable16};

/// 何回目の映像か。2 パスの 1 回目は出力に書かず統計だけを取る。
#[derive(Debug, Clone)]
pub enum Pass {
    Single,
    /// 統計ファイルのパス。
    First(PathBuf),
    Second(PathBuf),
}

/// 映像のエンコーダーと、出力の中のストリーム。
pub struct VideoTrack {
    encoder: codec::encoder::video::Encoder,
    scaler: Scaler,
    /// 1 回目のパスは出力を持たない。
    stream_index: Option<usize>,
    encoder_tb: Rational,
    stream_tb: Rational,
    size: Size,
    next_pts: i64,
    input: VideoInput,
    /// 線形 → 符号化値。スケーラーに渡す前にエンコードし直す。8 ビットは NV12 向け、16 ビットは 10 ビットの YUV 向け。
    table8: EncodeTable,
    table16: Option<EncodeTable16>,
}

impl VideoTrack {
    /// 出力にストリームを足し（`output` があれば）、エンコーダーを開く。
    pub fn open(
        output: Option<&mut Output>,
        settings: &EncodeSettings,
        spec: &CodecSpec,
        pass: &Pass,
        global_header: bool,
    ) -> Result<Self> {
        let size = settings.size;
        if size.width == 0
            || size.height == 0
            || !size.width.is_multiple_of(2)
            || !size.height.is_multiple_of(2)
        {
            return Err(MmError::InvalidArgument(format!(
                "偶数の大きさが要る: {}x{}",
                size.width, size.height
            )));
        }
        let fps = settings.fps;
        let (num, den) = (
            i32::try_from(fps.num()).map_err(|_| too_large("fps"))?,
            i32::try_from(fps.den()).map_err(|_| too_large("fps"))?,
        );
        let codec = encoder::find_by_name(spec.encoder)
            .ok_or_else(|| MmError::Unsupported(format!("{} が無い FFmpeg", spec.encoder)))?;
        let encoder_tb = Rational::new(den, num);
        let opened = open_encoder(codec, settings, spec, pass, encoder_tb, global_header)?;
        let mut stream_index = None;
        let mut stream_tb = encoder_tb;
        if let Some(output) = output {
            let mut stream = output
                .add_stream(codec)
                .map_err(|e| MmError::Internal(format!("映像ストリーム: {e}")))?;
            stream_index = Some(stream.index());
            stream.set_parameters(&opened);
            stream.set_time_base(encoder_tb);
            stream_tb = stream.time_base();
        }
        Ok(Self {
            encoder: opened,
            scaler: make_scaler(size, spec.input)?,
            stream_index,
            encoder_tb,
            stream_tb,
            size,
            next_pts: 0,
            input: spec.input,
            table8: EncodeTable::new(),
            table16: spec.input.is_high_bit_depth().then(EncodeTable16::new),
        })
    }

    /// ヘッダーを書いた後の、muxer が決めたストリームの時間の単位を記録する。
    pub fn after_header(mut self, output: &Output) -> Self {
        if let Some(stream) = self.stream_index.and_then(|i| output.stream(i)) {
            self.stream_tb = stream.time_base();
        }
        self
    }

    /// NV12 をそのまま送れるか。
    pub fn accepts_nv12(&self) -> bool {
        self.input == VideoInput::Nv12
    }

    /// 1 フレームをエンコードして書く。
    pub fn push(&mut self, output: Option<&mut Output>, image: &RgbaImage) -> Result<()> {
        if image.size() != self.size {
            return Err(MmError::InvalidArgument(
                "フレームの大きさが設定と違う".into(),
            ));
        }
        let (w, h) = (self.size.width, self.size.height);
        let rgba = match &self.table16 {
            Some(t16) => {
                let mut f = frame::Video::new(Pixel::RGBA64LE, w, h);
                fill_rgba64(&mut f, image, t16);
                f
            }
            None => {
                let mut f = frame::Video::new(Pixel::RGBA, w, h);
                fill_rgba(&mut f, image, &self.table8);
                f
            }
        };
        // エンコーダーが参照を持ち続けるので、フレームごとに新しい YUV の領域を使う
        let mut yuv = frame::Video::new(self.pixel(), w, h);
        self.scaler
            .run(&rgba, &mut yuv)
            .map_err(|e| MmError::Internal(format!("YUV への変換: {e}")))?;
        self.send(output, yuv)
    }

    /// GPU で作った NV12 の 1 フレームを、変換せずにエンコードして書く。
    pub fn push_nv12(&mut self, output: Option<&mut Output>, nv12: &Nv12Frame) -> Result<()> {
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

    fn pixel(&self) -> Pixel {
        pixel_of(self.input)
    }

    fn send(&mut self, output: Option<&mut Output>, mut yuv: frame::Video) -> Result<()> {
        yuv.set_pts(Some(self.next_pts));
        self.next_pts += 1;
        self.encoder
            .send_frame(&yuv)
            .map_err(|e| MmError::Internal(format!("映像の送信: {e}")))?;
        self.drain(output)
    }

    /// エンコーダーに残ったフレームを出し切る。
    pub fn finish(&mut self, output: Option<&mut Output>) -> Result<()> {
        self.encoder
            .send_eof()
            .map_err(|e| MmError::Internal(format!("映像の終端: {e}")))?;
        self.drain(output)
    }

    fn drain(&mut self, output: Option<&mut Output>) -> Result<()> {
        crate::writer::drain_packets(
            &mut self.encoder,
            output,
            self.stream_index.unwrap_or(0),
            self.encoder_tb,
            self.stream_tb,
            1,
        )
    }
}

fn pixel_of(input: VideoInput) -> Pixel {
    match input {
        VideoInput::Nv12 => Pixel::NV12,
        VideoInput::Yuv422p8 => Pixel::YUV422P,
        VideoInput::Yuv422p10 => Pixel::YUV422P10LE,
        VideoInput::Yuv444p10 => Pixel::YUV444P10LE,
    }
}

/// 行の余白の無いプレーンを、FFmpeg のフレームのプレーン `plane`（行の余白つき）へコピーする。
fn copy_plane(dst: &mut frame::Video, plane: usize, src: &[u8], row: usize) {
    let stride = dst.stride(plane);
    let data = dst.data_mut(plane);
    for (line, out) in src.chunks_exact(row).zip(data.chunks_exact_mut(stride)) {
        out[..row].copy_from_slice(line);
    }
}

/// 目標のビットレート（bit/s）。大きさ x fps x 1 画素 1 フレームあたりのビット数。
fn target_bit_rate(size: Size, fps: f64, bits_per_pixel: f64) -> usize {
    (f64::from(size.width) * f64::from(size.height) * fps * bits_per_pixel) as usize
}

/// 画の大きさ、画素形式、時間の単位、色（Rec.709・tv レンジ）を書く。
fn describe_picture(
    enc: &mut codec::encoder::video::Video,
    size: Size,
    input: VideoInput,
    time_base: Rational,
) {
    enc.set_width(size.width);
    enc.set_height(size.height);
    enc.set_format(pixel_of(input));
    enc.set_time_base(time_base);
    enc.set_frame_rate(Some(Rational::new(
        time_base.denominator(),
        time_base.numerator(),
    )));
    enc.set_colorspace(Space::BT709);
    enc.set_color_range(Range::MPEG);
    enc.set_color_primaries(ff::color::Primaries::BT709);
    enc.set_color_transfer_characteristic(TransferCharacteristic::BT709);
}

/// パス（1 パス、2 パスの 1 回目と 2 回目）とグローバルヘッダーのフラグ、統計ファイルのオプション。
fn pass_options(pass: &Pass, global_header: bool) -> (codec::Flags, ff::Dictionary<'static>) {
    let mut flags = codec::Flags::empty();
    if global_header {
        flags |= codec::Flags::GLOBAL_HEADER;
    }
    let mut options = ff::Dictionary::new();
    let stats = match pass {
        Pass::Single => None,
        Pass::First(stats) => Some((codec::Flags::PASS1, stats)),
        Pass::Second(stats) => Some((codec::Flags::PASS2, stats)),
    };
    if let Some((flag, stats)) = stats {
        flags |= flag;
        options.set("stats", &stats.to_string_lossy());
    }
    (flags, options)
}

/// エンコーダーを Rec.709・tv レンジで開く。スレッド数は自動（0）にする。
fn open_encoder(
    codec: ff::Codec,
    settings: &EncodeSettings,
    spec: &CodecSpec,
    pass: &Pass,
    time_base: Rational,
    global_header: bool,
) -> Result<codec::encoder::video::Encoder> {
    let size = settings.size;
    let mut context = codec::context::Context::new_with_codec(codec);
    // 1 スレッドだと 1080p で数倍遅い。フレームの並列は x264 と ProRes のスレッドで効く。
    context.set_threading(ff::threading::Config {
        kind: ff::threading::Type::Frame,
        ..Default::default()
    });
    let mut enc = context
        .encoder()
        .video()
        .map_err(|e| MmError::Internal(format!("映像のエンコーダー: {e}")))?;
    describe_picture(&mut enc, size, spec.input, time_base);
    let (flags, mut options) = pass_options(pass, global_header);
    enc.set_flags(flags);
    match &spec.rate {
        Rate::Quality(key, value) => options.set(key, value),
        Rate::BitsPerPixel(bpp) => {
            enc.set_bit_rate(target_bit_rate(size, settings.fps.to_f64(), *bpp));
        }
        Rate::Profile => {}
    }
    for (key, value) in &spec.options {
        options.set(key, value);
    }
    if let Some(tag) = spec.codec_tag {
        // SAFETY: 開く前のコンテキストの codec_tag に整数を書くだけ。
        unsafe { (*enc.as_mut_ptr()).codec_tag = u32::from_le_bytes(tag) };
    }
    enc.open_with(options)
        .map_err(|e| MmError::Unsupported(format!("{} を開けない: {e}", spec.encoder)))
}

fn too_large(what: &str) -> MmError {
    MmError::InvalidArgument(format!("{what} が大きすぎる"))
}

fn make_scaler(size: Size, input: VideoInput) -> Result<Scaler> {
    let source = if input.is_high_bit_depth() {
        Pixel::RGBA64LE
    } else {
        Pixel::RGBA
    };
    let mut scaler = Scaler::get(
        source,
        size.width,
        size.height,
        pixel_of(input),
        size.width,
        size.height,
        Flags::BICUBIC | Flags::ACCURATE_RND | Flags::FULL_CHR_H_INT,
    )
    .map_err(|e| MmError::Internal(format!("swscale: {e}")))?;
    // RGB は フルレンジ、YUV は Rec.709 の tv レンジ
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

/// `fill_rgba` の 16 ビットにしたもの（リトルエンディアン）。
fn fill_rgba64(dst: &mut frame::Video, image: &RgbaImage, table: &EncodeTable16) {
    let width = image.width() as usize;
    let stride = dst.stride(0);
    let data = dst.data_mut(0);
    for (y, row) in image.data().chunks_exact(width * 4).enumerate() {
        let out = &mut data[y * stride..y * stride + width * 8];
        for (o, px) in out.chunks_exact_mut(8).zip(row.chunks_exact(4)) {
            for c in 0..3 {
                o[c * 2..c * 2 + 2].copy_from_slice(&table.encode(px[c]).to_le_bytes());
            }
            o[6..8].copy_from_slice(&u16::MAX.to_le_bytes());
        }
    }
}
