//! 書き出し。レンダーターゲットを選び、出力先を整え、区間を描いてエンコーダーに送り、速さを測って返す。
//!
//! インターフェースの実装を選ぶのは mm-runtime だけという契約のため、このクレートは実装を知らず、`Engine` から
//! レンダラーとエンコーダーを取り出して使う（[`Engine::renderer`]、[`Engine::encoders`]）。
//! 工程ごとの時間はスパン `export.render`（1 フレームの描画とリードバック）と `export.encode`（1 バッチの送信）に
//! 出す。
//!
//! - レンダーターゲットごと（[`export_targets`]）、範囲の指定（[`ExportOptions::range`]）、
//!   sequence のまま（[`ExportOptions::sequence`]）の書き出し。
//! - 出力は `<名前>.partial.<拡張子>` に書き、完成してから置き換える。失敗と取り消しでは一時ファイルを消す。
//! - 進み具合と取り消しは [`ExportControl`]。結果の検証は [`verify`]（ffprobe）。
//! - 並行の描画（[`ExportOptions::parallel`]）。区間を短いチャンクに分けて複数のレンダラーで描き、フレームの順に
//!   エンコーダーへ送る。2 パスのコーデックは、同じ描画を 2 回通す。
//! - 字幕の焼き込み（`burn_in`）: 字幕を text ノードのレイヤーにしたプロジェクトのデータを作る。書き出しとプレビューが同じものを描く。
//! - 納品物（`deliver`、`captions`、`chapters`）: 目標のラウドネスへの正規化（2 パス、トゥルーピークの上限つき）、
//!   字幕（書き出しの隣の SRT と WebVTT、または MP4 の字幕のトラック）、チャプター（MP4 と YouTube の説明欄の文字列）。

mod burn_in;
mod captions;
mod chapters;
mod control;
mod cue_text;
mod deliver;
mod ordered;
mod parallel;
mod passes;
mod run;
mod sequence;
mod subtitle_format;
pub mod verify;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mm_doc::{Project, SequenceId};
use mm_runtime::Engine;
use mm_types::{MmError, RenderTargetId, Result, TimeRange};

pub use burn_in::{burn_in_captions, burn_in_enabled};
pub use captions::{sequence_cues, CaptionWord, TranscriptWords};
pub use chapters::{chapter_marks, youtube_description};
pub use control::{ExportControl, ExportProgress, ProgressFn};
pub use deliver::DeliveryReport;
pub use run::export_run;
pub use subtitle_format::{srt, vtt};
pub use verify::{verify_output, Expected, Verification};

use control::partial_path;
#[cfg(test)]
use passes::{export_frames, export_passes, PassPosition, Plan};
#[cfg(test)]
use run::check_container;

/// エンコーダーに一度に送るフレームの数。インターフェースの呼び出しを毎フレームにしない。
pub const EXPORT_BATCH_FRAMES: usize = 8;
/// 取り消しでエンコーダーの送信を止めたときのエラーの文言。呼び出しの内側だけで使い、外へは出さない。
const CANCELLED_MARK: &str = "書き出しの取り消し";

/// 書き出しの指定。
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// プロジェクトのデータのレンダーターゲットの `output_path` を上書きする。
    pub output: Option<PathBuf>,
    /// 書き出すレンダーターゲット。`None` ならプロジェクトのデータの最初の 1 つ。
    pub target: Option<RenderTargetId>,
    /// 書き出す区間。`None` ならコンポジション（`sequence` があればその全長）の全部。コンポジションの外は拒否する。
    pub range: Option<TimeRange>,
    /// レンダーターゲットのコンポジションの代わりに、この sequence を全長で書き出す。
    pub sequence: Option<SequenceId>,
    /// 書き出したあと ffprobe で出力を検証する。モックのエンコーダー（JSON を書く）では使えない。
    pub verify: bool,
    /// プロジェクトのデータのレンダーターゲットの `codec` を上書きする（`h264_nvenc`、`prores_422hq` など）。
    pub codec: Option<String>,
    /// 並行に描くレンダラーの数。0 と 1 は並行にしない（`Engine` のレンダラー 1 つで描く）。
    /// 2 以上は、レンダラーを作れる設定で組んだ別の `Engine` をその数だけ作り、区間をチャンクに分けて描かせる。
    pub parallel: usize,
    /// 並行にする最短のフレーム数。`None` は `parallel` 本 x 30 フレーム以上のとき（それより短いと全ワーカーに
    /// チャンクが回らないので、1 つのレンダラーで描く）。
    pub parallel_min_frames: Option<u64>,
    /// 素材ごとの字幕にする単語（消した単語を除き、直した文字を反映したもの）。レンダーターゲットが字幕を出す設定のとき要る。
    pub transcript: Option<Arc<TranscriptWords>>,
    /// 取り消しと進み具合。
    pub control: ExportControl,
}

/// 書き出した結果と速さ。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub target: RenderTargetId,
    pub output_path: PathBuf,
    pub frames: u64,
    pub audio_frames: u64,
    pub bytes_written: u64,
    pub elapsed: Duration,
    /// 全体の fps（フレーム数 / 経過時間）。
    pub fps: f64,
    /// `ExportOptions::verify` のときの、ffprobe が読んだ出力の中身。
    pub verification: Option<Verification>,
    /// ラウドネスの正規化の結果と、書き出しの隣に置いた字幕・チャプターのファイル。
    pub delivery: Box<DeliveryReport>,
    /// 実際に書いた実装とコーデック（`nvenc/h264_nvenc` など）。代わりのコーデックにフォールバックしたらその名前。分からない実装は `None`。
    pub encoder: Option<String>,
}

/// 1 つのレンダーターゲットの書き出しの終わり方。
#[derive(Debug, Clone, PartialEq)]
pub enum ExportOutcome {
    Completed(ExportSummary),
    /// 取り消した。一時ファイルは消してあり、前の出力はそのまま。
    Cancelled,
}

/// 複数のレンダーターゲットを書き出した結果。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportBatch {
    /// 完成したもの（指定の順）。
    pub done: Vec<ExportSummary>,
    /// 途中で取り消したか。取り消したら、残りのレンダーターゲットは書き出さない。
    pub cancelled: bool,
}

/// 書き出すレンダーターゲットを決める。指定が無ければ最初の 1 つ。
pub fn pick_target(project: &Project, wanted: Option<&RenderTargetId>) -> Result<RenderTargetId> {
    match wanted {
        Some(id) => project.render_target(id).map(|t| t.id.clone()),
        None => project
            .render_targets
            .first()
            .map(|t| t.id.clone())
            .ok_or_else(|| {
                MmError::NotFound("プロジェクトのデータにレンダーターゲットが無い".into())
            }),
    }
}

/// 出力先の親ディレクトリを作る。無ければエンコーダーが開けずにクラッシュするため先に整える。
fn ensure_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Ok(std::fs::create_dir_all(dir)?),
        _ => Ok(()),
    }
}

/// レンダーターゲットを 1 つ書き出す。取り消されたら `Err`（取り消しを区別したいときは [`export_run`]）。
/// ゲージ `export.fps` に全体の fps を出す。
pub fn export_project(
    engine: &mut Engine,
    project: &Project,
    options: &ExportOptions,
) -> Result<ExportSummary> {
    match export_run(engine, project, options)? {
        ExportOutcome::Completed(summary) => Ok(summary),
        ExportOutcome::Cancelled => Err(MmError::Internal("書き出しを取り消した".into())),
    }
}

/// `targets` を順に書き出す。`options.output` は 1 つのときだけ使える。
/// 同じ出力先を指すレンダーターゲットが 2 つあると互いに上書きするので、始める前に拒否する。
pub fn export_targets(
    engine: &mut Engine,
    project: &Project,
    targets: &[RenderTargetId],
    options: &ExportOptions,
) -> Result<ExportBatch> {
    if targets.len() > 1 && options.output.is_some() {
        return Err(MmError::InvalidArgument(
            "出力先の上書きは、レンダーターゲットが 1 つのときだけ使える".into(),
        ));
    }
    reject_shared_outputs(project, targets)?;
    let mut done = Vec::with_capacity(targets.len());
    for target in targets {
        let one = ExportOptions {
            target: Some(target.clone()),
            ..options.clone()
        };
        match export_run(engine, project, &one)? {
            ExportOutcome::Completed(summary) => done.push(summary),
            ExportOutcome::Cancelled => {
                return Ok(ExportBatch {
                    done,
                    cancelled: true,
                })
            }
        }
    }
    Ok(ExportBatch {
        done,
        cancelled: false,
    })
}

/// 出力先と一時ファイルのパスのどれかが他のレンダーターゲットと重なる組を拒否する
/// （A の一時ファイル `x.partial.mp4` が B の出力 `x.mp4` を指す場合も、互いに壊し合う）。
pub fn reject_shared_outputs(project: &Project, targets: &[RenderTargetId]) -> Result<()> {
    let mut seen: Vec<([PathBuf; 2], &RenderTargetId)> = Vec::new();
    for id in targets {
        let path = PathBuf::from(&project.render_target(id)?.output_path);
        let names = [partial_path(&path), path];
        if let Some((other_names, other)) = seen
            .iter()
            .find(|(others, _)| others.iter().any(|o| names.contains(o)))
        {
            let shared = names
                .iter()
                .find(|n| other_names.contains(n))
                .unwrap_or(&names[1]);
            return Err(MmError::InvalidArgument(format!(
                "レンダーターゲット {other} と {id} が同じ出力先 {} を指している",
                shared.display()
            )));
        }
        seen.push((names, id));
    }
    Ok(())
}

/// 取り消しで止めたことを表すエラーか。旗が立っていても、別の原因の失敗は取り消しにしない。
fn is_cancel_error(e: &MmError) -> bool {
    matches!(e, MmError::Internal(m) if m == CANCELLED_MARK)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_parallel;

#[cfg(test)]
mod tests_deliver;

#[cfg(test)]
mod tests_burn_in;

#[cfg(test)]
mod tests_mixer;
