//! 可変フレームレート（VFR）の見本のデコーダーと、`decode_range` の契約。
//!
//! 実装のクレートは VFR の素材を開く `make` を渡して [`vfr_contract`] を呼ぶ。

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, MediaInfo};
use mm_types::{ColorSpace, RationalTime, Result, Rgba, RgbaImage, Size, TimeRange};

use crate::decode::frame_color;

/// PTS の列を持つ見本のデコーダー。フレームの番号を G に焼く（`decode::frame_color`）。
#[derive(Debug)]
pub struct SampleVfrDecoder {
    info: MediaInfo,
    cursor: i64,
}

impl SampleVfrDecoder {
    /// `pts`（昇順）のフレームを持つ、長さ `duration` の素材。
    pub fn new(pts: Vec<RationalTime>, duration: RationalTime) -> Self {
        Self {
            info: MediaInfo {
                duration,
                fps: None,
                size: Size::new(2, 2),
                color_space: ColorSpace::bt709(),
                timebase_offset: RationalTime::ZERO,
                audio: None,
                pts_index: pts,
            },
            cursor: 0,
        }
    }

    /// 契約の既定の素材: PTS が 0、1/2、3/4 秒の 3 フレームで長さ 1 秒。
    pub fn sample() -> Self {
        let half = RationalTime::new(1, 2).expect("表せる");
        let three_quarters = RationalTime::new(3, 4).expect("表せる");
        Self::new(
            vec![RationalTime::ZERO, half, three_quarters],
            RationalTime::seconds(1),
        )
    }
}

impl Decoder for SampleVfrDecoder {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.cursor = self.info.frame_index_at(t);
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let Some(&pts) = usize::try_from(self.cursor)
            .ok()
            .and_then(|i| self.info.pts_index.get(i))
        else {
            return Ok(None);
        };
        let index = self.cursor;
        self.cursor += 1;
        let image = RgbaImage::filled(self.info.size, frame_color(Rgba::BLACK, index));
        Ok(Some(DecodedFrame::cpu(index, pts, image)))
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let rate = mm_audio_api::DEFAULT_SAMPLE_RATE;
        Ok(AudioBlock::silence(
            rate,
            mm_audio_api::DEFAULT_CHANNELS,
            mm_node_api::audio_frames_in(range, rate),
        ))
    }
}

fn indices(dec: &mut dyn Decoder, start: RationalTime, end: RationalTime) -> Vec<i64> {
    let range = TimeRange::new(start, end).expect("順序");
    dec.decode_range(range)
        .expect("区間")
        .iter()
        .map(|f| f.index)
        .collect()
}

/// VFR の `decode_range` の契約。`make` は PTS が 3 つ以上ある VFR の素材を開く。
/// 区間に PTS が入るフレームを全部、番号の順に返し、最後のフレームを落とさない。
pub fn vfr_contract(make: impl Fn() -> Box<dyn Decoder>) {
    let mut dec = make();
    let info = dec.info().clone();
    let pts = &info.pts_index;
    assert!(info.fps.is_none() && pts.len() >= 3, "契約の素材は VFR");
    let n = pts.len() as i64;
    assert_eq!(info.frame_count(), n, "VFR のフレーム数は PTS の数");
    assert_eq!(
        info.frame_index_at(RationalTime::new(-1, 10).expect("表せる")),
        -1,
        "最初のフレームより前は CFR と同じ -1"
    );

    let all = indices(dec.as_mut(), RationalTime::ZERO, info.duration);
    assert_eq!(all, (0..n).collect::<Vec<_>>(), "全体なら最後のフレームまで");

    let last = pts[pts.len() - 1];
    let head = indices(dec.as_mut(), RationalTime::ZERO, last);
    assert_eq!(
        head,
        (0..n - 1).collect::<Vec<_>>(),
        "end ちょうどの PTS は含まない"
    );

    let tail = indices(dec.as_mut(), pts[1], info.duration);
    assert_eq!(
        tail,
        (1..n).collect::<Vec<_>>(),
        "start ちょうどの PTS は含む"
    );

    let mid = pts[0]
        .checked_add(pts[1])
        .expect("表せる")
        .div_by(mm_types::Rational::integer(2))
        .expect("表せる");
    let from_mid = indices(dec.as_mut(), mid, info.duration);
    assert_eq!(
        from_mid,
        (1..n).collect::<Vec<_>>(),
        "頭が PTS の途中なら次のフレームから"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 見本の_vfr_デコーダーは契約を満たす() {
        vfr_contract(|| Box::new(SampleVfrDecoder::sample()));
    }
}
