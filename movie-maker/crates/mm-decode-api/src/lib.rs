//! デコーダーとエンコーダーの口。
//!
//! コマは CPU の [`RgbaImage`] か、GPU のテクスチャ（[`GpuFrameHandle`]）で受け渡す。
//! GPU のコマの中身（wgpu のテクスチャ）は口に出さず、[`GpuFrame`] の型を消した値として運ぶ。
//! 描画器が `GpuHelper::import` で受け、`as_any` で実装の共有部（`mm-gpu`）の型に戻す。
//!
//! GPU のコマの寿命（RFC 0001）:
//! - `FrameSource::frame_at` が返す GPU のコマは、描画器の次の `submit` が終わるまでの借り物。
//!   描画器はそれより長く持たず、キャッシュに置くなら中間形式（`Rgba16Float`）に写す。
//! - デコーダーは、復号の完了を待たせる手続きをコマに付ける。描画器は `submit` の前にそれを呼ぶ。
//! - デコーダーが GPU で復号するには、描画器と同じデバイスが要る。`DecoderFactory::open` に
//!   [`GpuContextRef`] を渡し、`None` なら CPU のコマを返す。
//!
//! 契約（`mm_testkit::decode::contract`）:
//! - `info()` の `duration` と `fps` から数えたコマ数だけ `next_frame` が出て、その後 `None`。
//! - `seek(t)` の直後の `next_frame` は `index == t.frame_index(fps)`、`pts` はそのコマの時刻。
//! - 同じ時刻にシークし直すと同じコマが出る（bit 一致）。
//! - `decode_range(r)` は `info().frame_range(r)` と同じ番号の列を返す。CFR は
//!   `r.frame_indices(fps)`、VFR は PTS が `[start, end)` に入るコマ全部（最後のコマも落とさない）。
//! - `frame_index_at(t)` は最初のコマより前の `t` で CFR も VFR も -1。
//! - 壊れたファイルは `Err(Corrupt)` を返し、panic しない。
//! - エンコーダーは `begin` → `push_frames`/`push_audio` → `finish` の順で、報告のコマ数が押した数と一致する。

use std::any::Any;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use mm_audio_api::AudioBlock;
use mm_types::{
    ColorSpace, GpuAdapterId, Rational, RationalTime, Result, RgbaImage, Size, TimeRange,
};

/// 開いた素材の情報。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub duration: RationalTime,
    /// 固定フレームレートなら fps。VFR は `None` で、`pts_index` に PTS の列を持つ。
    pub fps: Option<Rational>,
    pub size: Size,
    pub color_space: ColorSpace,
    /// 音と映像の頭のずれ。映像の最初の PTS − 音の最初の PTS。
    pub timebase_offset: RationalTime,
    pub audio: Option<AudioStreamInfo>,
    /// VFR のとき、各コマの PTS（昇順）。CFR では空。
    #[serde(default)]
    pub pts_index: Vec<RationalTime>,
}

impl MediaInfo {
    /// `t` に映っているコマの番号。CFR は `frame_index` の規則、VFR は PTS の索引で `t` 以下の最後。
    /// 最初のコマより前の `t` は、CFR も VFR も -1。
    pub fn frame_index_at(&self, t: RationalTime) -> i64 {
        match self.fps {
            Some(fps) => t.frame_index(fps),
            None => self.pts_index.partition_point(|pts| *pts <= t) as i64 - 1,
        }
    }

    /// コマの数。
    pub fn frame_count(&self) -> i64 {
        match self.fps {
            Some(fps) => self.duration.frame_index(fps),
            None => self.pts_index.len() as i64,
        }
    }

    /// 区間 `[start, end)` に含まれるコマの番号。`decode_range` が返す番号の列。
    /// CFR は `TimeRange::frame_indices`、VFR は PTS が区間に入るコマ
    /// （`end` より前の PTS の数 − `start` より前の PTS の数）。
    pub fn frame_range(&self, range: TimeRange) -> std::ops::Range<i64> {
        match self.fps {
            Some(fps) => range.frame_indices(fps),
            None => {
                let before =
                    |t: RationalTime| self.pts_index.partition_point(|pts| *pts < t) as i64;
                before(range.start)..before(range.end)
            }
        }
    }
}

/// 音のストリームの情報。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

/// GPU の文脈への口。中身は実装の共有部（`mm-gpu`）の型で、`as_any` で降りる。
pub trait GpuContextRef: Send + Sync {
    /// 使っているアダプター。
    fn adapter(&self) -> &GpuAdapterId;
    /// 実装の型に降りる口。
    fn as_any(&self) -> &dyn Any;
}

/// GPU の上にあるコマの中身。wgpu の型は口に出さず、実装の共有部の型を `as_any` で戻す。
/// 復号の完了を待たせる手続きも中身が持つ（描画器が `submit` の前に呼ぶ）。
pub trait GpuFrame: Send + Sync {
    /// 実装の型に降りる口。
    fn as_any(&self) -> &dyn Any;
}

/// GPU の上にあるコマへの参照。`id` はデコーダーの中で一意。
#[derive(Clone)]
pub struct GpuFrameHandle {
    pub id: u64,
    pub size: Size,
    /// 型を消した中身。持っている間は復号器のプールを 1 つ占める。
    pub frame: Arc<dyn GpuFrame>,
}

impl fmt::Debug for GpuFrameHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuFrameHandle")
            .field("id", &self.id)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl PartialEq for GpuFrameHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.size == other.size && Arc::ptr_eq(&self.frame, &other.frame)
    }
}

/// 復号した 1 コマ。
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    pub index: i64,
    pub pts: RationalTime,
    /// CPU の絵。GPU で渡すときは `None`。
    pub image: Option<RgbaImage>,
    /// GPU のテクスチャ。CPU で渡すときは `None`。
    pub gpu: Option<GpuFrameHandle>,
}

impl DecodedFrame {
    /// CPU の絵として。
    pub fn cpu(index: i64, pts: RationalTime, image: RgbaImage) -> Self {
        Self {
            index,
            pts,
            image: Some(image),
            gpu: None,
        }
    }
}

/// デコーダー。素材 1 本を開いた状態。プールで使い回す。
pub trait Decoder: Send {
    /// 素材の情報。
    fn info(&self) -> &MediaInfo;
    /// `t` のコマが次に出るように位置を動かす。
    fn seek(&mut self, t: RationalTime) -> Result<()>;
    /// 次のコマ。終わりなら `None`。
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>>;
    /// 区間のコマをまとめて返す（毎コマの口の呼び出しを避ける）。番号は `MediaInfo::frame_range`
    /// と同じ（VFR は PTS が区間に入るコマ全部）。素材の終わりで足りなければそこまで。
    fn decode_range(&mut self, range: TimeRange) -> Result<Vec<DecodedFrame>> {
        let wanted = self.info().frame_range(range);
        self.seek(range.start)?;
        let mut out = Vec::new();
        while let Some(f) = self.next_frame()? {
            if f.index >= wanted.end {
                break;
            }
            // 区間の頭が PTS の途中だと、シーク先は頭より前のコマ。区間の外なので捨てる。
            if f.index >= wanted.start {
                out.push(f);
            }
        }
        Ok(out)
    }
    /// 区間の音。音が無ければ無音。
    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock>;
}

/// デコーダーを開く口。`mm-runtime` が設定で選ぶ。
pub trait DecoderFactory: Send + Sync {
    /// 実装の名前（`ffmpeg`、`d3d12va`、`cuvid`、見本は `sample`）。
    fn id(&self) -> &'static str;
    /// 素材を開く。壊れていれば `Err(Corrupt)`。`gpu` が `Some` なら、同じデバイスの上の
    /// GPU のコマを返してよい。`None` か GPU の復号に向かない素材では CPU のコマを返す。
    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>>;
}

/// 時刻で引けるコマの供給元。`FrameCx` が動画オブジェクトに渡す。描画のスレッドに移すので `Send`。
pub trait FrameSource: Send {
    /// 素材の情報。
    fn info(&self) -> &MediaInfo;
    /// 素材の時間軸の `t` のコマ。範囲の外は `Err(NotFound)`。
    fn frame_at(&mut self, t: RationalTime) -> Result<DecodedFrame>;
    /// 素材の時間軸の区間の音。
    fn audio_in(&mut self, range: TimeRange) -> Result<AudioBlock>;
}

/// エンコードの設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodeSettings {
    pub size: Size,
    pub fps: Rational,
    /// エンコーダーの名前（`h264_nvenc`、`prores_ks`、見本は `sample`）。
    pub codec: String,
    pub output_path: PathBuf,
    pub audio: Option<AudioStreamInfo>,
}

/// 書き終えたときの報告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodeReport {
    pub frames: u64,
    pub audio_frames: u64,
    pub bytes_written: u64,
}

/// エンコーダー。コマはバッチで押す。
pub trait Encoder: Send {
    /// 出力を開く。
    fn begin(&mut self, settings: &EncodeSettings) -> Result<()>;
    /// コマの列を押す。順序は再生順。
    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()>;
    /// 音を押す。
    fn push_audio(&mut self, block: &AudioBlock) -> Result<()>;
    /// 閉じて報告を返す。
    fn finish(&mut self) -> Result<EncodeReport>;
}

/// エンコーダーを作る口。
pub trait EncoderFactory: Send + Sync {
    /// 実装の名前。
    fn id(&self) -> &'static str;
    /// この名前のコーデックを扱えるか。
    fn supports(&self, codec: &str) -> bool;
    /// エンコーダーを作る。
    fn create(&self) -> Result<Box<dyn Encoder>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vfr_は_pts_の索引で引く() {
        let info = MediaInfo {
            duration: RationalTime::seconds(1),
            fps: None,
            size: Size::new(1, 1),
            color_space: ColorSpace::bt709(),
            timebase_offset: RationalTime::ZERO,
            audio: None,
            pts_index: vec![
                RationalTime::ZERO,
                RationalTime::new(1, 2).unwrap(),
                RationalTime::new(3, 4).unwrap(),
            ],
        };
        assert_eq!(info.frame_index_at(RationalTime::new(1, 4).unwrap()), 0);
        assert_eq!(info.frame_index_at(RationalTime::new(1, 2).unwrap()), 1);
        assert_eq!(info.frame_index_at(RationalTime::seconds(1)), 2);
        assert_eq!(info.frame_count(), 3);
    }

    fn vfr_info() -> MediaInfo {
        MediaInfo {
            duration: RationalTime::seconds(1),
            fps: None,
            size: Size::new(1, 1),
            color_space: ColorSpace::bt709(),
            timebase_offset: RationalTime::ZERO,
            audio: None,
            pts_index: vec![
                RationalTime::ZERO,
                RationalTime::new(1, 2).unwrap(),
                RationalTime::new(3, 4).unwrap(),
            ],
        }
    }

    fn range(a: (i128, u32), b: (i128, u32)) -> TimeRange {
        TimeRange::new(
            RationalTime::new(a.0, a.1).unwrap(),
            RationalTime::new(b.0, b.1).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn vfr_の区間は最後のコマも含む() {
        let info = vfr_info();
        assert_eq!(info.frame_range(range((0, 1), (1, 1))), 0..3);
        assert_eq!(
            info.frame_range(range((0, 1), (3, 4))),
            0..2,
            "end ちょうどの PTS は含まない"
        );
        assert_eq!(info.frame_range(range((1, 2), (1, 1))), 1..3);
        assert_eq!(
            info.frame_range(range((1, 4), (1, 1))),
            1..3,
            "頭が PTS の途中なら次のコマから"
        );
    }

    #[test]
    fn vfr_の最初のコマより前は_cfr_と同じ_マイナス_1() {
        let info = vfr_info();
        assert_eq!(info.frame_index_at(RationalTime::new(-1, 10).unwrap()), -1);
        let cfr = MediaInfo {
            fps: Some(Rational::integer(30)),
            pts_index: Vec::new(),
            ..vfr_info()
        };
        assert_eq!(cfr.frame_index_at(RationalTime::new(-1, 100).unwrap()), -1);
    }

    #[test]
    fn cfr_の区間は_frame_indices_と同じ() {
        let fps = Rational::integer(30);
        let info = MediaInfo {
            fps: Some(fps),
            pts_index: Vec::new(),
            ..vfr_info()
        };
        let r = range((0, 1), (1, 6));
        assert_eq!(info.frame_range(r), r.frame_indices(fps));
    }
}
