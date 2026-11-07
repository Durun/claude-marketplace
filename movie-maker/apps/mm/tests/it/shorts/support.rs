//! 本編とショートの通しのプロジェクトの組み立てと、ショートの画素から本編の画素への写像。

use std::path::{Path, PathBuf};
use std::process::Command;

use mm_doc::{
    Asset, Composition, Cut, Interpolation, Keyframe, Layer, Node, OriginRelation, Param, Project,
    RenderTarget, Sequence, Transform, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_eval::{cut_source_time, place_cuts};
use mm_ops::{Edge, ForkSequence, Op, OpContext, OverrideCut, Ripple, SetReframe};
use mm_types::{AssetId, ColorSpace, Id, ParamValue, Rational, RationalTime};

/// 本編（16:9）とショート（9:16）の大きさ。素材は本編と同じ大きさ。
pub const MAIN_SIZE: (u32, u32) = (320, 180);
pub const SHORT_SIZE: (u32, u32) = (180, 320);
const FPS: u32 = 30;
const CLIP_SECONDS: u32 = 6;
/// 再フレーミングの拡大。縦の画布に収めた素材（高さ 101 画素）が画布の高さを少し超える倍率。
const REFRAME_SCALE: f64 = 3.3;
/// r0 の切り出しの横のずれ（画素）。
const R0_SHIFT: f64 = -30.0;
/// r1 の切り出しは素材の 3.5 秒から 5.5 秒で、横に -60 から 60 画素へ動く。
const R1_FROM: (i64, u32, f64) = (7, 2, -60.0);
const R1_TO: (i64, u32, f64) = (11, 2, 60.0);

/// `mm` を実行して標準出力を返す。失敗したら止める。
pub fn mm(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mm"))
        .args(args)
        .output()
        .expect("mm を実行できる");
    assert!(
        out.status.success(),
        "mm {args:?} が失敗した: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 横と縦と時刻で滑らかに変わる画像の素材を作る。切り出しの位置と時刻の取り違えが差に出る。
fn make_clip(path: &Path) {
    let (w, h) = MAIN_SIZE;
    let video = format!(
        "color=c=black:s={w}x{h}:r={FPS},format=yuv444p,\
         geq=lum='128+90*sin(X/17+N/4)*cos(Y/13)':cb='128+60*sin(Y/23-N/6)':cr='128+60*cos(X/29+N/5)',\
         format=yuv420p,trim=end_frame={}",
        CLIP_SECONDS * FPS
    );
    let audio = format!("sine=frequency=440:sample_rate=48000:duration={CLIP_SECONDS}");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &video,
        ])
        .args(["-f", "lavfi", "-i", &audio])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "10"])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(path)
        .status()
        .expect("ffmpeg を実行できる");
    assert!(status.success(), "素材を作れた");
}

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

fn half(n: i64) -> RationalTime {
    RationalTime::new(i128::from(n), 2).unwrap()
}

fn apply(p: &mut Project, op: impl Into<Op>) {
    let op = op.into();
    op.apply(p, &OpContext::human())
        .unwrap_or_else(|e| panic!("{} が適用できる: {e}", op.name()));
}

fn sequence_comp(
    id: &str,
    seq: &str,
    (width, height): (u32, u32),
    duration: RationalTime,
) -> Composition {
    let node = Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text(seq.into()));
    Composition::new(Id::new(id), width, height, Rational::integer(FPS), duration).with_layers(
        vec![Layer::new(
            format!("{id}-seq"),
            node,
            secs(0),
            secs(0),
            duration,
        )],
    )
}

fn target(id: &str, comp: &str, (width, height): (u32, u32), dir: &Path) -> RenderTarget {
    RenderTarget {
        id: Id::new(id),
        composition: Id::new(comp),
        width,
        height,
        fps: Rational::integer(FPS),
        codec: "h264".into(),
        output_path: dir.join(format!("{id}.mp4")).to_string_lossy().into_owned(),
        loudness_lufs: None,
        delivery: Default::default(),
    }
}

fn reframe(shift: Param) -> Transform {
    Transform {
        position: shift,
        scale: Param::constant(ParamValue::Number(REFRAME_SCALE)),
        ..Transform::default()
    }
}

/// 本編 main（k0 [1,3)、k1 [3,5)）を作り、k0..k1 を参照するショート short（r0, r1）を分ける。r1 の in を 3.5 に上書きしてから
/// 本編をリップルする（k0 の out を -1、k1 の out を +0.5、k1 の in を -1）。
fn edited_sequences(p: &mut Project, asset: &AssetId) {
    p.sequences.push(Sequence {
        id: Id::new("main"),
        cuts: vec![
            Cut::new("k0", Node::footage(asset), secs(1), secs(3)),
            Cut::new("k1", Node::footage(asset), secs(3), secs(5)),
        ],
        origin: None,
    });
    apply(
        p,
        ForkSequence {
            sequence: Id::new("main"),
            new_sequence: Id::new("short"),
            relation: OriginRelation::Reference,
            cuts: vec![Id::new("r0"), Id::new("r1")],
            range: None,
        },
    );
    apply(
        p,
        OverrideCut {
            cut: Id::new("r1"),
            in_point: Some(half(7)),
            out_point: None,
        },
    );
    for (cut, edge, by) in [
        ("k0", Edge::Out, secs(-1)),
        ("k1", Edge::Out, half(1)),
        ("k1", Edge::In, secs(-1)),
    ] {
        apply(
            p,
            Ripple {
                cut: Id::new(cut),
                edge,
                by,
            },
        );
    }
    let r0 = p.cut(&Id::new("r0")).unwrap().1;
    assert_eq!(
        (r0.in_point, r0.out_point),
        (secs(1), secs(2)),
        "r0 は本編のリップルに従う"
    );
    let r1 = p.cut(&Id::new("r1")).unwrap().1;
    assert_eq!(
        (r1.in_point, r1.out_point),
        (half(7), half(11)),
        "r1 は上書きした in を保ち、out は従う"
    );
}

/// 通しのプロジェクトを `dir` に作り、JSON の場所とプロジェクトのデータを返す。
pub fn build_project(dir: &Path) -> (PathBuf, Project) {
    let clip = dir.join("clip.mp4");
    make_clip(&clip);
    let mut p = Project::new();
    let asset: AssetId = Id::new("clip");
    p.assets.push(Asset {
        id: asset.clone(),
        path: clip.to_string_lossy().into_owned(),
        content_hash: "shorts-clip".into(),
        color_space: ColorSpace::bt709(),
        duration: secs(i64::from(CLIP_SECONDS)),
        fps: Some(Rational::integer(FPS)),
        timebase_offset: RationalTime::ZERO,
    });
    edited_sequences(&mut p, &asset);
    p.compositions
        .push(sequence_comp("main", "main", MAIN_SIZE, half(9)));
    p.compositions
        .push(sequence_comp("vert", "short", SHORT_SIZE, secs(3)));
    p.render_targets
        .push(target("main", "main", MAIN_SIZE, dir));
    p.render_targets
        .push(target("vert", "vert", SHORT_SIZE, dir));
    let mut pan = Param::constant(ParamValue::Vec2([R1_FROM.2, 0.0]));
    pan.keyframes = [R1_FROM, R1_TO]
        .map(|(n, d, x)| {
            Keyframe::new(
                RationalTime::new(i128::from(n), d).unwrap(),
                ParamValue::Vec2([x, 0.0]),
                Interpolation::Linear,
            )
        })
        .into();
    let r0 = reframe(Param::constant(ParamValue::Vec2([R0_SHIFT, 0.0])));
    for (cut, transform) in [("r0", r0), ("r1", reframe(pan))] {
        apply(
            &mut p,
            SetReframe {
                layer: Id::new("vert-seq"),
                cut: Id::new(cut),
                transform: Some(transform),
            },
        );
    }
    let json = dir.join("shorts.json");
    std::fs::write(&json, serde_json::to_vec_pretty(&p).unwrap()).unwrap();
    (json, p)
}

/// ショートの画素（画布の座標）から本編の画素への写像。再フレーミングの変形を戻し、縦の画布に収めた素材の矩形から素材の画素へ。
pub struct Mapping {
    shift: f64,
    /// 縦の画布に縦横比を保って収めた素材の矩形の上端と高さ（`footage` の `contain_region` と同じ丸め）。
    place_y: f64,
    place_h: f64,
}

impl Mapping {
    fn new(shift: f64) -> Self {
        let (mw, mh) = (f64::from(MAIN_SIZE.0), f64::from(MAIN_SIZE.1));
        let (sw, sh) = (f64::from(SHORT_SIZE.0), f64::from(SHORT_SIZE.1));
        let ratio = (sw / mw).min(sh / mh);
        let place_h = (mh * ratio).round();
        Self {
            shift,
            place_y: ((sh - place_h) / 2.0).floor(),
            place_h,
        }
    }

    /// ショートの点 `(u, v)` に対応する本編の点。
    pub fn apply(&self, u: f64, v: f64) -> (f64, f64) {
        let (cx, cy) = (f64::from(SHORT_SIZE.0) / 2.0, f64::from(SHORT_SIZE.1) / 2.0);
        let qx = cx + (u - cx - self.shift) / REFRAME_SCALE;
        let qy = cy + (v - cy) / REFRAME_SCALE;
        let mx = qx * f64::from(MAIN_SIZE.0) / f64::from(SHORT_SIZE.0);
        let my = (qy - self.place_y) * f64::from(MAIN_SIZE.1) / self.place_h;
        (mx, my)
    }
}

/// ショートのフレーム `i` に写るはずの本編のフレームの番号と、画素の写像。
pub fn short_to_main(p: &Project, i: usize) -> (usize, Mapping) {
    let fps = Rational::integer(FPS);
    let t = RationalTime::from_frames(i as i64, fps).unwrap();
    let short = p.sequence(&Id::new("short")).unwrap();
    let placed = place_cuts(short).unwrap();
    let at = placed
        .iter()
        .find(|c| c.range.contains(t))
        .expect("ショートの中");
    let cut = &short.cuts[at.index];
    let source = cut_source_time(cut, at, t).unwrap();
    let main = p.sequence(&Id::new("main")).unwrap();
    let main_placed = place_cuts(main).unwrap();
    let origin = cut.origin.as_ref().unwrap();
    let m = main_placed.iter().find(|c| &c.id == origin).unwrap();
    let main_cut = &main.cuts[m.index];
    let tm = m
        .range
        .start
        .checked_add(source.checked_sub(main_cut.in_point).unwrap())
        .unwrap();
    let frame = (tm.to_f64() * f64::from(FPS)).round() as usize;
    let shift = if cut.id.as_str() == "r0" {
        R0_SHIFT
    } else {
        let s = source.to_f64();
        let at = |(n, d, _): (i64, u32, f64)| n as f64 / f64::from(d);
        let (from, to) = (at(R1_FROM), at(R1_TO));
        R1_FROM.2 + (R1_TO.2 - R1_FROM.2) * (s - from) / (to - from)
    };
    (frame, Mapping::new(shift))
}
