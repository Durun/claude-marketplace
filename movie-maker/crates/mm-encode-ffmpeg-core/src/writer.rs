//! `Encoder` の実装。コーデックの指定（`CodecSpec`）を名前から引く関数を受け取り、`begin` の設定で決める。

use std::path::{Path, PathBuf};

use ff::format::context::Output;
use ff::{codec, encoder, format, Packet, Rational};
use ffmpeg_next as ff;
use mm_audio_api::AudioBlock;
use mm_decode_api::{EncodeReport, EncodeSettings, Encoder, ExtraTracks, Nv12Frame};
use mm_types::{MmError, Result, RgbaImage, Size};

use crate::audio::AudioTrack;
use crate::extras;
use crate::spec::{CodecSpec, Container};
use crate::video::{Pass, VideoTrack};

/// コーデックの名前から指定を引く関数。扱えない名前は `None`。
pub type SpecResolver = fn(&str) -> Option<CodecSpec>;

/// FFmpeg を初期化し、ログをエラーだけにする。何度呼んでもよい。
pub fn init() -> Result<()> {
    ff::init().map_err(|e| MmError::Internal(format!("FFmpeg の初期化: {e}")))?;
    ff::util::log::set_level(ff::util::log::Level::Error);
    Ok(())
}

/// この名前の FFmpeg のエンコーダーが入っているか。
pub fn encoder_available(name: &str) -> bool {
    init().is_ok() && encoder::find_by_name(name).is_some()
}

/// この指定のエンコーダーを実際に開けるか。NVENC のように、エンコーダーが FFmpeg に入っていても
/// ハードウェア（NVIDIA の GPU とドライバー）が無いと開けないものの判定に使う。
pub fn can_open(spec: &CodecSpec) -> bool {
    const PROBE_SIZE: Size = Size {
        width: 256,
        height: 144,
    };
    if !encoder_available(spec.encoder) {
        return false;
    }
    let settings = EncodeSettings {
        size: PROBE_SIZE,
        fps: mm_types::Rational::integer(30),
        codec: spec.encoder.to_owned(),
        output_path: PathBuf::new(),
        audio: None,
    };
    VideoTrack::open(None, &settings, spec, &Pass::Single, false).is_ok()
}

/// 書き出し 1 本。`begin` から `finish` まで。
pub struct FfmpegEncoder {
    resolve: SpecResolver,
    state: State,
    /// `begin` で決めた、映像を通す回数。
    passes: u32,
    /// `set_extras` で渡された字幕とチャプター。`begin` が使う。
    extras: ExtraTracks,
}

enum State {
    Idle,
    /// 2 パスの 1 回目。出力は開いておらず、統計だけを取る。
    Analysis(Box<Analysis>),
    Running(Box<Session>),
}

struct Analysis {
    spec: CodecSpec,
    settings: EncodeSettings,
    video: VideoTrack,
    stats: StatsFiles,
}

/// 開いた出力と 2 本のトラック。
struct Session {
    output: Output,
    video: VideoTrack,
    audio: Option<AudioTrack>,
    settings: EncodeSettings,
    frames: u64,
    audio_frames: u64,
    /// 2 パスの統計ファイル。書き終えたら消す。
    _stats: Option<StatsFiles>,
}

// FFmpeg のコンテキストは生ポインタを持つため自動では Send にならない。Session と Analysis は
// 1 つの FfmpegEncoder だけが所有し、コンテキストはスレッドに紐づかないので、別スレッドへ移してよい。
unsafe impl Send for Session {}
unsafe impl Send for Analysis {}

/// x264 の統計ファイルと、それに付いてくるファイル。値がドロップされたら消す。
struct StatsFiles {
    path: PathBuf,
}

impl StatsFiles {
    fn beside(output: &Path) -> Self {
        let mut name = output.as_os_str().to_owned();
        name.push(".2pass.log");
        Self {
            path: PathBuf::from(name),
        }
    }
}

impl Drop for StatsFiles {
    fn drop(&mut self) {
        for suffix in ["", ".mbtree", ".temp", ".mbtree.temp"] {
            let mut name = self.path.as_os_str().to_owned();
            name.push(suffix);
            let _ = std::fs::remove_file(name);
        }
    }
}

impl FfmpegEncoder {
    /// `resolve` で名前から指定を引くエンコーダーを作る。
    pub fn new(resolve: SpecResolver) -> Self {
        Self {
            resolve,
            state: State::Idle,
            passes: 1,
            extras: ExtraTracks::default(),
        }
    }

    fn spec_of(&self, settings: &EncodeSettings) -> Result<CodecSpec> {
        (self.resolve)(&settings.codec).ok_or_else(|| {
            MmError::Unsupported(format!("コーデック {} は扱えない", settings.codec))
        })
    }

    fn session_mut(&mut self, call: &str) -> Result<&mut Session> {
        match &mut self.state {
            State::Running(s) => Ok(s),
            State::Analysis(_) => Err(MmError::InvalidArgument(format!(
                "2 パスの 1 回目に {call} は呼べない（映像は begin_pass(2) の後、音は映像の後）"
            ))),
            State::Idle => Err(MmError::InvalidArgument(format!("begin の前に {call}"))),
        }
    }

    /// 出力と映像・音のトラックを開く。
    fn open_session(
        settings: &EncodeSettings,
        spec: &CodecSpec,
        pass: &Pass,
        stats: Option<StatsFiles>,
        extras: &ExtraTracks,
    ) -> Result<Session> {
        let mut output = format::output_as(&settings.output_path, spec.container.muxer())
            .map_err(|e| MmError::Io(std::io::Error::other(format!("出力を開けない: {e}"))))?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);
        let video = VideoTrack::open(Some(&mut output), settings, spec, pass, global_header)?;
        let audio = settings
            .audio
            .as_ref()
            .map(|a| AudioTrack::open(&mut output, a, spec.audio, global_header))
            .transpose()?;
        let subtitles = extras::open(&mut output, extras)?;
        let mut options = ff::Dictionary::new();
        if spec.container == Container::Mp4 {
            options.set("movflags", "+faststart");
        }
        // B フレームの遅れと AAC の前置きは muxer の edit list が吸収する。時刻をずらす指定（avoid_negative_ts）を
        // 付けると、パケットだけがずれてヘッダーのチャプターが映像より早くなる。
        output
            .write_header_with(options)
            .map_err(|e| MmError::Internal(format!("ヘッダーの書き込み: {e}")))?;
        let video = video.after_header(&output);
        let audio = audio.map(|a| a.after_header(&output));
        if let Some(track) = subtitles {
            track.after_header(&output).write(&mut output)?;
        }
        Ok(Session {
            output,
            video,
            audio,
            settings: settings.clone(),
            frames: 0,
            audio_frames: 0,
            _stats: stats,
        })
    }
}

impl Encoder for FfmpegEncoder {
    fn set_extras(&mut self, extras: ExtraTracks) -> Result<()> {
        if !matches!(self.state, State::Idle) {
            return Err(MmError::InvalidArgument("begin の後に set_extras".into()));
        }
        self.extras = extras;
        Ok(())
    }

    fn begin(&mut self, settings: &EncodeSettings) -> Result<()> {
        if !matches!(self.state, State::Idle) {
            return Err(MmError::InvalidArgument("begin を 2 回呼んだ".into()));
        }
        init()?;
        let spec = self.spec_of(settings)?;
        if !encoder_available(spec.encoder) {
            return Err(MmError::Unsupported(format!(
                "{} が無い FFmpeg",
                spec.encoder
            )));
        }
        self.passes = spec.passes.max(1);
        self.state = if spec.passes >= 2 {
            let stats = StatsFiles::beside(&settings.output_path);
            let video = VideoTrack::open(
                None,
                settings,
                &spec,
                &Pass::First(stats.path.clone()),
                false,
            )?;
            State::Analysis(Box::new(Analysis {
                spec,
                settings: settings.clone(),
                video,
                stats,
            }))
        } else {
            State::Running(Box::new(Self::open_session(
                settings,
                &spec,
                &Pass::Single,
                None,
                &self.extras,
            )?))
        };
        Ok(())
    }

    fn passes(&self) -> u32 {
        self.passes
    }

    fn begin_pass(&mut self, pass: u32) -> Result<()> {
        match (std::mem::replace(&mut self.state, State::Idle), pass) {
            (State::Analysis(a), 1) => {
                self.state = State::Analysis(a);
                Ok(())
            }
            (State::Analysis(mut a), 2) => {
                a.video.finish(None)?;
                let Analysis {
                    spec,
                    settings,
                    video,
                    stats,
                } = *a;
                // x264 は閉じるときに統計ファイルを書き終える。2 回目が読む前に閉じる。
                drop(video);
                let pass = Pass::Second(stats.path.clone());
                self.state = State::Running(Box::new(Self::open_session(
                    &settings,
                    &spec,
                    &pass,
                    Some(stats),
                    &self.extras,
                )?));
                Ok(())
            }
            (State::Running(s), 1) => {
                self.state = State::Running(s);
                Ok(())
            }
            (state, n) => {
                self.state = state;
                Err(MmError::InvalidArgument(format!(
                    "begin_pass({n}) は今の状態では呼べない"
                )))
            }
        }
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        if let State::Analysis(a) = &mut self.state {
            for image in frames {
                a.video.push(None, image)?;
            }
            return Ok(());
        }
        let s = self.session_mut("push_frames")?;
        for image in frames {
            s.video.push(Some(&mut s.output), image)?;
            s.frames += 1;
        }
        Ok(())
    }

    fn push_nv12_frames(&mut self, frames: &[Nv12Frame]) -> Result<()> {
        if let State::Analysis(a) = &mut self.state {
            return push_nv12_analysis(a, frames);
        }
        let s = self.session_mut("push_nv12_frames")?;
        if !s.video.accepts_nv12() {
            return Err(MmError::Unsupported(format!(
                "{} は NV12 を受けない",
                s.settings.codec
            )));
        }
        for frame in frames {
            s.video.push_nv12(Some(&mut s.output), frame)?;
            s.frames += 1;
        }
        Ok(())
    }

    fn push_audio(&mut self, block: &AudioBlock) -> Result<()> {
        let s = self.session_mut("push_audio")?;
        let track = s
            .audio
            .as_mut()
            .ok_or_else(|| MmError::InvalidArgument("設定に音が無いのに push_audio".into()))?;
        track.push(&mut s.output, block)?;
        s.audio_frames += block.frames() as u64;
        Ok(())
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        let mut s = match std::mem::replace(&mut self.state, State::Idle) {
            State::Running(s) => s,
            State::Analysis(_) => {
                return Err(MmError::InvalidArgument(
                    "2 パスの 2 回目（begin_pass(2)）の前に finish".into(),
                ))
            }
            State::Idle => return Err(MmError::InvalidArgument("begin の前に finish".into())),
        };
        s.video.finish(Some(&mut s.output))?;
        if let Some(audio) = s.audio.as_mut() {
            audio.finish(&mut s.output)?;
        }
        s.output
            .write_trailer()
            .map_err(|e| MmError::Internal(format!("トレーラーの書き込み: {e}")))?;
        let Session {
            output,
            settings,
            frames,
            audio_frames,
            ..
        } = *s;
        drop(output);
        let bytes_written = std::fs::metadata(&settings.output_path)?.len();
        Ok(EncodeReport {
            frames,
            audio_frames,
            bytes_written,
        })
    }
}

/// 2 パスの 1 回目の NV12。NV12 を受けないコーデックなら対応の有無の問い合わせに `Unsupported` を返す。
fn push_nv12_analysis(a: &mut Analysis, frames: &[Nv12Frame]) -> Result<()> {
    if !a.video.accepts_nv12() {
        return Err(MmError::Unsupported(format!(
            "{} は NV12 を受けない",
            a.settings.codec
        )));
    }
    for frame in frames {
        a.video.push_nv12(None, frame)?;
    }
    Ok(())
}

/// エンコーダーから出たパケットを全部出力に書く。`output` が無ければ捨てる（2 パスの 1 回目）。
/// `duration` はエンコーダーの時間の単位での 1 パケットの長さで、0 以外なら上書きする
/// （libx264 は入れず、最後のフレームの長さが muxer に伝わらないため）。
pub(crate) fn drain_packets(
    encoder: &mut codec::encoder::Encoder,
    mut output: Option<&mut Output>,
    stream_index: usize,
    from: Rational,
    to: Rational,
    duration: i64,
) -> Result<()> {
    let mut packet = Packet::empty();
    loop {
        match encoder.receive_packet(&mut packet) {
            Ok(()) => {
                let Some(output) = output.as_deref_mut() else {
                    continue;
                };
                packet.set_stream(stream_index);
                if duration > 0 {
                    packet.set_duration(duration);
                }
                packet.rescale_ts(from, to);
                packet
                    .write_interleaved(output)
                    .map_err(|e| MmError::Internal(format!("パケットの書き込み: {e}")))?;
            }
            Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => return Ok(()),
            Err(ff::Error::Eof) => return Ok(()),
            Err(e) => return Err(MmError::Internal(format!("エンコード: {e}"))),
        }
    }
}
