//! 書き出したファイルのフレームを、`mm frame`（プレビューと同じレンダラーの経路）のフレームと比べる。
//!
//! 比べ方は SSIM。画素の差の平均より、H.264 の量子化のぼけや色の丸めに強く、位置や時刻のずれには敏感なため。
//! 正しい時刻のフレームとの SSIM が `MIN_SSIM` 以上で、時刻をずらしたフレームとの SSIM より `MIN_MARGIN` 以上大きいことを求める。

use std::path::Path;

use mm_doc::Project;
use mm_eval::{cut_at, cut_source_time, place_cuts};
use mm_types::{Id, Rational, RationalTime};
use serde_json::{json, Value};

use super::checks::{extract_frame, frame_count, reframe_png, ssim};
use super::edit::{
    mm, place_h, reframe_shifts, short_cut_ids, short_size, Env, FPS, REFRAME_SCALE,
};

/// 書き出しとプレビューの SSIM の下限。testsrc2 の素材は乱数の市松の領域を持ち、H.264 で最も下がる（実測 1080p で 0.93 から 0.97、縮尺 1/3 で 0.90 から 0.96）。
const MIN_SSIM: f64 = 0.88;
/// 本編のフレームから ffmpeg のフィルターで再フレーミングした画像との SSIM の下限。拡大の補間の違い（二重の双線形）を見込む。
const MIN_SSIM_REFRAMED: f64 = 0.85;
/// 時刻をずらしたフレームとの SSIM を、正しい時刻の SSIM が上回るべき差。
const MIN_MARGIN: f64 = 0.03;
/// 取り違えの比べに使う、ずらすフレームの数。
const WRONG_SHIFT: i64 = 15;

fn fps() -> Rational {
    Rational::integer(FPS as u32)
}

/// コンポジション `comp` のフレーム `n` をプレビューの経路で描く。
fn preview(env: &Env, comp: &str, n: i64, png: &Path) {
    mm(&[
        "frame",
        &env.dir,
        &format!("{n}/{FPS}"),
        "--comp",
        comp,
        "-o",
        png.to_str().unwrap(),
    ]);
}

/// 数か所のフレーム番号。先頭、先頭の次、全長の 1/4・1/2・3/4、最後、編集点の前後。
fn sample(count: i64, boundaries: &[i64]) -> Vec<i64> {
    let mut v = vec![0, count / 2, count - 1];
    for b in boundaries.iter().take(1) {
        v.extend([b - 1, *b]);
    }
    v.retain(|n| (0..count).contains(n));
    v.sort_unstable();
    v.dedup();
    v
}

fn boundaries(p: &Project, seq: &str) -> Vec<i64> {
    let placed = place_cuts(p.sequence(&Id::new(seq)).unwrap()).unwrap();
    placed
        .iter()
        .skip(1)
        .map(|c| c.range.start.frame_index(fps()))
        .collect()
}

fn near(n: i64, count: i64) -> i64 {
    if n + WRONG_SHIFT < count {
        n + WRONG_SHIFT
    } else {
        n - WRONG_SHIFT
    }
}

fn judge(label: &str, got: f64, wrong: f64, min: f64) {
    assert!(got >= min, "{label}: SSIM {got:.4}（{min} 以上のはず）");
    assert!(
        got - wrong >= MIN_MARGIN,
        "{label}: 正しい時刻の SSIM {got:.4} が、ずらした時刻の {wrong:.4} と見分けられない"
    );
}

/// 本編の書き出しと `mm frame main` を比べる。
pub fn check_main(env: &Env, p: &Project, video: &Path, report: &mut Value) {
    let count = frame_count(video);
    let tmp = env.root.join("frames");
    std::fs::create_dir_all(&tmp).unwrap();
    let (got_png, ref_png, wrong_png) = (
        tmp.join("got.png"),
        tmp.join("ref.png"),
        tmp.join("wrong.png"),
    );
    let mut rows = Vec::new();
    for n in sample(count, &boundaries(p, "s1")) {
        extract_frame(video, n, &got_png);
        preview(env, "main", n, &ref_png);
        preview(env, "main", near(n, count), &wrong_png);
        let (got, wrong) = (ssim(&got_png, &ref_png), ssim(&got_png, &wrong_png));
        eprintln!("本編 {n}: SSIM {got:.4}（ずらしたフレーム {wrong:.4}）");
        judge(&format!("本編 {n}"), got, wrong, MIN_SSIM);
        rows.push(json!({"frame": n, "ssim": got, "wrong": wrong}));
    }
    report["main_frames"] = json!(rows);
}

/// ショートのフレームに対応する、本編の同じ素材の時刻のフレーム番号と、ショートのカットの番号。
fn main_frame_of(p: &Project, n: i64) -> (i64, usize) {
    let t = RationalTime::from_frames(n, fps()).unwrap();
    let short = p.sequence(&Id::new("short")).unwrap();
    let placed = place_cuts(short).unwrap();
    let at = cut_at(&placed, t).expect("ショートのカットの中");
    let cut = &short.cuts[at.index];
    let source = cut_source_time(cut, at, t).unwrap();
    let origin = cut.origin.as_ref().expect("参照のカット");
    let main = p.sequence(&Id::new("s1")).unwrap();
    let main_placed = place_cuts(main).unwrap();
    let m = main_placed.iter().find(|c| &c.id == origin).unwrap();
    let main_cut = &main.cuts[m.index];
    assert!(
        (main_cut.in_point..main_cut.out_point).contains(&source),
        "ショートの素材の時刻 {source:?} が本編のカットの外"
    );
    let at_main = m
        .range
        .start
        .checked_add(source.checked_sub(main_cut.in_point).unwrap())
        .unwrap();
    let frame = at_main.frame_index(fps());
    assert_eq!(
        RationalTime::from_frames(frame, fps()).unwrap(),
        at_main,
        "本編の時刻がフレームの上"
    );
    let index = short_cut_ids()
        .iter()
        .position(|c| c.1 == cut.id.as_str())
        .unwrap();
    (frame, index)
}

/// ショートの書き出しを、`mm frame vert`（プレビュー）と、本編の同じ素材の時刻のフレームを再フレーミングした画像に比べる。
pub fn check_short(env: &Env, p: &Project, video: &Path, report: &mut Value) {
    let count = frame_count(video);
    let tmp = env.root.join("frames");
    std::fs::create_dir_all(&tmp).unwrap();
    let png = |name: &str| tmp.join(name);
    let mut rows = Vec::new();
    for n in sample(count, &boundaries(p, "short")) {
        extract_frame(video, n, &png("s-got.png"));
        preview(env, "vert", n, &png("s-ref.png"));
        preview(env, "vert", near(n, count), &png("s-wrong.png"));
        let (got, wrong) = (
            ssim(&png("s-got.png"), &png("s-ref.png")),
            ssim(&png("s-got.png"), &png("s-wrong.png")),
        );
        judge(&format!("ショート {n}（プレビュー）"), got, wrong, MIN_SSIM);

        let (main_n, cut) = main_frame_of(p, n);
        preview(env, "main", main_n, &png("s-main.png"));
        reframe_png(
            &png("s-main.png"),
            &png("s-expect.png"),
            short_size(),
            place_h(),
            REFRAME_SCALE,
            reframe_shifts()[cut],
        );
        let (wrong_main, _) = main_frame_of(p, near(n, count));
        preview(env, "main", wrong_main, &png("s-main-wrong.png"));
        reframe_png(
            &png("s-main-wrong.png"),
            &png("s-expect-wrong.png"),
            short_size(),
            place_h(),
            REFRAME_SCALE,
            reframe_shifts()[cut],
        );
        let (from_main, from_wrong) = (
            ssim(&png("s-got.png"), &png("s-expect.png")),
            ssim(&png("s-got.png"), &png("s-expect-wrong.png")),
        );
        eprintln!("ショート {n}: プレビューと SSIM {got:.4}、本編の素材の時刻 {main_n} 番から {from_main:.4}（ずらしたフレーム {from_wrong:.4}）");
        judge(
            &format!("ショート {n}（本編から）"),
            from_main,
            from_wrong,
            MIN_SSIM_REFRAMED,
        );
        rows.push(
            json!({"frame": n, "preview_ssim": got, "main_frame": main_n,
            "from_main_ssim": from_main, "from_main_wrong": from_wrong}),
        );
    }
    report["short_frames"] = json!(rows);
}
