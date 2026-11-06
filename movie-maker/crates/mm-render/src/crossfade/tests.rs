use mm_doc::{Asset, Node, Transition};
use mm_types::{ColorSpace, Id, ParamValue};

use super::*;

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

fn at(num: i128, den: u32) -> RationalTime {
    RationalTime::new(num, den).unwrap()
}

/// 2 秒のカットが 3 つ。2 番目と 3 番目の頭に、長さ `d` 秒のトランジション。
fn three_cuts(d: i64) -> Sequence {
    let cut = |id: &str| Cut::new(id, Node::new("solid", 1), secs(0), secs(2));
    let mut cuts = vec![cut("a"), cut("b"), cut("c")];
    for c in &mut cuts[1..] {
        c.transition_in = Some(Transition {
            node: Node::new("transition.dissolve", 1),
            duration: secs(d),
        });
    }
    Sequence {
        id: Id::new("s"),
        cuts,
        origin: None,
    }
}

fn fade_with(
    seq: &Sequence,
    t: RationalTime,
    handles: &dyn Fn(&Cut) -> Handles,
) -> Option<Crossfade> {
    let placed = place_cuts(seq).unwrap();
    let pos = placed.iter().position(|p| p.range.contains(t)).unwrap();
    crossfade_at(seq, &placed, pos, t, handles).unwrap()
}

fn fade(seq: &Sequence, t: RationalTime) -> Option<Crossfade> {
    fade_with(seq, t, &|_| Handles::UNLIMITED)
}

#[test]
fn 境から離れた時刻は掛からない() {
    let seq = three_cuts(1);
    assert_eq!(fade(&seq, secs(1)), None);
    assert_eq!(fade(&seq, at(11, 8)), None);
}

#[test]
fn 境の前は前のカットから次のカットへ進む() {
    let f = fade(&three_cuts(1), at(7, 4)).unwrap();
    assert_eq!((f.from, f.to), (0, 1));
    assert!((f.progress - 0.25).abs() < 1e-6, "{f:?}");
}

#[test]
fn 境ちょうどは半分で混ざる() {
    let f = fade(&three_cuts(1), secs(2)).unwrap();
    assert_eq!((f.from, f.to), (0, 1));
    assert!((f.progress - 0.5).abs() < 1e-6, "{f:?}");
}

#[test]
fn 境の後は次のカットの頭で終わる() {
    let f = fade(&three_cuts(1), at(9, 4)).unwrap();
    assert_eq!((f.from, f.to), (0, 1));
    assert!((f.progress - 0.75).abs() < 1e-6, "{f:?}");
    assert_eq!(fade(&three_cuts(1), at(5, 2)), None);
}

#[test]
fn 最初のカットの頭にはトランジションが掛からない() {
    let mut seq = three_cuts(1);
    seq.cuts[0].transition_in = seq.cuts[1].transition_in.clone();
    assert_eq!(fade(&seq, secs(0)), None);
}

#[test]
fn トランジションの半分は片側だけのカットの長さを超えない() {
    // 最初のカットは頭にトランジションが無いので、境の前の 2 秒の全部まで延びる。
    let mut seq = three_cuts(10);
    seq.cuts[2].transition_in = None;
    let f = fade(&seq, secs(1)).unwrap();
    assert_eq!((f.from, f.to), (0, 1));
    assert!((f.progress - 0.25).abs() < 1e-6, "{f:?}");
}

#[test]
fn 両端にトランジションがあるカットは長さの半分ずつに分け_絵が飛ばない() {
    // b の両端に 10 秒のトランジション。b の 2 秒を 1 秒ずつ分けるので、a→b は t=3 で終わり、b→c は t=3 から始まる。
    let seq = three_cuts(10);
    let before = fade(&seq, at(39, 10)).unwrap();
    assert_eq!((before.from, before.to), (1, 2));
    assert!((before.progress - 0.45).abs() < 1e-6, "{before:?}");
    let ending = fade(&seq, at(299, 100)).unwrap();
    assert_eq!((ending.from, ending.to), (0, 1));
    assert!(ending.progress > 0.99, "{ending:?}");
    let starting = fade(&seq, secs(3)).unwrap();
    assert_eq!((starting.from, starting.to), (1, 2));
    assert!(starting.progress.abs() < 1e-6, "{starting:?}");
}

#[test]
fn 無効にしたカットを飛ばした隣と混ざる() {
    let mut seq = three_cuts(1);
    seq.cuts[1].enabled = false;
    let f = fade(&seq, secs(2)).unwrap();
    assert_eq!((f.from, f.to), (0, 1), "配置した並びの中の番号");
}

#[test]
fn 半分の長さは延ばす側の素材の余白で切る() {
    // 入ってくるカット（b）のイン点の前に 0.25 秒しか素材が無い。
    let seq = three_cuts(1);
    let handles = |cut: &Cut| match cut.id.as_str() {
        "b" => Handles {
            head: 0.25,
            tail: f64::INFINITY,
        },
        _ => Handles::UNLIMITED,
    };
    assert_eq!(fade_with(&seq, at(3, 2), &handles), None, "境の 0.25 秒より前");
    let f = fade_with(&seq, secs(2), &handles).unwrap();
    assert!((f.progress - 0.5).abs() < 1e-6, "{f:?}");
    let none = |_: &Cut| Handles {
        head: 0.0,
        tail: 0.0,
    };
    assert_eq!(fade_with(&seq, secs(2), &none), None, "余白が無ければ掛けない");
}

#[test]
fn 素材の余白はイン点の前と出の後ろ_速さで割る() {
    let mut project = Project::new();
    project.assets.push(Asset {
        id: Id::new("clip"),
        path: "clip.mp4".into(),
        content_hash: "h".into(),
        color_space: ColorSpace::bt709(),
        duration: secs(10),
        fps: None,
        timebase_offset: RationalTime::ZERO,
    });
    let node =
        Node::new("footage", 1).with_param("asset", ParamValue::AssetRef(Id::new("clip")));
    let mut cut = Cut::new("a", node, secs(0), secs(4));
    assert_eq!(
        handles_in(&project, &cut),
        Handles {
            head: 0.0,
            tail: 6.0
        }
    );
    cut.in_point = secs(2);
    cut.speed = mm_types::Rational::integer(2);
    assert_eq!(
        handles_in(&project, &cut),
        Handles {
            head: 1.0,
            tail: 3.0
        }
    );
    let solid = Cut::new("s", Node::new("solid", 1), secs(0), secs(1));
    assert_eq!(handles_in(&project, &solid), Handles::UNLIMITED);
}
