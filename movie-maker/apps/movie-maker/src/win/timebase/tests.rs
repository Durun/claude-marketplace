use mm_doc::Node;

use super::*;
use crate::win::sequence_edit::tests::sample_project;

/// シーケンスのレイヤーを 2 倍速にし、シーケンスの 1 秒から 5 秒だけを、コンポジションの 10 秒から映す。
fn fast_project() -> Project {
    let mut p = sample_project();
    let layer = &mut p.compositions[0].layers[0];
    layer.speed = Rational::integer(2);
    layer.start = RationalTime::seconds(10);
    layer.in_point = RationalTime::seconds(1);
    layer.out_point = RationalTime::seconds(5);
    p
}

#[test]
fn フレームの後半の秒も表示と同じフレームに数える() {
    let fps = Rational::integer(30);
    assert_eq!(frame_index(1.0 / 30.0 * 0.9, 30.0), 0);
    assert_eq!(
        frame_start(0.99 / 30.0, fps).unwrap(),
        RationalTime::ZERO,
        "round だと次のフレームに付く"
    );
    assert_eq!(
        frame_start(1.5 / 30.0, fps).unwrap(),
        RationalTime::from_frames(1, fps).unwrap()
    );
}

#[test]
fn 素材の時刻はレイヤーの_speed_と_in_を通して再生位置になる() {
    let p = fast_project();
    let comp = &p.compositions[0];
    // k1 の素材の 2 秒 = シーケンスの 2 秒。レイヤーの in 1 秒から 2 倍速なので 10 + (2 - 1) / 2 = 10.5 秒。
    assert_eq!(source_to_comp(&p, comp, "a", 2.0), Some(10.5));
}

#[test]
fn レイヤーの_in_と_out_の外の素材の時刻は映らない() {
    let p = fast_project();
    let comp = &p.compositions[0];
    // シーケンスの 0.5 秒（in より前）と 6 秒（out 以降。k3 の素材の 6 秒）。
    assert_eq!(source_to_comp(&p, comp, "a", 0.5), None);
    assert_eq!(source_to_comp(&p, comp, "a", 6.0), None);
}

#[test]
fn 無効なカットは配置に数えない() {
    let mut p = sample_project();
    p.sequences[0].cuts[0].enabled = false;
    let comp = p.compositions[0].clone();
    // k1 を外すと k2（素材の 3-4 秒）が先頭に来る。
    assert_eq!(source_to_comp(&p, &comp, "a", 3.5), Some(0.5));
    assert_eq!(
        source_to_comp(&p, &comp, "a", 1.0),
        None,
        "外した k1 は映らない"
    );
}

#[test]
fn 撮影素材のレイヤーも_out_以降は映らない() {
    let mut p = sample_project();
    let node = Node::footage(&Id::new("a"));
    let comp = &mut p.compositions[0];
    comp.layers = vec![Layer::new(
        "l-a",
        node,
        RationalTime::seconds(2),
        RationalTime::seconds(1),
        RationalTime::seconds(3),
    )];
    let comp = p.compositions[0].clone();
    assert_eq!(source_to_comp(&p, &comp, "a", 1.5), Some(2.5));
    assert_eq!(source_to_comp(&p, &comp, "a", 3.0), None);
}

#[test]
fn カットの始まりの再生位置() {
    let p = fast_project();
    let comp = &p.compositions[0];
    // k2 はシーケンスの 3 秒から = 10 + (3 - 1) / 2 = 11 秒。k1 は in より前から始まるので in の所（10 秒）。
    assert_eq!(cut_to_comp(&p, comp, "k2"), Some(11.0));
    assert_eq!(cut_to_comp(&p, comp, "k1"), Some(10.0));
    assert_eq!(cut_to_comp(&p, comp, "k3"), Some(11.5));
    assert_eq!(cut_to_comp(&p, comp, "none"), None);
}

#[test]
fn ドラッグの量はレイヤーの_speed_倍のシーケンスの長さになる() {
    let p = fast_project();
    let half = RationalTime::new(1, 2).unwrap();
    assert_eq!(
        layer_inner_delta(&p.compositions[0].layers[0], half).unwrap(),
        RationalTime::seconds(1)
    );
}
