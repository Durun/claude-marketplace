//! デコーダーとエンコーダーのインターフェース。
//!
//! フレームは CPU の [`RgbaImage`] か、GPU のテクスチャ（[`GpuFrameHandle`]）で受け渡す。
//! GPU のフレームの中身（wgpu のテクスチャ）はインターフェースに出さず、[`GpuFrame`] の型を消した値として運ぶ。
//! レンダラーが `GpuHelper::import` で受け、`as_any` で共通実装（`mm-gpu`）の型に戻す。
//!
//! GPU のフレームの寿命（RFC 0001）:
//! - `FrameSource::frame_at` が返す GPU のフレームは、レンダラーの次の `submit` が終わるまでの借用したもの。
//!   レンダラーはそれより長く持たず、キャッシュに置くなら中間形式（`Rgba16Float`）にコピーする。
//! - デコーダーは、デコードの完了を待たせる手続きをフレームに付ける。レンダラーは `submit` の前にそれを呼ぶ。
//! - デコーダーが GPU でデコードするには、レンダラーと同じデバイスが要る。`DecoderFactory::open` に
//!   [`GpuContextRef`] を渡し、`None` なら CPU のフレームを返す。
//!
//! 契約（`mm_testkit::decode::contract`）:
//! - `info()` の `duration` と `fps` から数えたフレーム数だけ `next_frame` が出て、その後 `None`。
//! - `seek(t)` の直後の `next_frame` は `index == t.frame_index(fps)`、`pts` はそのフレームの時刻。
//! - 同じ時刻にシークし直すと同じフレームが出る（bit 一致）。
//! - `decode_range(r)` は `info().frame_range(r)` と同じ番号の列を返す。CFR は
//!   `r.frame_indices(fps)`、VFR は PTS が `[start, end)` に入るフレーム全部（最後のフレームも取りこぼさない）。
//! - `frame_index_at(t)` は最初のフレームより前の `t` で CFR も VFR も -1。
//! - 壊れたファイルは `Err(Corrupt)` を返し、panic しない。
//! - エンコーダーは `begin` → `push_frames`/`push_audio` → `finish` の順で、報告のフレーム数が押した数と一致する。

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
    /// 音と映像の先頭のずれ。映像の最初の PTS − 音の最初の PTS。
    pub timebase_offset: RationalTime,
    pub audio: Option<AudioStreamInfo>,
    /// VFR のとき、各フレームの PTS（昇順）。CFR では空。
    #[serde(default)]
    pub pts_index: Vec<RationalTime>,
}

impl MediaInfo {
    /// `t` に映っているフレームの番号。CFR は `frame_index` の規則、VFR は PTS の索引で `t` 以下の最後。
    /// 最初のフレームより前の `t` は、CFR も VFR も -1。
    pub fn frame_index_at(&self, t: RationalTime) -> i64 {
        match self.fps {
            Some(fps) => t.frame_index(fps),
            None => self.pts_index.partition_point(|pts| *pts <= t) as i64 - 1,
        }
    }

    /// フレームの数。
    pub fn frame_count(&self) -> i64 {
        match self.fps {
            Some(fps) => self.duration.frame_index(fps),
            None => self.pts_index.len() as i64,
        }
    }

    /// 区間 `[start, end)` に含まれるフレームの番号。`decode_range` が返す番号の列。
    /// CFR は `TimeRange::frame_indices`、VFR は PTS が区間に入るフレーム
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

/// GPU のコンテキストを運ぶインターフェース。中身は共通実装（`mm-gpu`）の型で、`as_any` で降りる。
pub trait GpuContextRef: Send + Sync {
    /// 使っているアダプター。
    fn adapter(&self) -> &GpuAdapterId;
    /// 実装の型に降りる関数。
    fn as_any(&self) -> &dyn Any;
}

/// GPU の上にあるフレームの中身。wgpu の型はインターフェースに出さず、共通実装の型を `as_any` で戻す。
/// デコードの完了を待たせる手続きも中身が持つ（レンダラーが `submit` の前に呼ぶ）。
pub trait GpuFrame: Send + Sync {
    /// 実装の型に降りる関数。
    fn as_any(&self) -> &dyn Any;
}

/// GPU の上にあるフレームへの参照。`id` はデコーダーの中で一意。
#[derive(Clone)]
pub struct GpuFrameHandle {
    pub id: u64,
    pub size: Size,
    /// 型を消した中身。持っている間はデコーダーのプールを 1 つ占める。
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

/// デコードした 1 フレーム。
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    pub index: i64,
    pub pts: RationalTime,
    /// CPU の画像。GPU で渡すときは `None`。
    pub image: Option<RgbaImage>,
    /// GPU のテクスチャ。CPU で渡すときは `None`。
    pub gpu: Option<GpuFrameHandle>,
}

impl DecodedFrame {
    /// CPU の画像として。
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
    /// `t` のフレームが次に出るように位置を動かす。
    fn seek(&mut self, t: RationalTime) -> Result<()>;
    /// 次のフレーム。終わりなら `None`。
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>>;
    /// 区間のフレームをまとめて返す（毎フレームのインターフェース越しの呼び出しを避ける）。番号は `MediaInfo::frame_range`
    /// と同じ（VFR は PTS が区間に入るフレーム全部）。素材の終わりで足りなければそこまで。
    fn decode_range(&mut self, range: TimeRange) -> Result<Vec<DecodedFrame>> {
        let wanted = self.info().frame_range(range);
        self.seek(range.start)?;
        let mut out = Vec::new();
        while let Some(f) = self.next_frame()? {
            if f.index >= wanted.end {
                break;
            }
            // 区間の先頭が PTS の途中だと、シーク先は先頭より前のフレーム。区間の外なので破棄する。
            if f.index >= wanted.start {
                out.push(f);
            }
        }
        Ok(out)
    }
    /// 区間の音。音が無ければ無音。
    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock>;
    /// デコードしている実装の名前（`ffmpeg`、`d3d12va`、`cuvid`）。実装を選ぶファクトリー（`auto`）が開いた
    /// デコーダーでも、実際に選ばれた実装を返す。画素は実装で変わりうるので、フレームのキャッシュのキーに入る。
    /// `None` は、開いたファクトリーの名前で足りる（モックなど）。
    fn backend_id(&self) -> Option<&'static str> {
        None
    }
}

/// デコーダーを開くインターフェース。`mm-runtime` が設定で選ぶ。
pub trait DecoderFactory: Send + Sync {
    /// 実装の名前（`ffmpeg`、`d3d12va`、`cuvid`、モックは `sample`）。
    fn id(&self) -> &'static str;
    /// 素材を開く。壊れていれば `Err(Corrupt)`。`gpu` が `Some` なら、同じデバイスの上の
    /// GPU のフレームを返してよい。`None` か GPU のデコードに向かない素材では CPU のフレームを返す。
    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>>;
}

/// 時刻で引けるフレームの供給元。`FrameCx` がノードに渡す。描画のスレッドに移すので `Send`。
pub trait FrameSource: Send {
    /// 素材の情報。
    fn info(&self) -> &MediaInfo;
    /// 素材の時間軸の `t` のフレーム。範囲の外は `Err(NotFound)`。
    fn frame_at(&mut self, t: RationalTime) -> Result<DecodedFrame>;
    /// 素材の時間軸の区間の音。
    fn audio_in(&mut self, range: TimeRange) -> Result<AudioBlock>;
}

/// エンコードの設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodeSettings {
    pub size: Size,
    pub fps: Rational,
    /// エンコーダーの名前（`h264_nvenc`、`prores_ks`、モックは `sample`）。
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

/// NV12 の 1 フレーム（Y のプレーンと、横縦 1/2 の UV のプレーン）。Rec.709 の行列、tv レンジ、
/// Rec.1886 の符号化値。レンダラーが GPU で YUV にしてリードバックしたフレームを、そのままエンコーダーへ渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv12Frame {
    pub size: Size,
    /// 幅 x 高さ バイト。行の余白は無い。
    pub y: Vec<u8>,
    /// 幅 x 高さ / 2 バイト（UV が交互、行は高さ / 2）。行の余白は無い。
    pub uv: Vec<u8>,
}

impl Nv12Frame {
    /// プレーンの長さを確かめて作る。大きさは偶数が要る。
    pub fn new(size: Size, y: Vec<u8>, uv: Vec<u8>) -> Result<Self> {
        let (w, h) = (size.width as usize, size.height as usize);
        if w == 0 || h == 0 || w % 2 == 1 || h % 2 == 1 {
            return Err(mm_types::MmError::InvalidArgument(format!(
                "NV12 は偶数の大きさが要る: {w}x{h}"
            )));
        }
        if y.len() != w * h || uv.len() != w * h / 2 {
            return Err(mm_types::MmError::InvalidArgument(format!(
                "NV12 のプレーンの長さが合わない: Y {} UV {}（{w}x{h}）",
                y.len(),
                uv.len()
            )));
        }
        Ok(Self { size, y, uv })
    }
}

/// エンコーダー。フレームはバッチで押す。
pub trait Encoder: Send {
    /// 出力を開く。
    fn begin(&mut self, settings: &EncodeSettings) -> Result<()>;
    /// フレームの列を押す。順序は再生順。
    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()>;
    /// GPU で YUV にしたフレームの列を押す。順序は再生順。対応しない実装は `Err(Unsupported)` を返し、
    /// 呼ぶ側は `push_frames` に切り替える。空の列は何もせず、対応の有無だけを返す（切り替えの判断に使う）。
    fn push_nv12_frames(&mut self, frames: &[Nv12Frame]) -> Result<()> {
        let _ = frames;
        Err(mm_types::MmError::Unsupported(
            "このエンコーダーは NV12 を受けない".into(),
        ))
    }
    /// 音を押す。
    fn push_audio(&mut self, block: &AudioBlock) -> Result<()>;
    /// 閉じて報告を返す。
    fn finish(&mut self) -> Result<EncodeReport>;
}

/// エンコーダーを作るインターフェース。
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
    fn vfr_の区間は最後のフレームも含む() {
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
            "先頭が PTS の途中なら次のフレームから"
        );
    }

    #[test]
    fn vfr_の最初のフレームより前は_cfr_と同じ_マイナス_1() {
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
