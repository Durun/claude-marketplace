//! 並行の書き出し。区間を短いチャンクに分け、複数のレンダラー（`Engine` の組み立てをワーカーごとに 1 つずつ）で
//! 描き、受け取りの側がフレームの順にエンコーダーへ送る（`06-engine.md` の「書き出し」）。
//!
//! 音は並行にしない。呼び出し側が区間の全体を 1 回で混ぜるので、チャンクの境で音は切れない。
//! 映像はチャンクの番号順にしか送らないので、順序と数は 1 つのレンダラーで描いたときと同じ。

use std::ops::Range;
use std::time::{Duration, Instant};

use mm_decode_api::{EncodeReport, EncodeSettings, Encoder, Nv12Frame};
use mm_doc::Project;
use mm_node_api::Quality;
use mm_render_api::RangeRequest;
use mm_runtime::{assemble, Engine, RuntimeConfig};
use mm_types::{CompositionId, MmError, RationalTime, Result, RgbaImage, Size, TimeRange};

use crate::ordered::run_ordered;

/// 並行の描画が溜めておく映像のバイト数の目安。チャンクの長さをこれで抑える。
const MEMORY_BUDGET_BYTES: u64 = 2 << 30;
/// 1 チャンクのフレーム数の上限と下限。上限はシークの回数、下限はワーカーの起動の割合を抑える。
const MAX_CHUNK_FRAMES: u64 = 30;
const MIN_CHUNK_FRAMES: u64 = 2;
/// ワーカーのレンダラーが VRAM に持つフレームのキャッシュの上限。ワーカーの数だけ増えるので小さくする。
const WORKER_FRAME_MEMORY_BYTES: u64 = 256 << 20;
/// 並行にするのは、ワーカー 1 本あたり 1 チャンク以上あるときだけ（それ未満ではワーカーが遊ぶ）。
/// ワーカーのエンジンを組む時間（1080p で約 1 秒、初回は数秒）は、`parallel` を指定した呼び出し側が引き受ける。
/// エンコーダーが 1 セッションで頭打ちになる NVENC の 1080p では、1 分未満の書き出しは並行にしないほうが速い。
const MIN_CHUNKS_PER_WORKER: u64 = 1;
/// 1 バイトあたりの画素の数。NV12 は 1.5 バイト、リードバックした RGBA の f32 は 16 バイト。
const NV12_BYTES_PER_PIXEL: f64 = 1.5;
const RGBA_BYTES_PER_PIXEL: f64 = 16.0;

/// 並行にする最短のフレーム数の既定。
pub(crate) fn default_min_frames(workers: usize) -> u64 {
    workers as u64 * MAX_CHUNK_FRAMES * MIN_CHUNKS_PER_WORKER
}

/// `workers` 本で並行に描く価値がある長さか。`workers` が 1 以下なら並行にしない。
/// `min_frames` が `None` なら既定の長さ以上のとき。
pub(crate) fn worth_it(workers: usize, frames: u64, min_frames: Option<u64>) -> bool {
    workers > 1 && frames >= min_frames.unwrap_or_else(|| default_min_frames(workers))
}

/// 並行で描く区間。
pub(crate) struct ParallelJob<'a> {
    pub project: &'a Project,
    pub composition: &'a CompositionId,
    pub range: TimeRange,
    pub settings: &'a EncodeSettings,
    pub workers: usize,
    /// ワーカーのエンジンを組む設定。
    pub config: RuntimeConfig,
    /// 1 チャンクのフレーム数。`None` は大きさとワーカーの数から決める（テストが短い区間で分割を試すためだけに指定する）。
    pub chunk_frames: Option<u64>,
}

/// 1 チャンクの描いたフレーム。
enum Chunk {
    Nv12(Vec<Nv12Frame>),
    Rgba(Vec<RgbaImage>),
}

/// チャンクを描いている間だけエンコーダーのふりをして、送られたフレームを溜める。
struct Collect {
    chunk: Chunk,
}

impl Encoder for Collect {
    fn begin(&mut self, _settings: &EncodeSettings) -> Result<()> {
        Ok(())
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        match &mut self.chunk {
            Chunk::Rgba(v) => v.extend_from_slice(frames),
            Chunk::Nv12(_) => return Err(MmError::Internal("NV12 のチャンクに RGBA".into())),
        }
        Ok(())
    }

    fn push_nv12_frames(&mut self, frames: &[Nv12Frame]) -> Result<()> {
        match &mut self.chunk {
            Chunk::Nv12(v) => v.extend_from_slice(frames),
            _ => {
                return Err(MmError::Unsupported(
                    "このチャンクは NV12 を受けない".into(),
                ))
            }
        }
        Ok(())
    }

    fn push_audio(&mut self, _block: &mm_audio_api::AudioBlock) -> Result<()> {
        Err(MmError::Internal("チャンクに音は送らない".into()))
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        Err(MmError::Internal("チャンクは閉じない".into()))
    }
}

/// 1 チャンクのフレーム数。溜まるバイト数（先取りの幅と作っている途中の分）が予算に収まるようにする。
fn chunk_frames(size: Size, nv12: bool, workers: usize) -> u64 {
    let per_pixel = if nv12 {
        NV12_BYTES_PER_PIXEL
    } else {
        RGBA_BYTES_PER_PIXEL
    };
    let frame_bytes = (f64::from(size.width) * f64::from(size.height) * per_pixel).max(1.0) as u64;
    let held = (2 * workers as u64 + 1).max(1);
    (MEMORY_BUDGET_BYTES / frame_bytes / held).clamp(MIN_CHUNK_FRAMES, MAX_CHUNK_FRAMES)
}

/// フレームの番号の範囲をチャンクに分ける。
fn split(indices: Range<i64>, chunk: u64) -> Vec<Range<i64>> {
    let step = chunk.max(1) as i64;
    (indices.start..indices.end)
        .step_by(step as usize)
        .map(|start| start..(start + step).min(indices.end))
        .collect()
}

/// チャンクの時間の範囲。フレームの番号の端をそのまま時刻にするので、チャンクのフレームの番号は元の範囲とずれない。
fn chunk_range(frames: &Range<i64>, fps: mm_types::Rational) -> Result<TimeRange> {
    TimeRange::new(
        RationalTime::from_frames(frames.start, fps)?,
        RationalTime::from_frames(frames.end, fps)?,
    )
}

/// `sink` は送ったフレームごとに、その番号と、そのフレームの描画にかかった時間（チャンクの均等割り）で呼ぶ。
/// 送ったフレームの数を返す。
pub(crate) fn export_parallel(
    job: &ParallelJob<'_>,
    encoder: &mut dyn Encoder,
    sink: &mut dyn FnMut(i64, Duration),
) -> Result<usize> {
    let settings = job.settings;
    // 空のスライスで NV12 を受けるかを問い合わせる。受けなければチャンクも RGBA で運ぶ。
    let nv12 = encoder.push_nv12_frames(&[]).is_ok();
    let indices = job.range.frame_indices(settings.fps);
    let chunk = job
        .chunk_frames
        .unwrap_or_else(|| chunk_frames(settings.size, nv12, job.workers));
    let chunks = split(indices, chunk);
    let mut worker_config = job.config.clone();
    worker_config.frame_memory_bytes = Some(
        worker_config
            .frame_memory_bytes
            .map_or(WORKER_FRAME_MEMORY_BYTES, |b| {
                b.min(WORKER_FRAME_MEMORY_BYTES)
            }),
    );
    let mut pushed = 0usize;
    run_ordered(
        chunks.len(),
        job.workers,
        job.workers + 1,
        || assemble(&worker_config),
        |engine, index| render_chunk(engine, job, &chunks[index], nv12),
        |index, (chunk, elapsed)| {
            let frames = &chunks[index];
            let per_frame = elapsed / (frames.end - frames.start).max(1) as u32;
            push_chunk(encoder, chunk)?;
            for number in frames.clone() {
                sink(number, per_frame);
            }
            pushed += (frames.end - frames.start) as usize;
            Ok(())
        },
    )?;
    Ok(pushed)
}

/// エンジンのレンダラーでチャンクを描き、溜めたフレームと描画の時間を返す。
fn render_chunk(
    engine: &mut Engine,
    job: &ParallelJob<'_>,
    frames: &Range<i64>,
    nv12: bool,
) -> Result<(Chunk, Duration)> {
    let started = Instant::now();
    let registry = engine.registry().clone();
    let mut collect = Collect {
        chunk: if nv12 {
            Chunk::Nv12(Vec::new())
        } else {
            Chunk::Rgba(Vec::new())
        },
    };
    let want = (frames.end - frames.start) as usize;
    let count = engine.renderer().export_range(
        &RangeRequest {
            project: job.project,
            composition: job.composition,
            range: chunk_range(frames, job.settings.fps)?,
            fps: job.settings.fps,
            size: job.settings.size,
            quality: Quality::Full,
            nodes: &registry,
        },
        &mut collect,
        crate::EXPORT_BATCH_FRAMES,
        &mut |_| {},
    )?;
    if count != want {
        return Err(MmError::Internal(format!(
            "チャンク {}..{} で {count} フレームしか描けなかった（{want} 枚のはず）",
            frames.start, frames.end
        )));
    }
    Ok((collect.chunk, started.elapsed()))
}

/// 溜めたフレームを `EXPORT_BATCH_FRAMES` ずつエンコーダーに送る。
fn push_chunk(encoder: &mut dyn Encoder, chunk: Chunk) -> Result<()> {
    match chunk {
        Chunk::Nv12(frames) => frames
            .chunks(crate::EXPORT_BATCH_FRAMES)
            .try_for_each(|batch| encoder.push_nv12_frames(batch)),
        Chunk::Rgba(frames) => frames
            .chunks(crate::EXPORT_BATCH_FRAMES)
            .try_for_each(|batch| encoder.push_frames(batch)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 区間は端を欠かさず重ねずにチャンクに分かれる() {
        let parts = split(10..95, 30);
        assert_eq!(parts, vec![10..40, 40..70, 70..95]);
        assert_eq!(split(0..0, 30), Vec::<Range<i64>>::new());
        assert_eq!(split(5..6, 30), vec![5..6]);
    }

    #[test]
    fn 短い区間は並行にしない() {
        assert!(!worth_it(1, 100_000, None) && !worth_it(0, 100_000, None));
        assert!(!worth_it(4, 119, None) && worth_it(4, 120, None));
        assert!(worth_it(2, 10, Some(10)), "最短の長さは指定できる");
    }

    #[test]
    fn チャンクは大きいフレームほど短くなる() {
        let hd = chunk_frames(Size::new(1920, 1080), true, 4);
        let uhd = chunk_frames(Size::new(3840, 2160), true, 4);
        let uhd_rgba = chunk_frames(Size::new(3840, 2160), false, 4);
        assert_eq!(hd, MAX_CHUNK_FRAMES);
        assert!(uhd < hd && uhd_rgba < uhd);
        assert_eq!(uhd_rgba, MIN_CHUNK_FRAMES);
    }

    #[test]
    fn チャンクの時間の範囲はフレームの番号の端と一致する() {
        let fps = mm_types::Rational::new(30_000, 1001).unwrap();
        let r = chunk_range(&(59..89), fps).unwrap();
        assert_eq!(r.frame_indices(fps), 59..89);
    }
}
