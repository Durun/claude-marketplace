//! 納品物。字幕、チャプター、ラウドネスの正規化を、レンダーターゲットの `delivery` と `loudness_lufs` から決めて作る。

use std::path::{Path, PathBuf};

use mm_audio_api::{AudioBlock, NormalizeReport, NormalizeSpec};
use mm_decode_api::{ExtraTracks, SubtitleCue};
use mm_doc::{Composition, Project, RenderTarget, SubtitleOutput};
use mm_runtime::Engine;
use mm_types::{MmError, Result, TimeRange};

use crate::captions::{composition_cues, TranscriptWords};
use crate::chapters::{chapter_marks, youtube_description};
use crate::subtitle_format::{srt, vtt};

/// 納品物の結果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeliveryReport {
    /// ラウドネスの正規化の測定と結果。目標が無ければ `None`。
    pub normalization: Option<NormalizeReport>,
    /// 書き出しの隣に置いた字幕のファイル。
    pub subtitle_files: Vec<PathBuf>,
    /// 書き出しの隣に置いた、YouTube の説明欄用のチャプターの文字列のファイル。
    pub chapters_file: Option<PathBuf>,
    /// 出さなかったものの理由など。
    pub warnings: Vec<String>,
}

/// 書き出しの前に決める納品物。
#[derive(Debug, Default)]
pub(crate) struct Prepared {
    /// エンコーダーに渡す字幕のトラックとチャプター。
    pub tracks: ExtraTracks,
    /// 書き出しの隣に置く字幕。
    sidecar_cues: Vec<SubtitleCue>,
    /// 書き出しの隣に置くチャプターの文字列。
    chapters_text: Option<String>,
    pub warnings: Vec<String>,
}

/// 字幕とチャプターを作る。字幕を出す設定で文字起こしが渡されていなければ拒否する（黙って字幕なしにしない）。
pub(crate) fn prepare(
    project: &Project,
    target: &RenderTarget,
    comp: &Composition,
    range: TimeRange,
    transcript: Option<&TranscriptWords>,
) -> Result<Prepared> {
    let mut out = Prepared::default();
    let d = &target.delivery;
    if d.subtitles != SubtitleOutput::None {
        let words = transcript.ok_or_else(|| {
            MmError::InvalidArgument(
                "レンダーターゲットが字幕を出す設定だが、文字起こしが渡されていない".into(),
            )
        })?;
        let cues = composition_cues(project, comp, range, words)?;
        if cues.is_empty() {
            out.warnings.push("字幕にできる文字起こしが無い".into());
        } else if d.subtitles == SubtitleOutput::Embedded {
            out.tracks.subtitles = cues;
        } else {
            out.sidecar_cues = cues;
        }
    }
    if d.chapters {
        let marks = chapter_marks(comp, range)?;
        match youtube_description(&marks) {
            Ok(text) => out.chapters_text = Some(text),
            Err(e) => out
                .warnings
                .push(format!("YouTube のチャプターの文字列を作れない: {e}")),
        }
        if marks.is_empty() {
            out.warnings.push("チャプターにするマーカーが無い".into());
        }
        out.tracks.chapters = marks;
    }
    Ok(out)
}

/// 字幕の形式の文字列を作る関数。
type FormatCues = fn(&[SubtitleCue]) -> String;
/// 書き出しの隣に置く字幕の拡張子と、その形式の文字列を作る関数。
const SUBTITLE_FORMATS: [(&str, FormatCues); 2] = [("srt", srt), ("vtt", vtt)];
/// 書き出しの隣に置くチャプターの文字列の拡張子。
const CHAPTERS_EXT: &str = "chapters.txt";

/// `<出力>.srt`、`<出力>.vtt`、`<出力>.chapters.txt` のうち要るものを書き、要らなくなった前回のものを消す。
/// 前回の字幕が残ると今回の書き出しと食い違うので、自分が作る名前に完全に一致するファイルだけ消して警告に出す。
pub(crate) fn write_sidecars(prepared: &Prepared, output: &Path) -> Result<DeliveryReport> {
    let mut report = DeliveryReport {
        warnings: prepared.warnings.clone(),
        ..DeliveryReport::default()
    };
    let cues = &prepared.sidecar_cues;
    for (ext, format) in SUBTITLE_FORMATS {
        let path = output.with_extension(ext);
        if cues.is_empty() {
            remove_stale(&path, &mut report.warnings)?;
            continue;
        }
        std::fs::write(&path, format(cues))?;
        report.subtitle_files.push(path);
    }
    let path = output.with_extension(CHAPTERS_EXT);
    match &prepared.chapters_text {
        Some(text) => {
            std::fs::write(&path, text)?;
            report.chapters_file = Some(path);
        }
        None => remove_stale(&path, &mut report.warnings)?,
    }
    Ok(report)
}

/// 前回の書き出しが置いたファイルがあれば消し、警告に出す。
fn remove_stale(path: &Path, warnings: &mut Vec<String>) -> Result<()> {
    if path.is_file() {
        std::fs::remove_file(path)?;
        warnings.push(format!(
            "今回は出さないので、前回の {} を消した",
            path.display()
        ));
    }
    Ok(())
}

/// 目標のラウドネスがあれば、音を正規化する（実装は `Engine::mixers()` の `loudness_normalizer`）。
pub(crate) fn normalize(
    engine: &Engine,
    target: &RenderTarget,
    audio: &mut AudioBlock,
) -> Result<Option<NormalizeReport>> {
    let Some(target_lufs) = target.loudness_lufs else {
        return Ok(None);
    };
    let normalizer = engine.mixers().loudness_normalizer().ok_or_else(|| {
        MmError::Unsupported(format!(
            "音の実装 {} はラウドネスの正規化を持たない",
            engine.mixers().id()
        ))
    })?;
    let spec = NormalizeSpec {
        target_lufs,
        ceiling_dbtp: target.delivery.ceiling_dbtp(),
    };
    normalizer.normalize(audio, &spec).map(Some)
}
