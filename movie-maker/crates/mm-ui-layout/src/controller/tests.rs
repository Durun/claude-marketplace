use super::*;
use crate::gesture::Side;
use crate::timeline::{row_top, CutBar, Item, ItemKind};

fn model() -> TimelineModel {
    TimelineModel {
        duration: 10.0,
        fps: 30.0,
        items: vec![
            Item::layer(0.0, 10.0, ItemKind::Text, true),
            Item::layer(2.0, 4.0, ItemKind::Footage, true),
        ],
    }
}

/// 先頭がシーケンスのレイヤー（カット 3 つ: 0-3、3-4、4-7 秒）、次が普通のレイヤー。
fn seq_model() -> TimelineModel {
    let cut = |start, end| CutBar {
        start,
        end,
        gap: false,
        ..CutBar::default()
    };
    let mut seq = Item::layer(0.0, 20.0, ItemKind::Sequence, true);
    seq.cuts = vec![cut(0.0, 3.0), cut(3.0, 4.0), cut(4.0, 7.0)];
    TimelineModel {
        duration: 20.0,
        fps: 30.0,
        items: vec![seq, Item::layer(10.0, 14.0, ItemKind::Text, true)],
    }
}

fn ctl() -> TimelineController {
    TimelineController::new(TimeView {
        origin: 0.0,
        px_per_sec: 100.0,
    })
}

fn down(x: f32, y: f32) -> PointerEvent {
    PointerEvent::Down {
        x,
        y,
        mods: Mods::default(),
    }
}

#[test]
fn 目盛りの上のドラッグは再生位置をフレームの境目で追う() {
    let mut c = ctl();
    let m = model();
    let a = c.handle(down(251.0, 5.0), &m, 0.0);
    assert_eq!(a, vec![Action::Seek(quantize(2.51, 30.0))]);
    let b = c.handle(PointerEvent::Move { x: 5000.0, y: 80.0 }, &m, 0.0);
    let Action::Seek(t) = b[0] else {
        panic!("{b:?}")
    };
    assert!(t < 10.0 && t > 9.9, "終わりの手前で止まる: {t}");
}

#[test]
fn レイヤーのつまみのドラッグは別のレイヤーの端と再生位置に吸い付き_trim_layer_になる() {
    let mut c = ctl();
    let m = model();
    let y = row_top(1) + 4.0;
    // 先頭の帯の終わり(10 秒)ではなく、2 番目の帯の出つまみを掴む。
    let a = c.handle(down(399.0, y), &m, 0.0);
    assert_eq!(a, vec![Action::Select(Some(1))]);
    let b = c.handle(PointerEvent::Move { x: 697.0, y }, &m, 7.0);
    let Action::Edit { kind, by, done } = b[0] else {
        panic!("{b:?}")
    };
    assert_eq!(
        kind,
        EditKind::TrimLayer {
            item: 1,
            side: Side::Out
        }
    );
    assert!((by - 3.0).abs() < 1e-9, "出つまみの 4 秒から 7 秒へ: {by}");
    assert!(!done);
    let done = c.handle(PointerEvent::Up, &m, 7.0);
    assert!(matches!(done[0], Action::Edit { done: true, .. }));
}

#[test]
fn スナップしないドラッグも離したときに確定する() {
    let mut c = ctl();
    let m = model();
    let y = row_top(1) + 4.0;
    c.handle(down(399.0, y), &m, 0.0);
    // どの目標からも遠い所（5.55 秒）で離す。
    let moved = c.handle(PointerEvent::Move { x: 555.0, y }, &m, 0.0);
    let Action::Edit { by, .. } = moved[0] else {
        panic!("{moved:?}")
    };
    let done = c.handle(PointerEvent::Up, &m, 0.0);
    assert_eq!(
        done,
        vec![Action::Edit {
            kind: EditKind::TrimLayer {
                item: 1,
                side: Side::Out
            },
            by,
            done: true
        }]
    );
    assert!(
        c.handle(PointerEvent::Up, &m, 0.0).is_empty(),
        "確定は 1 回だけ"
    );
}

#[test]
fn 動かさずに離したクリックは編集にしない() {
    let mut c = ctl();
    let m = model();
    let y = row_top(1) + 4.0;
    c.handle(down(399.0, y), &m, 0.0);
    c.handle(PointerEvent::Move { x: 399.0, y }, &m, 0.0);
    assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
}

#[test]
fn モデルの並びが変わったら範囲の外の選択とドラッグを捨てる() {
    let mut c = ctl();
    let m = model();
    let y = row_top(1) + 4.0;
    c.handle(down(399.0, y), &m, 0.0);
    assert_eq!(c.selected, Some(1));
    c.model_changed(&TimelineModel {
        items: vec![Item::layer(0.0, 10.0, ItemKind::Text, true)],
        ..m.clone()
    });
    assert_eq!(c.selected, None);
    assert!(
        c.handle(PointerEvent::Up, &m, 0.0).is_empty(),
        "古いドラッグは確定しない"
    );
}

#[test]
fn 本体のクリックは選ぶだけ_空きで選択を外す() {
    let mut c = ctl();
    let m = model();
    let y = row_top(1) + 4.0;
    assert_eq!(
        c.handle(down(300.0, y), &m, 0.0),
        vec![Action::Select(Some(1))]
    );
    assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
    assert_eq!(
        c.handle(down(900.0, y), &m, 0.0),
        vec![Action::Select(None)]
    );
}

#[test]
fn ホイールは_ctrl_で拡大_なしで移動() {
    let mut c = ctl();
    let m = model();
    let t = c.view.t_of(300.0);
    c.handle(
        PointerEvent::Wheel {
            x: 300.0,
            y: 0.0,
            dy: 2.0,
            ctrl: true,
        },
        &m,
        0.0,
    );
    assert!(c.view.px_per_sec > 100.0);
    assert!((c.view.t_of(300.0) - t).abs() < 1e-6);
    let origin = c.view.origin;
    c.handle(
        PointerEvent::Wheel {
            x: 300.0,
            y: 0.0,
            dy: 1.0,
            ctrl: false,
        },
        &m,
        0.0,
    );
    assert!(c.view.origin > origin, "奥へ回すと先の時刻が見える");
}

#[test]
fn ポインターが乗った所を覚え_出たら消す() {
    let mut c = ctl();
    let m = model();
    let y = row_top(1) + 4.0;
    c.handle(PointerEvent::Move { x: 300.0, y }, &m, 0.0);
    assert_eq!(
        c.hover,
        Hit::Layer {
            index: 1,
            part: Part::Body
        }
    );
    c.handle(PointerEvent::Leave, &m, 0.0);
    assert_eq!(c.hover, Hit::Empty);
}

#[test]
fn カットのクリックはレイヤーとカットの両方を選ぶ() {
    let mut c = ctl();
    let m = seq_model();
    let y = row_top(0) + 4.0;
    let a = c.handle(down(150.0, y), &m, 0.0);
    assert_eq!(
        a,
        vec![Action::Select(Some(0)), Action::SelectCut(Some((0, 0)))]
    );
    assert!(c.handle(PointerEvent::Up, &m, 0.0).is_empty());
    let b = c.handle(down(900.0, y), &m, 0.0);
    assert_eq!(
        b,
        vec![Action::Select(Some(0)), Action::SelectCut(None)],
        "カットの外はレイヤー"
    );
}

#[test]
fn カットの出つまみのドラッグは_ripple_離したときに確定() {
    let mut c = ctl();
    let m = seq_model();
    let y = row_top(0) + 4.0;
    c.handle(down(298.0, y), &m, 0.0);
    let moved = c.handle(PointerEvent::Move { x: 250.0, y }, &m, 0.0);
    assert_eq!(
        moved,
        vec![Action::Edit {
            kind: EditKind::Ripple {
                item: 0,
                cut: 0,
                side: Side::Out
            },
            by: -0.5,
            done: false
        }]
    );
    let done = c.handle(PointerEvent::Up, &m, 0.0);
    assert!(matches!(done[0], Action::Edit { done: true, by, .. } if (by + 0.5).abs() < 1e-9));
}

#[test]
fn alt_を押してつかんだカットの端は_roll() {
    let mut c = ctl();
    let m = seq_model();
    let y = row_top(0) + 4.0;
    c.handle(
        PointerEvent::Down {
            x: 298.0,
            y,
            mods: Mods {
                shift: false,
                alt: true,
            },
        },
        &m,
        0.0,
    );
    let moved = c.handle(PointerEvent::Move { x: 270.0, y }, &m, 0.0);
    let Action::Edit { kind, by, .. } = moved[0] else {
        panic!("{moved:?}")
    };
    assert_eq!(kind, EditKind::Roll { item: 0, cut: 0 });
    assert!((by + 0.3).abs() < 1e-9, "{by}");
}

#[test]
fn カットの中のドラッグは_slip_で_shift_なら_slide() {
    let m = seq_model();
    let y = row_top(0) + 4.0;
    let mut c = ctl();
    c.handle(down(100.0, y), &m, 0.0);
    let moved = c.handle(PointerEvent::Move { x: 140.0, y }, &m, 0.0);
    let Action::Edit { kind, by, .. } = moved[0] else {
        panic!("{moved:?}")
    };
    assert_eq!(kind, EditKind::Slip { item: 0, cut: 0 });
    assert!((by + 0.4).abs() < 1e-9, "{by}");
    c.handle(PointerEvent::Up, &m, 0.0);

    c.handle(
        PointerEvent::Down {
            x: 350.0,
            y,
            mods: Mods {
                shift: true,
                alt: false,
            },
        },
        &m,
        0.0,
    );
    let moved = c.handle(PointerEvent::Move { x: 380.0, y }, &m, 0.0);
    let Action::Edit { kind, by, .. } = moved[0] else {
        panic!("{moved:?}")
    };
    assert_eq!(kind, EditKind::Slide { item: 0, cut: 1 });
    assert!((by - 0.3).abs() < 1e-9, "{by}");
}

#[test]
fn カットの端は他のカットの端に吸い付く() {
    let mut c = ctl();
    let m = seq_model();
    let y = row_top(0) + 4.0;
    // カット 0 の出つまみを 6.97 秒付近へ。カット 2 の出（7 秒）に吸い付く。
    c.handle(down(298.0, y), &m, 0.0);
    let moved = c.handle(PointerEvent::Move { x: 697.0, y }, &m, 0.0);
    let Action::Edit { by, .. } = moved[0] else {
        panic!("{moved:?}")
    };
    assert!((by - 4.0).abs() < 1e-9, "7 秒へ: {by}");
}

#[test]
fn カットからレイヤーへ選び直すとカットの選択が外れる() {
    let mut c = ctl();
    let m = seq_model();
    c.handle(down(150.0, row_top(0) + 4.0), &m, 0.0);
    c.handle(PointerEvent::Up, &m, 0.0);
    let a = c.handle(down(1200.0, row_top(1) + 4.0), &m, 0.0);
    assert_eq!(a, vec![Action::Select(Some(1)), Action::SelectCut(None)]);
}
