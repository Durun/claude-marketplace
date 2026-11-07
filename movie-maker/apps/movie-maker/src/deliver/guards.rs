//! 書き出しを始める前の確かめ（出力先の重なり、ffprobe）と、キューを止める処理。

use super::*;

/// 依頼のレンダーターゲットと sequence が在るか、同じ出力先を指す組が無いかを確かめて、キューの順に返す。
/// 同じ出力先に順に書くと、先に書いた成果物を黙って上書きするため。
pub(super) fn pick(project: &Project, request: &StartRequest) -> Result<Vec<RenderTarget>, String> {
    if request.targets.is_empty() {
        return Err("書き出すレンダーターゲットを選んでいない".into());
    }
    if let Some(sequence) = &request.sequence {
        project
            .sequence(&Id::new(sequence.as_str()))
            .map_err(|e| e.to_string())?;
    }
    let ids: Vec<RenderTargetId> = request
        .targets
        .iter()
        .map(|t| Id::new(t.as_str()))
        .collect();
    let picked = ids
        .iter()
        .map(|id| project.render_target(id).cloned())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    reject_shared_outputs(&case_folded_outputs(project), &ids).map_err(|e| e.to_string())?;
    Ok(picked)
}

/// 出力先を小文字にしたデータ。Windows のファイル名は大文字小文字を区別しないので、
/// `Out.mp4` と `out.mp4` を同じ出力先として判定に渡す。
fn case_folded_outputs(project: &Project) -> Project {
    let mut folded = project.clone();
    for target in &mut folded.render_targets {
        target.output_path = target.output_path.to_lowercase();
    }
    folded
}

/// ffprobe を起動できるか。検証と同じ起動の経路（`MM_FFPROBE`）を通すため、存在しないパスを読ませて、
/// 「起動できない」（`NotFound`）かどうかだけを見る。
pub(super) fn ffprobe_available() -> bool {
    let expected = Expected {
        frames: 0,
        size: Size::new(1, 1),
        fps: 1.0,
        audio_seconds: None,
    };
    !matches!(
        verify_output(Path::new(""), &expected),
        Err(MmError::NotFound(_))
    )
}

/// コーデックに合う出力ファイルの拡張子。保存ダイアログの絞り込みに使う。ProRes と DNxHR は MOV にしか書けない。
pub fn output_extensions(codec: &str) -> &'static [&'static str] {
    if codec.starts_with("prores") || codec.starts_with("dnx") {
        &["mov"]
    } else {
        &["mp4", "mov"]
    }
}

/// キューを止まった状態にして返す。動いていない項目に理由を付けて失敗にする。
pub(super) fn stop_all(status: &Mutex<ExportStatus>, message: &str) -> ExportStatus {
    let mut status = status.lock().expect("export status");
    status.running = false;
    for item in &mut status.items {
        if matches!(item.state, ItemState::Queued | ItemState::Running) {
            item.state = ItemState::Failed;
            item.error.get_or_insert_with(|| message.to_owned());
        }
    }
    status.clone()
}
