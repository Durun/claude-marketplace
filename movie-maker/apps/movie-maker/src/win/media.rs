//! Media パネルのアプリ側の処理。素材の一覧、サムネイル、波形、`cache/` の大きさと掃除、取り込み。
//! 取り込みは `mm ingest` と同じ `mm_ingest::ingest` の流れで、`addAsset` を再生スレッドに適用させる。
//! Tauri に依存しない部分（ここ）と、Tauri のコマンドのハンドラー（`media_ipc.rs`）を分けて、WebView 無しで試せるようにする。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};

use mm_doc::Asset;
use mm_ingest::{CacheLayout, IngestOptions, MediaProbe, PreviewEncoder, Tools};
use mm_ingest_api::SidecarBody;
use mm_ops::{AddAsset, Op};
use serde::Serialize;

use crate::win::state::Shared;

/// プロジェクトの中のキャッシュのフォルダー名。`mm ingest` と同じ。
const CACHE_DIR: &str = "cache";
/// 掃除してよいキャッシュ。作り直せるものだけで、`ingest/`（文字起こしなどの付属ファイル）は消さない。
pub const CLEARABLE: [&str; 3] = ["preview", "audio", "thumbs"];
/// 大きさを数える `cache/` の下のフォルダー。
const CACHE_FOLDERS: [&str; 4] = ["preview", "audio", "thumbs", "ingest"];

/// 素材の取り込みの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaStatus {
    /// 作るべき出力（プレビュー動画、サムネイル、波形）が全部ある。
    Ready,
    /// 出力が足りない（プレビュー動画は後から足されるので、取り込みの直後はこの状態）。
    Partial,
    /// 元のファイルが見つからない。
    Missing,
}

/// 素材の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Video,
    Audio,
    /// ffprobe が使えず、調べられなかった。
    Unknown,
}

/// 素材 1 つの一覧の行。
#[derive(Debug, Clone, Serialize)]
pub struct MediaItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub content_hash: String,
    pub kind: MediaKind,
    pub duration: f64,
    pub fps: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_thumbnail: bool,
    pub has_waveform: bool,
    pub status: MediaStatus,
}

/// 波形を一覧の幅に間引いたもの。ピークは 0..=1（チャンネルの最大）。
#[derive(Debug, Clone, Serialize)]
pub struct WaveformView {
    pub duration: f64,
    pub peaks: Vec<f32>,
}

/// `cache/` の下のフォルダー 1 つの大きさ。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CacheFolder {
    pub name: String,
    pub bytes: u64,
    pub files: u64,
    pub clearable: bool,
}

/// `cache/` の大きさ。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CacheInfo {
    pub total_bytes: u64,
    pub folders: Vec<CacheFolder>,
}

/// 取り込みの進み具合。Tauri のイベント `mm:ingest-progress` の中身。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportEvent {
    Started {
        index: usize,
        total: usize,
        path: String,
    },
    /// `added` が偽なら、同じコンテンツハッシュの素材がすでにプロジェクトにあった。
    Done {
        index: usize,
        path: String,
        asset_id: String,
        added: bool,
    },
    Failed {
        index: usize,
        path: String,
        error: String,
    },
    Finished {
        added: usize,
        skipped: usize,
        failed: usize,
    },
}

/// アプリ全体で 1 つの Media の状態。
pub struct MediaState {
    shared: Arc<Shared>,
    cache: CacheLayout,
    tools: Tools,
    /// 素材の中身の調べ。ffprobe を一覧のたびに呼ばないよう、コンテンツハッシュをキーにキャッシュする。
    probes: Mutex<HashMap<String, Option<MediaProbe>>>,
    /// 取り込みは 1 度に 1 つだけ回す。掃除も取り込みと同時に回さない（作っている途中の出力を消さない）。
    importing: Mutex<()>,
    /// ファイルの選択とドロップで人が渡したパス。取り込みはこの中のパスだけを受ける（WebView から任意のパスを読ませない）。
    offered: Mutex<HashSet<PathBuf>>,
}

/// プロジェクトの `cache/`。`project.mmdb` のフォルダーか、プロジェクトの JSON のあるフォルダーの下。
pub fn project_cache_dir(project: &Path) -> PathBuf {
    let dir = mm_store::store_dir(project)
        .unwrap_or_else(|| project.parent().unwrap_or(Path::new(".")).to_path_buf());
    dir.join(CACHE_DIR)
}

impl MediaState {
    pub fn new(shared: Arc<Shared>, cache_root: PathBuf) -> Self {
        Self {
            shared,
            cache: CacheLayout::new(cache_root),
            tools: Tools::default(),
            probes: Mutex::new(HashMap::new()),
            importing: Mutex::new(()),
            offered: Mutex::new(HashSet::new()),
        }
    }

    /// ファイルの選択かドロップで得たパスを、取り込んでよいパスとして覚える。
    pub fn offer<P: Into<PathBuf>>(&self, paths: impl IntoIterator<Item = P>) {
        let mut offered = self.offered.lock().expect("offered");
        offered.extend(paths.into_iter().map(Into::into));
    }

    /// `paths` が全部、人が選ぶかドロップしたパスか。違うものがあればその 1 つを返す。
    fn check_offered(&self, paths: &[String]) -> Result<(), String> {
        let offered = self.offered.lock().expect("offered");
        match paths.iter().find(|p| !offered.contains(Path::new(p))) {
            Some(p) => Err(format!(
                "選択かドロップで渡されていないパスは取り込まない: {p}"
            )),
            None => Ok(()),
        }
    }

    fn assets(&self) -> Vec<Asset> {
        self.shared
            .doc()
            .map(|d| d.project.assets.clone())
            .unwrap_or_default()
    }

    fn asset(&self, id: &str) -> Result<Asset, String> {
        self.assets()
            .into_iter()
            .find(|a| a.id.as_str() == id)
            .ok_or_else(|| format!("素材が無い: {id}"))
    }

    fn probe_of(&self, asset: &Asset) -> Option<MediaProbe> {
        if let Some(hit) = self.probes.lock().expect("probes").get(&asset.content_hash) {
            return hit.clone();
        }
        let found = mm_ingest::probe(&self.tools, Path::new(&asset.path)).ok();
        self.probes
            .lock()
            .expect("probes")
            .insert(asset.content_hash.clone(), found.clone());
        found
    }

    /// 素材の一覧（プロジェクトの並び順）。
    pub fn list(&self) -> Vec<MediaItem> {
        self.assets().iter().map(|a| self.item_of(a)).collect()
    }

    fn item_of(&self, a: &Asset) -> MediaItem {
        let thumb = self.cache.thumbnail(&a.content_hash).is_file();
        let wave = self.cache.waveform(&a.content_hash).is_file();
        let preview = self.cache.preview(&a.content_hash).is_file();
        let present = Path::new(&a.path).is_file();
        let probe = if present { self.probe_of(a) } else { None };
        let video = probe.as_ref().and_then(|p| p.video.as_ref());
        let kind = match &probe {
            Some(p) if p.video.is_some() => MediaKind::Video,
            Some(_) => MediaKind::Audio,
            None => MediaKind::Unknown,
        };
        let complete = match &probe {
            Some(p) => (p.video.is_none() || (thumb && preview)) && (p.audio.is_none() || wave),
            None => thumb || wave,
        };
        let status = if !present {
            MediaStatus::Missing
        } else if complete {
            MediaStatus::Ready
        } else {
            MediaStatus::Partial
        };
        MediaItem {
            id: a.id.to_string(),
            name: file_name(&a.path),
            path: a.path.clone(),
            content_hash: a.content_hash.clone(),
            kind,
            duration: a.duration.to_f64(),
            fps: a.fps.map(|f| f.to_f64()),
            width: video.map(|v| v.width),
            height: video.map(|v| v.height),
            has_thumbnail: thumb,
            has_waveform: wave,
            status,
        }
    }

    /// サムネイルの JPEG。
    pub fn thumbnail(&self, id: &str) -> Result<Vec<u8>, String> {
        let a = self.asset(id)?;
        std::fs::read(self.cache.thumbnail(&a.content_hash))
            .map_err(|e| format!("サムネイルを読めない: {e}"))
    }

    /// 波形を `buckets` 個のピークに間引く。
    pub fn waveform(&self, id: &str, buckets: usize) -> Result<WaveformView, String> {
        let a = self.asset(id)?;
        let path = self.cache.waveform(&a.content_hash);
        let sidecar = mm_ingest::sidecar::read_sidecar(&path, &a.content_hash)
            .map_err(|e| format!("波形を読めない: {e}"))?;
        let SidecarBody::Waveform(w) = sidecar.body else {
            return Err("波形の付属ファイルでない".into());
        };
        Ok(WaveformView {
            duration: a.duration.to_f64(),
            peaks: downsample(&w.peaks, usize::from(w.channels), buckets),
        })
    }

    /// `cache/` の大きさ。
    pub fn cache_info(&self) -> CacheInfo {
        let folders: Vec<CacheFolder> = CACHE_FOLDERS
            .iter()
            .map(|name| {
                let (bytes, files) = dir_size(&self.cache.root().join(name));
                CacheFolder {
                    name: (*name).to_owned(),
                    bytes,
                    files,
                    clearable: CLEARABLE.contains(name),
                }
            })
            .collect();
        CacheInfo {
            total_bytes: folders.iter().map(|f| f.bytes).sum(),
            folders,
        }
    }

    /// 掃除してよいフォルダーを消し、空けた大きさを返す。許可の無い名前は何も消さずに拒否する。
    pub fn clear_cache(&self, kinds: &[String]) -> Result<u64, String> {
        if let Some(bad) = kinds.iter().find(|k| !CLEARABLE.contains(&k.as_str())) {
            return Err(format!("掃除できないフォルダー: {bad}"));
        }
        let _guard = match self.importing.try_lock() {
            Ok(g) => g,
            Err(TryLockError::WouldBlock) => return Err("取り込み中は掃除できない".into()),
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
        };
        let mut freed = 0;
        for kind in kinds {
            let dir = self.cache.root().join(kind);
            freed += dir_size(&dir).0;
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{kind} を消せない: {e}")),
            }
        }
        Ok(freed)
    }

    /// 素材を取り込み、新しい素材を 1 つずつ `addAsset` で足す。取り込み中にもう一度呼ぶと拒否する。
    pub fn import(&self, paths: &[String], emit: &dyn Fn(ImportEvent)) -> Result<(), String> {
        self.check_offered(paths)?;
        let _guard = match self.importing.try_lock() {
            Ok(g) => g,
            Err(TryLockError::WouldBlock) => return Err("取り込み中".into()),
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
        };
        let opts = IngestOptions {
            cache: self.cache.clone(),
            tools: self.tools.clone(),
            encoder: PreviewEncoder::Auto,
        };
        let known = || self.assets().into_iter().map(|a| a.content_hash).collect();
        let apply = |op: Op| crate::win::edit::submit(&self.shared, vec![op]).map(|_| ());
        run_import(&opts, paths, &known, &apply, emit);
        Ok(())
    }
}

/// 取り込みの本体。素材ごとに進み具合を `emit` に渡す。失敗した素材はスキップして次へ進む。
pub fn run_import(
    opts: &IngestOptions,
    paths: &[String],
    known: &dyn Fn() -> HashSet<String>,
    apply: &dyn Fn(Op) -> Result<(), String>,
    emit: &dyn Fn(ImportEvent),
) {
    let (mut added, mut skipped, mut failed) = (0, 0, 0);
    // この取り込みで足した中身のハッシュ。`known` が適用をまだ映していなくても、同じ中身を 2 度足さない。
    let mut fresh = HashSet::new();
    for (index, path) in paths.iter().enumerate() {
        emit(ImportEvent::Started {
            index,
            total: paths.len(),
            path: path.clone(),
        });
        match import_one(opts, path, known, &mut fresh, apply) {
            Ok((asset_id, was_added)) => {
                if was_added {
                    added += 1;
                } else {
                    skipped += 1;
                }
                emit(ImportEvent::Done {
                    index,
                    path: path.clone(),
                    asset_id,
                    added: was_added,
                });
            }
            Err(error) => {
                failed += 1;
                emit(ImportEvent::Failed {
                    index,
                    path: path.clone(),
                    error,
                });
            }
        }
    }
    emit(ImportEvent::Finished {
        added,
        skipped,
        failed,
    });
}

/// 素材 1 つを取り込み、まだプロジェクトに無ければ足す。素材の ID と、足したかを返す。
fn import_one(
    opts: &IngestOptions,
    path: &str,
    known: &dyn Fn() -> HashSet<String>,
    fresh: &mut HashSet<String>,
    apply: &dyn Fn(Op) -> Result<(), String>,
) -> Result<(String, bool), String> {
    let report = mm_ingest::ingest(opts, Path::new(path)).map_err(|e| e.to_string())?;
    let id = report.asset.id.to_string();
    let hash = report.asset.content_hash.clone();
    if fresh.contains(&hash) || known().contains(&hash) {
        return Ok((id, false));
    }
    let op = AddAsset {
        asset: report.asset,
        index: None,
    };
    apply(op.into())?;
    fresh.insert(hash);
    Ok((id, true))
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// インターリーブしたピークを、チャンネルの最大を取りながら `buckets` 個にする。
pub fn downsample(peaks: &[f32], channels: usize, buckets: usize) -> Vec<f32> {
    let channels = channels.max(1);
    let frames = peaks.len() / channels;
    if frames == 0 || buckets == 0 {
        return Vec::new();
    }
    let buckets = buckets.min(frames);
    (0..buckets)
        .map(|b| {
            let from = b * frames / buckets;
            let to = ((b + 1) * frames / buckets).max(from + 1);
            peaks[from * channels..to * channels]
                .iter()
                .fold(0.0_f32, |m, v| m.max(*v))
        })
        .collect()
}

/// フォルダーの下のファイルの合計バイト数とファイル数。無いフォルダーは 0。
fn dir_size(dir: &Path) -> (u64, u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    entries
        .flatten()
        .fold((0, 0), |(bytes, files), e| match e.metadata() {
            Ok(m) if m.is_dir() => {
                let (b, f) = dir_size(&e.path());
                (bytes + b, files + f)
            }
            Ok(m) => (bytes + m.len(), files + 1),
            Err(_) => (bytes, files),
        })
}

#[cfg(test)]
mod tests;
