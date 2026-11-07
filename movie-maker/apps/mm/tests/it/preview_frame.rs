//! アプリの Program と同じ組み合わせ（`RuntimeConfig::real()`、Preview の画質）で、sequence のフレームが再生位置の
//! 素材のフレームになるかを確かめる。素材は `mm ingest` で取り込み、プレビュー動画を持つ。
//! 素材のフレームの番号は左上の矩形に焼き込む（`fixtures/media/generate.sh` と同じ形）。FFmpeg が要るので feature `ffmpeg` のときだけ走る。

use crate::common;

use std::path::Path;
use std::process::Command;

use mm_doc::{Composition, Cut, Layer, Node, Project, Sequence, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_node_api::Quality;
use mm_runtime::{assemble, RuntimeConfig};
use mm_store::Store;
use mm_types::{Id, ParamValue, Rational, RationalTime, Size};

/// 素材の長さ（秒）。キーフレームは x264 の既定（250 フレームごと）なので、途中のシークは前のキーフレームから読む。
const CLIP_SECONDS: u32 = 12;
const FPS: i64 = 30;
const FPS_U32: u32 = 30;
const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
/// ノードの種類 `sequence` のバージョン。
const SEQUENCE_KIND_VERSION: u32 = 1;
/// sequence のカット（素材の秒の範囲）。後ろのカットを素材の先頭にして、境の前後の取り違えを捕まえる。
const CUTS: [(i64, i64); 2] = [(6, 9), (0, 3)];

/// 焼き込みの番号つきの素材を作る。
fn make_clip(path: &Path) {
    let barcode = "geq=lum='if(lt(Y,16)*lt(X,256),16+219*mod(floor(N/pow(2,floor(X/16))),2),p(X,Y))':cb=128:cr=128";
    let video = format!(
        "color=c=0x606060:s={WIDTH}x{HEIGHT}:r={FPS},format=yuv420p,{barcode},trim=end_frame={}",
        i64::from(CLIP_SECONDS) * FPS
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
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "12"])
        .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(path)
        .status()
        .expect("ffmpeg を実行できる");
    assert!(status.success(), "素材を作れた");
}

fn mm(args: &[&str]) {
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
}

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

/// 取り込んだ素材のカットを並べた sequence と、それを 1 枚のレイヤーで置いたコンポジションを足す。
fn arrange(mut project: Project) -> Project {
    let asset = project.assets[0].id.clone();
    let cuts = CUTS
        .iter()
        .enumerate()
        .map(|(i, (a, b))| Cut::new(format!("k{i}"), Node::footage(&asset), secs(*a), secs(*b)))
        .collect();
    project.sequences.push(Sequence {
        id: Id::new("s1"),
        cuts,
        origin: None,
    });
    let total: i64 = CUTS.iter().map(|(a, b)| b - a).sum();
    let source = Node::new(SEQUENCE_KIND, SEQUENCE_KIND_VERSION)
        .with_param(SEQUENCE_PARAM, ParamValue::Text("s1".into()));
    project.compositions.push(
        Composition::new(
            Id::new("main"),
            WIDTH,
            HEIGHT,
            Rational::new(FPS_U32, 1).unwrap(),
            secs(total),
        )
        .with_layers(vec![Layer::new(
            "l1",
            source,
            secs(0),
            secs(0),
            secs(total),
        )]),
    );
    project
}

/// sequence のフレーム `index` に出るはずの素材のフレームの番号。
fn expected_source_frame(index: i64) -> i64 {
    let mut start = 0;
    for (a, b) in CUTS {
        let len = (b - a) * FPS;
        if index < start + len {
            return a * FPS + index - start;
        }
        start += len;
    }
    panic!("フレーム {index} は sequence の外");
}

fn prepared_project(root: &Path) -> Project {
    common::fixtures();
    let clip = root.join("clip.mp4");
    make_clip(&clip);
    let dir = root.join("p");
    let dir_s = dir.to_str().unwrap();
    mm(&["new", dir_s]);
    mm(&["ingest", dir_s, clip.to_str().unwrap()]);
    assert!(
        dir.join("cache/preview")
            .read_dir()
            .unwrap()
            .next()
            .is_some(),
        "プレビュー動画がある"
    );
    let project = Store::open_read_only(&dir).unwrap().project().clone();
    arrange(project)
}

#[test]
fn preview_の画質で再生した_sequence_の絵が再生位置の素材のフレームになる() {
    let tmp = tempfile::tempdir().unwrap();
    let project = prepared_project(tmp.path());
    let mut engine = assemble(&RuntimeConfig::real()).expect("組み立てられる");
    let comp = Id::new("main");
    let size = Size::new(WIDTH, HEIGHT);
    let fps = Rational::new(FPS_U32, 1).unwrap();
    // 3 秒の位置へのシーク（停止中の表示）と、先頭からの再生（カットの境をまたぐ）。
    let seek = [89_i64, 90, 91];
    let play = 0..(4 * FPS);
    let mut wrong = Vec::new();
    for index in seek.into_iter().chain(play) {
        let t = RationalTime::from_frames(index, fps).unwrap();
        let image = engine
            .render_frame(&project, &comp, t, size, Quality::Preview)
            .expect("描ける");
        let got = common::read_frame_number(&image);
        let want = expected_source_frame(index);
        if got != want {
            wrong.push((index, got, want));
        }
    }
    assert!(
        wrong.is_empty(),
        "（sequence のフレーム、出た素材の番号、出るはずの番号）: {wrong:?}"
    );
}
