//! 編集のキーと WebView のボタンが呼ぶ、名前のついた編集（イン点・アウト点、insert、overwrite、split、lift、extract、取り消し）。
//! 操作を計画して再生スレッドに適用させる。適用は再生スレッドの返事を待つので、呼び出し側を止めないよう別のスレッドで行う。

use std::sync::Arc;

use mm_ops::{MarkIn, MarkOut, Op};

use crate::win::edit;
use crate::win::playback::Command;
use crate::win::sequence_edit::{self as plan, Ctx, Planned};
use crate::win::source::SourceHandle;
use crate::win::state::Shared;
use crate::win::timebase;

/// 名前のついた編集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    /// Source の今の位置を素材のイン点・アウト点にする。
    MarkIn,
    MarkOut,
    /// Source の範囲を再生位置に近い編集点に差し込む・選んだカットに重ねる。
    Insert,
    Overwrite,
    /// 再生位置でカットを割る。
    Split,
    /// 選んだカットを隙間にする・抜いて詰める。
    Lift,
    Extract,
    Undo,
    Redo,
}

impl EditAction {
    /// WebView のボタンの名前から。
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "mark_in" => Self::MarkIn,
            "mark_out" => Self::MarkOut,
            "insert" => Self::Insert,
            "overwrite" => Self::Overwrite,
            "split" => Self::Split,
            "lift" => Self::Lift,
            "extract" => Self::Extract,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            _ => return None,
        })
    }
}

/// 編集を実行する。計画と適用は編集のキュー（`edit::queue`）で順に行い、呼び出し側を止めない。
pub fn run(shared: &Arc<Shared>, action: EditAction) {
    let s = shared.clone();
    let job: Box<dyn FnOnce() + Send> = match (action, shared.core.get().is_some()) {
        (EditAction::Undo | EditAction::Redo, false) => {
            let undo = action == EditAction::Undo;
            return shared.send(if undo { Command::Undo } else { Command::Redo });
        }
        (EditAction::Undo | EditAction::Redo, true) => Box::new(move || {
            if let Some(core) = s.core.get() {
                edit::step_saved(&s, core, action == EditAction::Undo);
            }
        }),
        _ => Box::new(move || {
            let result = plan_and_apply(&s, action);
            s.edit.set_message(result.err().unwrap_or_default());
        }),
    };
    edit::queue(shared, job);
}

/// 編集を計画して適用し、成功したら新しいカットを選ぶ。失敗は文言で返す。
/// 計画に使ったデータのバージョンを付けて適用するので、間に CLI などが同じ所を触っていたら衝突で断られる。
pub fn plan_and_apply(shared: &Shared, action: EditAction) -> Result<(), String> {
    let doc = shared.doc().ok_or("プロジェクトを開いていない")?;
    let comp = doc
        .project
        .composition(&doc.comp)
        .map_err(|e| e.to_string())?;
    let layer = shared.selected_layer();
    let cut = shared.edit.selected_cut();
    let ctx = Ctx {
        project: &doc.project,
        comp,
        layer: layer.as_deref(),
        cut: cut.as_deref(),
        playhead: shared.playhead(),
    };
    let planned = plan_action(shared, &ctx, action)?;
    edit::submit_based(shared, vec![planned.op], doc.version)?;
    // イン点・アウト点は素材の整理で、Timeline で選んだカットには関係しない。
    if !matches!(action, EditAction::MarkIn | EditAction::MarkOut) {
        shared
            .edit
            .select_cut(planned.select.map(|id| id.to_string()));
    }
    Ok(())
}

fn plan_action(shared: &Shared, ctx: &Ctx<'_>, action: EditAction) -> Result<Planned, String> {
    let new_id = || shared.edit.fresh_cut_id(ctx.project);
    match action {
        EditAction::MarkIn | EditAction::MarkOut => mark(shared, action == EditAction::MarkIn),
        EditAction::Insert => plan::plan_insert(ctx, &source_range(shared, ctx)?, new_id()),
        EditAction::Overwrite => plan::plan_overwrite(ctx, &source_range(shared, ctx)?, new_id()),
        EditAction::Split => plan::plan_split(ctx, new_id()),
        EditAction::Lift => plan::plan_lift(ctx),
        EditAction::Extract => plan::plan_extract(ctx),
        EditAction::Undo | EditAction::Redo => Err("履歴の操作は計画しない".into()),
    }
}

fn open_source(shared: &Shared) -> Result<Arc<SourceHandle>, String> {
    shared
        .source
        .handle()
        .ok_or_else(|| "Source に素材を開いていない".to_owned())
}

fn source_range(shared: &Shared, ctx: &Ctx<'_>) -> Result<plan::SourceRange, String> {
    plan::source_range(ctx.project, &open_source(shared)?.asset)
}

/// Source の今の位置（素材のフレーム）にイン点・アウト点を付ける操作。
fn mark(shared: &Shared, is_in: bool) -> Result<Planned, String> {
    let h = open_source(shared)?;
    let at = timebase::frame_start(h.shared.playhead(), h.fps)?;
    let (asset, at) = (h.asset.clone(), Some(at));
    let op: Op = if is_in {
        MarkIn { asset, at }.into()
    } else {
        MarkOut { asset, at }.into()
    };
    Ok(Planned { op, select: None })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use mm_runtime::{Backend, RuntimeConfig};
    use mm_types::{Id, RgbaImage};

    use super::*;
    use mm_ui_layout::{EditKind, Side};

    use crate::win::playback::{Playback, PlayerOptions};
    use crate::win::sequence_edit::tests::sample_project;
    use crate::win::sequence_edit::{gesture_op, TimelineIds};
    use crate::win::state::FrameImage;

    /// 映像のデコードだけモックにして、描画は実際の wgpu のレンダラー（シーケンスを展開する）にする。
    fn options() -> PlayerOptions {
        PlayerOptions {
            no_audio: true,
            config: Some(RuntimeConfig {
                render: Backend::Wgpu,
                decode: Backend::Sample,
                encode: Backend::Sample,
                present: true,
                ..RuntimeConfig::real()
            }),
            store_dir: None,
        }
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// `shared` の再生位置を `t` にして、その時刻のフレームの CPU の画像を待つ。
    fn frame_at(shared: &Shared, t: f64) -> RgbaImage {
        // フレームの境界ぴったりの秒は浮動小数点の丸めで 1 つ手前のフレームになりうるので、少し先を指す。
        shared.send(Command::Seek(t + 1e-4));
        let mut found = None;
        assert!(
            wait_until(Duration::from_secs(10), || {
                found = shared
                    .frames
                    .newer_than(0)
                    .filter(|f| (f.time - t).abs() < 1e-6);
                found.is_some()
            }),
            "{t} 秒のフレームが出る"
        );
        match found.expect("フレーム").image {
            FrameImage::Cpu(image) => (*image).clone(),
            FrameImage::Gpu(_) => panic!("cpu_frames を立てたのに GPU のフレーム"),
        }
    }

    /// モックのデコーダーが G に焼き込んだ素材のフレーム番号を、中央の画素から読む。
    fn source_frame_of(image: &RgbaImage) -> i64 {
        let p = image.pixel(image.width() / 2, image.height() / 2);
        mm_testkit::decode::frame_index_of(p)
    }

    /// 試験用のプロジェクトを開いた再生スレッド。フレームは CPU の画像で受ける。
    fn start() -> (Arc<Shared>, Playback) {
        let shared = Arc::new(Shared::new());
        shared.cpu_frames.store(true, Ordering::SeqCst);
        shared.source.configure(options());
        let project = Arc::new(sample_project());
        let (playback, _) = Playback::spawn(shared.clone(), project, Id::new("main"), options())
            .expect("再生を始める");
        assert!(wait_until(Duration::from_secs(10), || shared
            .doc()
            .is_some()));
        (shared, playback)
    }

    fn cut_count(shared: &Shared) -> usize {
        shared
            .doc()
            .map_or(0, |d| d.project.sequences[0].cuts.len())
    }

    fn wait_playhead(shared: &Shared, t: f64) -> bool {
        wait_until(Duration::from_secs(5), || {
            (shared.playhead() - t).abs() < 0.05
        })
    }

    #[test]
    fn source_の範囲を_insert_するとカットが増え_再生でその素材のその位置が出る() {
        let (shared, _playback) = start();

        // 挿入の前の 3.5 秒は k2（素材の 3 秒から）の 0.5 秒目 = 105 フレーム。
        let before = source_frame_of(&frame_at(&shared, 3.5));
        assert!((before - 105).abs() <= 1, "挿入前 {before}");

        // Source で素材 a を開き、5 秒にイン点、6 秒にアウト点を付ける。
        let doc = shared.doc().unwrap();
        let comp = doc.project.composition(&doc.comp).unwrap();
        shared
            .source
            .open(&doc.project, comp, "a")
            .expect("Source を開く");
        let source = shared.source.handle().expect("開いた");
        assert!(wait_until(Duration::from_secs(10), || source
            .shared
            .frames
            .newer_than(0)
            .is_some()));
        source.shared.send(Command::Seek(5.0));
        assert!(wait_playhead(&source.shared, 5.0));
        plan_and_apply(&shared, EditAction::MarkIn).expect("イン点");
        source.shared.send(Command::Seek(6.0));
        assert!(wait_playhead(&source.shared, 6.0));
        plan_and_apply(&shared, EditAction::MarkOut).expect("アウト点");

        // 再生位置 3.2 秒（k2 の前半）で insert すると、k1 と k2 の間に入る。
        shared.send(Command::Seek(3.2));
        assert!(wait_playhead(&shared, 3.2));
        assert_eq!(cut_count(&shared), 3);
        plan_and_apply(&shared, EditAction::Insert).expect("insert");
        assert_eq!(cut_count(&shared), 4, "Timeline のカットが増える");
        let inserted = shared.edit.selected_cut().expect("新しいカットを選ぶ");

        // 3.5 秒は挿入したカット（3 秒から。素材の 5 秒から）の 0.5 秒目 = 165 フレーム。
        let after = source_frame_of(&frame_at(&shared, 3.5));
        assert!(
            (after - 165).abs() <= 1,
            "挿入後 {after}（挿入前は {before}）"
        );
        // 後ろにずれた k2 は 4 秒から。4.5 秒は k2 の 0.5 秒目 = 105 フレームに戻る。
        let shifted = source_frame_of(&frame_at(&shared, 4.5));
        assert!((shifted - 105).abs() <= 1, "後ろにずれた k2 {shifted}");

        // 取り消すと元のカットの並びに戻り、その位置のフレームも戻る。
        run(&shared, EditAction::Undo);
        assert!(wait_until(Duration::from_secs(5), || cut_count(&shared) == 3));
        assert!(shared
            .doc()
            .unwrap()
            .project
            .cut(&Id::new(inserted.as_str()))
            .is_err());
        let undone = source_frame_of(&frame_at(&shared, 3.5));
        assert!((undone - 105).abs() <= 1, "取り消した後 {undone}");
    }

    #[test]
    fn ドラッグの編集を適用すると_触ったカットのフレームのキャッシュが捨てられ_絵が変わる() {
        let (shared, _playback) = start();
        let ids = TimelineIds {
            layers: &["l-seq".to_owned()],
            cuts: &[vec!["k1".to_owned(), "k2".to_owned(), "k3".to_owned()]],
        };
        let apply = |kind: EditKind, by: f64| {
            let doc = shared.doc().unwrap();
            let comp = doc.project.composition(&doc.comp).unwrap();
            let op = gesture_op(kind, by, &ids, comp).expect("操作になる");
            edit::submit(&shared, vec![op]).expect("適用できる");
        };
        // k1（素材の 0 秒から）の 0.5 秒目は 15 フレーム。見せてキャッシュに載せる。
        assert_eq!(source_frame_of(&frame_at(&shared, 0.5)), 15);
        // slip で中身を 0.5 秒後ろへずらすと、同じ位置が 30 フレームになる。
        apply(EditKind::Slip { item: 0, cut: 0 }, 0.5);
        assert_eq!(source_frame_of(&frame_at(&shared, 0.5)), 30);
        // k2 は 3 秒から。2.5 秒には k1 が居るが、k1 の出を 1 秒縮める ripple で k2 が 2 秒から始まる。
        assert_eq!(source_frame_of(&frame_at(&shared, 2.5)), 90);
        apply(
            EditKind::Ripple {
                item: 0,
                cut: 0,
                side: Side::Out,
            },
            -1.0,
        );
        assert_eq!(source_frame_of(&frame_at(&shared, 2.5)), 105);
        // roll で k1 と k2 の編集点を 0.5 秒後ろへ動かすと、2.3 秒は k1 の延びた分（素材の 2.8 秒）になる。
        assert_eq!(source_frame_of(&frame_at(&shared, 2.3)), 99);
        apply(EditKind::Roll { item: 0, cut: 0 }, 0.5);
        assert_eq!(source_frame_of(&frame_at(&shared, 2.3)), 84);
    }
}
