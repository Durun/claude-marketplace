//! 映像と音のほかに MP4 へ入れるもの: 字幕のトラック（mov_text）とチャプター。

use ff::codec::Parameters;
use ff::format::context::Output;
use ff::{media, Packet, Rational};
use ffmpeg_next as ff;
use mm_decode_api::{ChapterMark, ExtraTracks, SubtitleCue};
use mm_types::{MmError, RationalTime, Result};

/// 字幕とチャプターの時間の単位。mov_text は 1 ms で足りる。
const MS_BASE: Rational = Rational(1, 1000);
/// tx3g のサンプルの文字数の先頭につく長さ（バイト）。
const TX3G_LEN_BYTES: usize = 2;

/// 秒を ms に丸める。
fn ms(t: RationalTime) -> i64 {
    (t.to_f64() * 1000.0).round() as i64
}

/// ヘッダーを書く前に、チャプターを足し、字幕のストリームを開く。字幕が無ければ `None`。
pub fn open(output: &mut Output, extras: &ExtraTracks) -> Result<Option<SubtitleTrack>> {
    for (i, c) in extras.chapters.iter().enumerate() {
        add_chapter(output, i as i64, c)?;
    }
    if extras.subtitles.is_empty() {
        return Ok(None);
    }
    let mut params = Parameters::new();
    params.set_medium(media::Type::Subtitle);
    params.set_id(ff::codec::Id::MOV_TEXT);
    let mut stream = output
        .add_stream(ff::encoder::find(ff::codec::Id::MOV_TEXT))
        .map_err(|e| MmError::Unsupported(format!("字幕のストリームを足せない: {e}")))?;
    stream.set_parameters(params);
    stream.set_time_base(MS_BASE);
    Ok(Some(SubtitleTrack {
        stream_index: stream.index(),
        stream_tb: MS_BASE,
        cues: extras.subtitles.clone(),
    }))
}

fn add_chapter(output: &mut Output, id: i64, c: &ChapterMark) -> Result<()> {
    output
        .add_chapter(id, MS_BASE, ms(c.start), ms(c.end), &c.title)
        .map(|_| ())
        .map_err(|e| MmError::Internal(format!("チャプターの書き込み: {e}")))
}

/// 字幕のストリームと、書くサンプル。
pub struct SubtitleTrack {
    stream_index: usize,
    stream_tb: Rational,
    cues: Vec<SubtitleCue>,
}

impl SubtitleTrack {
    /// ヘッダーを書いた後の、muxer が決めたストリームの時間の単位を控える。
    pub fn after_header(mut self, output: &Output) -> Self {
        if let Some(stream) = output.stream(self.stream_index) {
            self.stream_tb = stream.time_base();
        }
        self
    }

    /// 字幕を時刻の順に書く。字幕の間の空きは空のサンプルで埋める（mov_text は次のサンプルまで表示し続けるため）。
    pub fn write(&self, output: &mut Output) -> Result<()> {
        let mut head = 0i64;
        for cue in &self.cues {
            let (start, end) = (ms(cue.start), ms(cue.end));
            if start > head {
                self.write_sample(output, head, start - head, "")?;
            }
            let start = start.max(head);
            if end > start {
                self.write_sample(output, start, end - start, &cue.text)?;
                head = end;
            }
        }
        Ok(())
    }

    fn write_sample(&self, output: &mut Output, start: i64, len: i64, text: &str) -> Result<()> {
        let bytes = text.as_bytes();
        let size = u16::try_from(bytes.len())
            .map_err(|_| MmError::InvalidArgument("字幕の 1 つが長すぎる".into()))?;
        let mut data = Vec::with_capacity(TX3G_LEN_BYTES + bytes.len());
        data.extend_from_slice(&size.to_be_bytes());
        data.extend_from_slice(bytes);
        let mut packet = Packet::copy(&data);
        packet.set_stream(self.stream_index);
        packet.set_pts(Some(start));
        packet.set_dts(Some(start));
        packet.set_duration(len);
        packet.rescale_ts(MS_BASE, self.stream_tb);
        packet
            .write_interleaved(output)
            .map_err(|e| MmError::Internal(format!("字幕の書き込み: {e}")))
    }
}
