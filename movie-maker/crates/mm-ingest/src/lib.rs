//! 取り込みの流れ。素材 1 つから、コンテンツハッシュ、プレビュー動画、波形、サムネイルを作り、
//! プロジェクトに足す素材（[`mm_doc::Asset`]）を返す。
//!
//! - 作る処理（プレビュー動画、波形、サムネイル）は別のスレッドで並行に回す。それぞれ `ingest.preview`、
//!   `ingest.waveform`、`ingest.thumbnail` のスパンで、全体は `ingest.file`（ハッシュは `ingest.hash`、調べは `ingest.probe`）。
//! - 出力は `cache/` の下に、素材のコンテンツハッシュのフォルダーで置く（[`CacheLayout`]）。すでにあれば作らずに飛ばす。
//!   書きかけは `.part` に書いて完成してから置くので、止まった出力を完成品と取り違えない。
//! - 素材を足す操作の適用は呼ぶ側（`mm` の CLI）が持つ。このクレートは `mm-ops` と `mm-store` に依存しない。
//! - 文字起こしなどの付属ファイルは外の道具（`tools/`）が書く。ここは読んで検証する（[`sidecar`]）。

mod hash;
mod layout;
mod preview;
mod probe;
pub mod sidecar;
mod thumbnail;
mod tools;
mod waveform;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tracing::{info_span, Span};

use mm_doc::Asset;
use mm_types::{AssetId, Id, MmError, Result};

pub use hash::hash_file;
pub use layout::CacheLayout;
pub use preview::{PreviewEncoder, PREVIEW_HEIGHT};
pub use probe::{parse_probe, probe, AudioInfo, MediaProbe, VideoInfo};
pub use tools::{Tools, ENV_FFMPEG, ENV_FFPROBE};
pub use waveform::{PeakAccumulator, PEAKS_PER_SECOND};

/// 付属ファイルの `producer`。
pub(crate) const PRODUCER: &str = concat!("mm-ingest ", env!("CARGO_PKG_VERSION"));
/// 素材の ID の接頭辞。
const ASSET_ID_PREFIX: &str = "a_";
/// 素材の ID に使うコンテンツハッシュの先頭の文字数。
const ASSET_ID_HASH_CHARS: usize = 12;

/// 取り込みの設定。
#[derive(Debug, Clone)]
pub struct IngestOptions {
    pub cache: CacheLayout,
    pub tools: Tools,
    pub encoder: PreviewEncoder,
}

/// 出力が今回作られたか、すでにあって飛ばしたか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactState {
    Created,
    Cached,
}

/// 作った（または飛ばした）出力 1 つ。
#[derive(Debug, Clone)]
pub struct ArtifactReport {
    pub path: PathBuf,
    pub state: ArtifactState,
    /// 作るのにかかった時間。飛ばしたときは 0。
    pub elapsed: Duration,
    /// 補足（プレビュー動画のエンコーダー）。
    pub note: Option<String>,
}

/// 取り込みの結果。
#[derive(Debug, Clone)]
pub struct IngestReport {
    /// プロジェクトに足す素材。
    pub asset: Asset,
    /// 映像の無い素材では `None`。
    pub preview: Option<ArtifactReport>,
    /// 映像の無い素材では `None`。
    pub thumbnail: Option<ArtifactReport>,
    /// 音の無い素材では `None`。
    pub waveform: Option<ArtifactReport>,
    /// `cache/ingest/` に文字起こしの付属ファイルがあれば、その区間の数。
    pub transcript_segments: Option<usize>,
    pub hash_elapsed: Duration,
    pub probe_elapsed: Duration,
    /// 全体の時間。
    pub total_elapsed: Duration,
}

impl IngestReport {
    /// 作った出力が 1 つも無い（全部すでにあった）か。
    pub fn all_cached(&self) -> bool {
        [&self.preview, &self.thumbnail, &self.waveform]
            .into_iter()
            .flatten()
            .all(|a| a.state == ArtifactState::Cached)
    }
}

/// コンテンツハッシュから決まる素材の ID。同じ素材は何度取り込んでも同じ ID になる。
pub fn asset_id(content_hash: &str) -> AssetId {
    let short: String = content_hash.chars().take(ASSET_ID_HASH_CHARS).collect();
    Id::new(format!("{ASSET_ID_PREFIX}{short}"))
}

/// 素材 1 つを取り込む。
pub fn ingest(opts: &IngestOptions, input: &Path) -> Result<IngestReport> {
    let started = Instant::now();
    let abs = std::path::absolute(input)?;
    if !abs.is_file() {
        return Err(MmError::NotFound(format!("素材: {}", abs.display())));
    }
    let span = info_span!("ingest.file", path = %abs.display(), hash = tracing::field::Empty);
    let _guard = span.enter();
    let ((hash, hash_elapsed), (media, probe_elapsed)) = hash_and_probe(opts, &abs)?;
    let short: String = hash.chars().take(8).collect();
    span.record("hash", short.as_str());
    let made = make_artifacts(opts, &abs, &hash, &media)?;
    let asset = build_asset(&abs, &hash, &media)?;
    Ok(IngestReport {
        asset,
        preview: made.preview,
        thumbnail: made.thumbnail,
        waveform: made.waveform,
        transcript_segments: sidecar::load_transcript(&opts.cache, &hash)?
            .map(|t| t.segments.len()),
        hash_elapsed,
        probe_elapsed,
        total_elapsed: started.elapsed(),
    })
}

type Timed<T> = (T, Duration);

/// 別のスレッドに渡す、呼び出し側のスパンと購読者。スレッドごとの購読者でも子のスレッドのスパンが記録される。
struct Ctx {
    parent: Span,
    dispatch: tracing::Dispatch,
}

impl Ctx {
    fn capture() -> Self {
        Self {
            parent: Span::current(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        }
    }

    /// 呼び出し側のスパンの下で、`span` を開いて `f` を回す。
    fn run<T>(&self, span: impl FnOnce() -> Span, f: impl FnOnce() -> T) -> T {
        tracing::dispatcher::with_default(&self.dispatch, || {
            self.parent.in_scope(|| span().in_scope(f))
        })
    }
}

/// ハッシュと調べは互いに待たないので並行にする。
fn hash_and_probe(opts: &IngestOptions, path: &Path) -> Result<(Timed<String>, Timed<MediaProbe>)> {
    let ctx = Ctx::capture();
    std::thread::scope(|s| {
        let h = s.spawn(|| ctx.run(|| info_span!("ingest.hash"), || timed(|| hash_file(path))));
        let p = s.spawn(|| {
            ctx.run(
                || info_span!("ingest.probe"),
                || timed(|| probe(&opts.tools, path)),
            )
        });
        let hash = join(h)?;
        let media = join(p)?;
        Ok((hash?, media?))
    })
}

fn timed<T>(f: impl FnOnce() -> Result<T>) -> Result<Timed<T>> {
    let t = Instant::now();
    let v = f()?;
    Ok((v, t.elapsed()))
}

fn join<T>(h: std::thread::ScopedJoinHandle<'_, T>) -> Result<T> {
    h.join()
        .map_err(|_| MmError::Internal("取り込みのスレッドが落ちた".into()))
}

struct Made {
    preview: Option<ArtifactReport>,
    thumbnail: Option<ArtifactReport>,
    waveform: Option<ArtifactReport>,
}

/// プレビュー動画、サムネイル、波形を並行に作る。
fn make_artifacts(
    opts: &IngestOptions,
    input: &Path,
    hash: &str,
    media: &MediaProbe,
) -> Result<Made> {
    let ctx = Ctx::capture();
    std::thread::scope(|s| {
        let preview = media.video.as_ref().map(|_| {
            s.spawn(|| {
                ctx.run(
                    || info_span!("ingest.preview"),
                    || preview_job(opts, input, hash, media),
                )
            })
        });
        let thumbnail = media.video.as_ref().map(|_| {
            s.spawn(|| {
                ctx.run(
                    || info_span!("ingest.thumbnail"),
                    || thumbnail_job(opts, input, hash, media),
                )
            })
        });
        let waveform = media.audio.as_ref().map(|a| {
            s.spawn(|| {
                ctx.run(
                    || info_span!("ingest.waveform"),
                    || waveform_job(opts, input, hash, a.channels),
                )
            })
        });
        Ok(Made {
            preview: collect(preview)?,
            thumbnail: collect(thumbnail)?,
            waveform: collect(waveform)?,
        })
    })
}

fn collect(
    h: Option<std::thread::ScopedJoinHandle<'_, Result<ArtifactReport>>>,
) -> Result<Option<ArtifactReport>> {
    h.map(|h| join(h)?).transpose()
}

/// `out` がなければ `make` で作る。`make` は補足を返してよい。
fn ensure(
    out: PathBuf,
    make: impl FnOnce(&Path) -> Result<Option<String>>,
) -> Result<ArtifactReport> {
    if out.is_file() {
        return Ok(ArtifactReport {
            path: out,
            state: ArtifactState::Cached,
            elapsed: Duration::ZERO,
            note: None,
        });
    }
    let t = Instant::now();
    let note = make(&out)?;
    Ok(ArtifactReport {
        path: out,
        state: ArtifactState::Created,
        elapsed: t.elapsed(),
        note,
    })
}

fn preview_job(
    opts: &IngestOptions,
    input: &Path,
    hash: &str,
    media: &MediaProbe,
) -> Result<ArtifactReport> {
    let report = ensure(opts.cache.preview(hash), |out| {
        let r = preview::make_preview(&opts.tools, input, media, out, opts.encoder)?;
        Ok(Some(r.encoder.to_owned()))
    })?;
    let record = opts.cache.sidecar(hash, "preview_media");
    if !record.is_file() {
        let made = probe(&opts.tools, &report.path)?;
        let video = made
            .video
            .ok_or_else(|| MmError::Corrupt("プレビュー動画に映像が無い".into()))?;
        sidecar::write_preview_record(&opts.cache, hash, &report.path, &video)?;
    }
    Ok(report)
}

fn thumbnail_job(
    opts: &IngestOptions,
    input: &Path,
    hash: &str,
    media: &MediaProbe,
) -> Result<ArtifactReport> {
    ensure(opts.cache.thumbnail(hash), |out| {
        thumbnail::make_thumbnail(&opts.tools, input, media, out).map(|()| None)
    })
}

fn waveform_job(
    opts: &IngestOptions,
    input: &Path,
    hash: &str,
    channels: u16,
) -> Result<ArtifactReport> {
    ensure(opts.cache.waveform(hash), |out| {
        waveform::make_waveform(&opts.tools, input, hash, channels, out).map(|()| None)
    })
}

/// プロジェクトに足す素材。パスは絶対パスで持つ（既定は元の場所を参照）。
fn build_asset(path: &Path, hash: &str, media: &MediaProbe) -> Result<Asset> {
    let color_space = media
        .video
        .as_ref()
        .map_or_else(mm_types::ColorSpace::bt709, |v| v.color_space.clone());
    Ok(Asset {
        id: asset_id(hash),
        path: path.to_string_lossy().into_owned(),
        content_hash: hash.to_owned(),
        color_space,
        duration: media.duration,
        fps: media.video.as_ref().and_then(|v| v.fps),
        timebase_offset: media.timebase_offset()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 素材の_id_はコンテンツハッシュの先頭から決まる() {
        let id = asset_id("0123456789abcdef0123");
        assert_eq!(id.as_str(), "a_0123456789ab");
        assert_eq!(id, asset_id("0123456789abcdef0123"));
    }

    #[test]
    fn すでにある出力は作らずに飛ばす() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("x.bin");
        let first = ensure(out.clone(), |p| {
            std::fs::write(p, b"x")?;
            Ok(None)
        })
        .unwrap();
        assert_eq!(first.state, ArtifactState::Created);
        let second = ensure(out, |_| panic!("作り直してはならない")).unwrap();
        assert_eq!(second.state, ArtifactState::Cached);
    }
}
