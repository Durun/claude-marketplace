//! デコーダーとエンコーダーの口。
//!
//! 段階 0 のコマは CPU の [`RgbaImage`] で受け渡す。GPU のテクスチャ（d3d12va、cuvid）で
//! 渡す道は [`DecodedFrame::gpu`] の欄で足し、`image` を `None` にする（RFC で決める）。
//!
//! 契約（`mm_testkit::decode::contract`）:
//! - `info()` の `duration` と `fps` から数えたコマ数だけ `next_frame` が出て、その後 `None`。
//! - `seek(t)` の直後の `next_frame` は `index == t.frame_index(fps)`、`pts` はそのコマの時刻。
//! - 同じ時刻にシークし直すと同じコマが出る（bit 一致）。
//! - `decode_range(r)` は `r.frame_indices(fps)` と同じ番号の列を返す。
//! - 壊れたファイルは `Err(Corrupt)` を返し、panic しない。
//! - エンコーダーは `begin` → `push_frames`/`push_audio` → `finish` の順で、報告のコマ数が押した数と一致する。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use mm_audio_api::AudioBlock;
use mm_types::{ColorSpace, Rational, RationalTime, Result, RgbaImage, Size, TimeRange};

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
    /// `t` のコマの番号。CFR は `frame_index` の規則、VFR は PTS の索引で `t` 以下の最後。
    pub fn frame_index_at(&self, t: RationalTime) -> i64 {
        match self.fps {
            Some(fps) => t.frame_index(fps),
            None => self
                .pts_index
                .partition_point(|pts| *pts <= t)
                .saturating_sub(1) as i64,
        }
    }

    /// コマの数。
    pub fn frame_count(&self) -> i64 {
        match self.fps {
            Some(fps) => self.duration.frame_index(fps),
            None => self.pts_index.len() as i64,
        }
    }
}

/// 音のストリームの情報。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

/// GPU の上にあるコマへの参照。中身の解釈は実装の組（decode と render）で合わせる。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GpuFrameHandle {
    pub id: u64,
    pub size: Size,
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
    /// 区間のコマをまとめて返す（毎コマの口の呼び出しを避ける）。
    fn decode_range(&mut self, range: TimeRange) -> Result<Vec<DecodedFrame>> {
        let count = self
            .info()
            .frame_index_at(range.end)
            .saturating_sub(self.info().frame_index_at(range.start));
        self.seek(range.start)?;
        let mut out = Vec::with_capacity(count.max(0) as usize);
        for _ in 0..count {
            match self.next_frame()? {
                Some(f) => out.push(f),
                None => break,
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
    /// 素材を開く。壊れていれば `Err(Corrupt)`。
    fn open(&self, path: &Path) -> Result<Box<dyn Decoder>>;
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
}
