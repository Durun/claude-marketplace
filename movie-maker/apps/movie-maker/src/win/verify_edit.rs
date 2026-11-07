//! `--play-test` の Timeline 編集の通し。実際のウィンドウへのキーとドラッグで、Source を開いてイン点・アウト点を付け、
//! insert、端のドラッグ（ripple）、中のドラッグ（slip）、split、lift、取り消しを順に行い、そのたびに
//! プロジェクトのデータが期待どおり変わったかを確かめて、段ごとにスクリーンショットを残す。

use std::sync::Arc;
use std::time::Duration;

use mm_doc::{Cut, Project, Sequence, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_types::{Id, ParamValue};
use mm_ui_layout::{row_top, split_viewers, TimeView, ROW_H, VIEW_MARGIN};
use mm_ui_wgpu::timeline_model;
use serde_json::{json, Value};

use crate::win::capture;
use crate::win::cli::PlayTest;
use crate::win::keys;
use crate::win::playback::Command;
use crate::win::state::Shared;
use crate::win::surface;
use crate::win::verify::wait_until;

/// 仮想キーコード。
const VK_I: u32 = 0x49;
const VK_O: u32 = 0x4F;
const VK_B: u32 = 0x42;
const VK_COMMA: u32 = 0xBC;
const VK_DELETE: u32 = 0x2E;
/// 入力を投げてから結果を見るまでの待ち。
const SETTLE: Duration = Duration::from_millis(300);
/// 結果が出るのを待つ上限。
const WAIT: Duration = Duration::from_secs(5);
/// Source に付けるイン点・アウト点（秒）。
const MARK_IN: f64 = 0.5;
const MARK_OUT: f64 = 1.5;
/// insert する再生位置（先頭のカットの後半）。
const INSERT_AT: f64 = 0.8;
/// 端と中のドラッグの量（秒）。
const DRAG_SECS: f64 = 0.25;
/// 端のつまみの中心を掴むための x の内側へのずれ（px）。
const HANDLE_INSET: f32 = 3.0;

/// 試験の対象。
pub struct Env<'a> {
    pub shared: &'a Arc<Shared>,
    pub surface_hwnd: isize,
    pub window_hwnd: isize,
    pub test: &'a PlayTest,
}

/// 結果の記録。段ごとの合否と、撮ったスクリーンショット。
#[derive(Default)]
struct Log {
    steps: Vec<(String, Value)>,
    shots: Vec<String>,
}

impl Log {
    fn step(&mut self, name: &str, ok: bool, detail: Value) -> bool {
        self.steps
            .push((name.to_owned(), json!({ "ok": ok, "detail": detail })));
        ok
    }

    fn into_json(self) -> Value {
        let all_ok = self.steps.iter().all(|(_, v)| v["ok"] == true);
        let steps: serde_json::Map<String, Value> = self.steps.into_iter().collect();
        json!({ "all_ok": all_ok, "steps": steps, "screenshots": self.shots })
    }
}

fn sequence_of(project: &Project) -> Option<&Sequence> {
    let comp = project.compositions.first()?;
    let layer = comp
        .layers
        .iter()
        .find(|l| l.source.kind == SEQUENCE_KIND)?;
    project
        .sequence(&Id::new(layer.source.text_param(SEQUENCE_PARAM)?))
        .ok()
}

fn cuts(shared: &Shared) -> Vec<Cut> {
    shared
        .doc()
        .and_then(|d| sequence_of(&d.project).map(|s| s.cuts.clone()))
        .unwrap_or_default()
}

fn cut_named(shared: &Shared, id: &str) -> Option<Cut> {
    cuts(shared).into_iter().find(|c| c.id.as_str() == id)
}

fn shot(env: &Env<'_>, log: &mut Log, name: &str) {
    let Some(base) = &env.test.screenshot else {
        return;
    };
    std::thread::sleep(SETTLE);
    let path = base.with_extension(format!("edit-{name}.png"));
    match capture::capture_window(env.window_hwnd, &path) {
        Ok(()) => log.shots.push(path.display().to_string()),
        Err(e) => log.shots.push(format!("失敗: {e}")),
    }
}

/// 画面の位置の計算に要るもの。Timeline の矩形と時間軸、シーケンスの行の添字。
struct Geometry {
    tl_x: f32,
    tl_y: f32,
    view: TimeView,
    row: usize,
}

fn geometry(shared: &Shared) -> Option<Geometry> {
    let doc = shared.doc()?;
    let comp = doc.project.composition(&doc.comp).ok()?;
    let model = timeline_model(&doc.project, comp);
    let (w, h) = shared.surface_size();
    let tl = split_viewers(w as f32, h as f32, model.content_height(), true).timeline;
    let row = model.items.iter().position(|i| !i.cuts.is_empty())?;
    Some(Geometry {
        tl_x: tl.x,
        tl_y: tl.y,
        view: TimeView::fit(model.duration, tl.w, VIEW_MARGIN),
        row,
    })
}

impl Geometry {
    /// 時刻 `t` の画面の x。
    fn x(&self, t: f64) -> i32 {
        (self.tl_x + self.view.x_of(t)) as i32
    }

    /// シーケンスの行の中央の y。
    fn y(&self) -> i32 {
        (self.tl_y + row_top(self.row) + ROW_H / 2.0) as i32
    }
}

/// カット `id` の Timeline の上の範囲（秒）。
fn cut_span_secs(shared: &Shared, id: &str) -> Option<(f64, f64)> {
    let doc = shared.doc()?;
    let comp = doc.project.composition(&doc.comp).ok()?;
    let ids = mm_ui_wgpu::timeline_cut_ids(&doc.project, comp);
    let model = timeline_model(&doc.project, comp);
    ids.iter().enumerate().find_map(|(item, list)| {
        let k = list.iter().position(|c| c == id)?;
        let bar = model.items.get(item)?.cuts.get(k)?;
        Some((bar.start, bar.end))
    })
}

fn seek_main(shared: &Shared, t: f64) -> bool {
    shared.send(Command::Seek(t));
    wait_until(WAIT, || (shared.playhead() - t).abs() < 0.05)
}

/// Source を開いてイン点・アウト点を付ける。
fn mark_source(env: &Env<'_>, log: &mut Log) -> Option<String> {
    let shared = env.shared;
    let doc = shared.doc()?;
    let comp = doc.project.composition(&doc.comp).ok()?;
    let asset = doc.project.assets.last()?.id.to_string();
    let opened = shared.source.open(&doc.project, comp, &asset);
    let source = shared.source.handle();
    let framed = source
        .as_ref()
        .is_some_and(|s| wait_until(WAIT, || s.shared.frames.newer_than(0).is_some()));
    log.step(
        "source_open",
        opened.is_ok() && framed,
        json!({ "asset": asset }),
    );
    let source = source?;
    for (t, vk) in [(MARK_IN, VK_I), (MARK_OUT, VK_O)] {
        source.shared.send(Command::Seek(t));
        wait_until(WAIT, || (source.shared.playhead() - t).abs() < 0.05);
        surface::post_key(env.surface_hwnd, vk);
        std::thread::sleep(SETTLE);
    }
    let marked =
        wait_until(WAIT, || {
            let d = shared.doc();
            d.is_some_and(|d| {
                d.project.bins.marks.iter().any(|m| {
                    m.in_point.is_some() && m.out_point.is_some() && m.asset.as_str() == asset
                })
            })
        });
    log.step(
        "source_marks",
        marked,
        json!({ "in": MARK_IN, "out": MARK_OUT }),
    );
    shot(env, log, "1-source");
    Some(asset)
}

/// insert のキーで、Source の範囲が再生位置のカットの編集点に入る。入ったカットの ID を返す。
fn insert_step(env: &Env<'_>, log: &mut Log, asset: &str) -> Option<String> {
    let shared = env.shared;
    let before = cuts(shared).len();
    seek_main(shared, INSERT_AT);
    surface::post_key(env.surface_hwnd, VK_COMMA);
    let grew = wait_until(WAIT, || cuts(shared).len() == before + 1);
    let id = shared.edit.selected_cut();
    let cut = id.as_deref().and_then(|i| cut_named(shared, i));
    let ranged = cut.as_ref().is_some_and(|c| {
        (c.in_point.to_f64() - MARK_IN).abs() < 1e-6
            && (c.out_point.to_f64() - MARK_OUT).abs() < 1e-6
            && c.source
                .params
                .get("asset")
                .is_some_and(|p| matches!(&p.value, ParamValue::AssetRef(a) if a.as_str() == asset))
    });
    log.step(
        "insert",
        grew && ranged,
        json!({ "cuts_before": before, "cuts_after": cuts(shared).len(), "cut": id }),
    );
    shot(env, log, "2-insert");
    id
}

/// 挿入したカットの出つまみを引いて ripple、中を引いて slip。
fn drag_steps(env: &Env<'_>, log: &mut Log, id: &str) {
    let shared = env.shared;
    let (Some(g), Some((start, end))) = (geometry(shared), cut_span_secs(shared, id)) else {
        log.step("drag", false, json!("Timeline の位置を引けない"));
        return;
    };
    let out_before = cut_named(shared, id).map(|c| c.out_point.to_f64());
    let from = (g.x(end) - HANDLE_INSET as i32, g.y());
    let to = (g.x(end - DRAG_SECS) - HANDLE_INSET as i32, g.y());
    surface::post_drag(env.surface_hwnd, from, to);
    let rippled = wait_until(WAIT, || {
        cut_named(shared, id).map(|c| c.out_point.to_f64()) != out_before
    });
    let out_after = cut_named(shared, id).map(|c| c.out_point.to_f64());
    log.step(
        "ripple_drag",
        rippled,
        json!({ "out_before": out_before, "out_after": out_after }),
    );
    shot(env, log, "3-ripple");

    let in_before = cut_named(shared, id).map(|c| c.in_point.to_f64());
    let mid = g.x((start + end - DRAG_SECS) / 2.0);
    surface::post_drag(
        env.surface_hwnd,
        (mid, g.y()),
        (mid + (DRAG_SECS * g.view.px_per_sec / 2.0) as i32, g.y()),
    );
    let slipped = wait_until(WAIT, || {
        cut_named(shared, id).map(|c| c.in_point.to_f64()) != in_before
    });
    let in_after = cut_named(shared, id).map(|c| c.in_point.to_f64());
    log.step(
        "slip_drag",
        slipped,
        json!({ "in_before": in_before, "in_after": in_after }),
    );
    shot(env, log, "4-slip");
}

/// 再生位置で split、選んだカットの lift、取り消し。
fn cut_steps(env: &Env<'_>, log: &mut Log, id: &str) {
    let shared = env.shared;
    let Some((start, end)) = cut_span_secs(shared, id) else {
        log.step("split", false, json!("カットの位置が無い"));
        return;
    };
    let before = cuts(shared).len();
    seek_main(shared, (start + end) / 2.0);
    surface::post_key(env.surface_hwnd, VK_B);
    let split = wait_until(WAIT, || cuts(shared).len() == before + 1);
    log.step(
        "split",
        split,
        json!({ "cuts_before": before, "cuts_after": cuts(shared).len() }),
    );
    shot(env, log, "5-split");

    let before_gaps = cuts(shared).iter().filter(|c| c.source.is_gap()).count();
    surface::post_key(env.surface_hwnd, VK_DELETE);
    let lifted = wait_until(WAIT, || {
        cuts(shared).iter().filter(|c| c.source.is_gap()).count() == before_gaps + 1
    });
    log.step("lift", lifted, json!({ "gaps_before": before_gaps }));
    shot(env, log, "6-lift");

    keys::dispatch(shared, "play-test", "Ctrl+KeyZ");
    let undone = wait_until(WAIT, || {
        cuts(shared).iter().filter(|c| c.source.is_gap()).count() == before_gaps
    });
    log.step("undo_lift", undone, json!({}));
    shot(env, log, "7-undo");
}

/// Timeline 編集の通しを行って結果を返す。シーケンスのレイヤーが無いプロジェクトではスキップする。
pub fn run(env: &Env<'_>) -> Value {
    let mut log = Log::default();
    let has_sequence = env
        .shared
        .doc()
        .is_some_and(|d| sequence_of(&d.project).is_some());
    if !has_sequence {
        return json!({ "skipped": "シーケンスのレイヤーが無い" });
    }
    if let Some(asset) = mark_source(env, &mut log) {
        if let Some(id) = insert_step(env, &mut log, &asset) {
            drag_steps(env, &mut log, &id);
            cut_steps(env, &mut log, &id);
        }
    }
    log.into_json()
}
