//! 書き出し。レンダーターゲットを選び、出力先を整えて `Engine` に書き出させ、速さを測って返す。
//!
//! 区間の描画とエンコーダーへのバッチ押しは `Engine::export`（mm-runtime）が持つ。口の実装を
//! 選ぶのは mm-runtime だけという約束のため、このクレートは実装を知らず、`Engine` 越しに呼ぶ。
//! 段階 0 は 1 プロセス・1 区間で、並行の描画とラウドネスの正規化は無い。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mm_doc::Project;
use mm_runtime::Engine;
use mm_types::{MmError, RenderTargetId, Result};

/// 書き出しの指定。
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// 文書のレンダーターゲットの `output_path` を上書きする。
    pub output: Option<PathBuf>,
    /// 書き出すレンダーターゲット。`None` なら文書の最初の 1 つ。
    pub target: Option<RenderTargetId>,
}

/// 書き出した結果と速さ。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub output_path: PathBuf,
    pub frames: u64,
    pub audio_frames: u64,
    pub bytes_written: u64,
    pub elapsed: Duration,
    /// 全体の fps（コマ数 / 経過時間）。
    pub fps: f64,
}

/// 書き出すレンダーターゲットを決める。指定が無ければ最初の 1 つ。
pub fn pick_target(project: &Project, wanted: Option<&RenderTargetId>) -> Result<RenderTargetId> {
    match wanted {
        Some(id) => project.render_target(id).map(|t| t.id.clone()),
        None => project
            .render_targets
            .first()
            .map(|t| t.id.clone())
            .ok_or_else(|| MmError::NotFound("文書にレンダーターゲットが無い".into())),
    }
}

/// 出力先の親ディレクトリを作る。無ければエンコーダーが開けずに落ちるため先に整える。
fn ensure_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Ok(std::fs::create_dir_all(dir)?),
        _ => Ok(()),
    }
}

/// レンダーターゲットを書き出す。ゲージ `export.fps` に全体の fps を出す。
pub fn export_project(
    engine: &mut Engine,
    project: &Project,
    options: &ExportOptions,
) -> Result<ExportSummary> {
    let span = tracing::info_span!("export.run");
    let _g = span.enter();
    let target = pick_target(project, options.target.as_ref())?;
    let mut project = project.clone();
    if let Some(output) = &options.output {
        let t = project
            .render_targets
            .iter_mut()
            .find(|t| t.id == target)
            .ok_or_else(|| MmError::NotFound(format!("render target {target}")))?;
        t.output_path = output.to_string_lossy().into_owned();
    }
    let output_path = PathBuf::from(&project.render_target(&target)?.output_path);
    ensure_parent(&output_path)?;

    let started = Instant::now();
    let report = engine.export(&project, &target)?;
    let elapsed = started.elapsed();
    let fps = report.encode.frames as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    mm_trace::gauge!("export.fps", fps);
    Ok(ExportSummary {
        output_path: report.output_path,
        frames: report.encode.frames,
        audio_frames: report.encode.audio_frames,
        bytes_written: report.encode.bytes_written,
        elapsed,
        fps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_runtime::{assemble, RuntimeConfig};
    use mm_testkit::decode::SampleMedia;
    use mm_testkit::fixtures::{vertical_project, VERTICAL_DURATION_SECONDS};

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mm_export_{name}_{}", std::process::id()))
    }

    #[test]
    fn 出力先を上書きして書き出す() {
        let media = SampleMedia::default();
        let (project, _, _) = vertical_project(&media, &temp("unused.json"));
        let out = temp("dir").join("nested").join("out.json");
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        let summary = export_project(
            &mut engine,
            &project,
            &ExportOptions {
                output: Some(out.clone()),
                target: None,
            },
        )
        .unwrap();
        assert_eq!(summary.output_path, out);
        assert_eq!(
            summary.frames,
            VERTICAL_DURATION_SECONDS as u64 * media.fps.num() as u64
        );
        assert!(out.exists(), "親ディレクトリごと作られる");
        assert!(summary.fps > 0.0);
        let _ = std::fs::remove_dir_all(temp("dir"));
    }

    #[test]
    fn 無いレンダーターゲットは断る() {
        let project = Project::new();
        assert!(pick_target(&project, None).is_err());
    }
}
