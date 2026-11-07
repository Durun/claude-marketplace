//! 1 つのプロジェクトに本編（16:9）と参照のショート（9:16）を作り、本編をリップルしてから両方を `mm render --target`
//! で書き出す。ショートの各フレームが、本編の同じ素材の時刻のフレームを再フレーミングして切り出した画像になるかを確かめる。
//! FFmpeg が要るので feature `ffmpeg` のときだけ実行する。

mod support;

use std::path::Path;

use mm_decode_api::DecoderFactory;
use mm_decode_ffmpeg::FfmpegDecoderFactory;
use mm_types::{RationalTime, RgbaImage};
use support::{build_project, mm, short_to_main, Mapping, MAIN_SIZE, SHORT_SIZE};

/// 比べないふちの幅（画素）。再フレーミングの切り出しが素材の端に寄る所の補間の差を除く。
const BORDER: u32 = 6;
/// 正しい時刻の本編から切り出した画像との、画素あたりの差（0..1）の平均の上限。圧縮と補間の差を許す。
const MAX_ERROR: f64 = 0.02;
/// 時刻をずらした本編との差が、正しい時刻との差の何倍以上あるべきか（時刻の取り違えを捕まえる）。
const WRONG_TIME_RATIO: f64 = 10.0;
/// 取り違えの比べに使う、ずらすフレームの数。
const WRONG_SHIFT: usize = 6;

fn decode_all(path: &Path) -> Vec<RgbaImage> {
    let mut decoder = FfmpegDecoderFactory.open(path, None).expect("開ける");
    decoder.seek(RationalTime::ZERO).expect("先頭へ");
    let mut frames = Vec::new();
    while let Some(frame) = decoder.next_frame().expect("読める") {
        frames.push(frame.image.expect("CPU の画像"));
    }
    frames
}

/// 本編の画像を、浮動小数の画素の位置で双線形に引く（端は端の値）。RGB の平均を返す。
fn sample(image: &RgbaImage, x: f64, y: f64) -> [f64; 3] {
    let (w, h) = (image.width() as f64, image.height() as f64);
    let (x, y) = ((x - 0.5).clamp(0.0, w - 1.0), (y - 0.5).clamp(0.0, h - 1.0));
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: f64, dy: f64| {
        let px = image.pixel(
            ((x0 + dx).min(w - 1.0)) as u32,
            ((y0 + dy).min(h - 1.0)) as u32,
        );
        [f64::from(px.r), f64::from(px.g), f64::from(px.b)]
    };
    let (a, b, c, d) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
    std::array::from_fn(|i| {
        let top = a[i] * (1.0 - fx) + b[i] * fx;
        let bottom = c[i] * (1.0 - fx) + d[i] * fx;
        top * (1.0 - fy) + bottom * fy
    })
}

/// ショートのフレームと、本編のフレームから写像で切り出した画像の、画素あたりの差の平均。
fn error(short: &RgbaImage, main: &RgbaImage, map: &Mapping) -> f64 {
    let (w, h) = SHORT_SIZE;
    let mut sum = 0.0;
    let mut n = 0.0;
    for v in BORDER..h - BORDER {
        for u in BORDER..w - BORDER {
            let (mx, my) = map.apply(f64::from(u) + 0.5, f64::from(v) + 0.5);
            let want = sample(main, mx, my);
            let got = short.pixel(u, v);
            let got = [f64::from(got.r), f64::from(got.g), f64::from(got.b)];
            sum += (0..3).map(|i| (want[i] - got[i]).abs()).sum::<f64>() / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn 参照のショートは本編のリップルに従い_本編の同じ素材の時刻から再フレーミングされる() {
    let tmp = tempfile::tempdir().unwrap();
    // MM_SHORTS_DIR を渡すと、素材と書き出しをそのフォルダーに残す（目で確かめる）。
    let kept = std::env::var_os("MM_SHORTS_DIR").map(std::path::PathBuf::from);
    let dir = kept.as_deref().unwrap_or(tmp.path());
    let (json, project) = build_project(dir);

    let report = mm(&["shorts", json.to_str().unwrap()]);
    assert!(report.contains("k0  short:r0（参照）"), "{report}");
    assert!(
        report.contains("k1  short:r1（参照、in を上書き）"),
        "{report}"
    );

    let main_out = dir.join("main.mp4");
    let short_out = dir.join("vert.mp4");
    for (target, out) in [("main", &main_out), ("vert", &short_out)] {
        mm(&[
            "render",
            json.to_str().unwrap(),
            "--target",
            target,
            "-o",
            out.to_str().unwrap(),
        ]);
    }
    let main = decode_all(&main_out);
    let short = decode_all(&short_out);
    assert_eq!((main[0].width(), main[0].height()), MAIN_SIZE);
    assert_eq!((short[0].width(), short[0].height()), SHORT_SIZE);
    assert_eq!(main.len(), 135, "本編は k0 [1,2) と k1 [2,5.5) の 4.5 秒");
    assert_eq!(
        short.len(),
        90,
        "ショートは r0 [1,2) と r1 [3.5,5.5) の 3 秒"
    );

    let mut worst = 0.0_f64;
    let mut wrong_total = 0.0;
    let mut right_total = 0.0;
    for (i, frame) in short.iter().enumerate() {
        let (j, map) = short_to_main(&project, i);
        let e = error(frame, &main[j], &map);
        worst = worst.max(e);
        let wrong = if j >= WRONG_SHIFT {
            j - WRONG_SHIFT
        } else {
            j + WRONG_SHIFT
        };
        wrong_total += error(frame, &main[wrong.min(main.len() - 1)], &map);
        right_total += e;
    }
    eprintln!("差の最大 {worst}、合計 {right_total}、時刻をずらした合計 {wrong_total}");
    assert!(worst < MAX_ERROR, "正しい時刻の本編との差の最大 {worst}");
    assert!(
        wrong_total > right_total * WRONG_TIME_RATIO,
        "時刻をずらした本編との差 {wrong_total} が正しい時刻の差 {right_total} と見分けられない"
    );
}
