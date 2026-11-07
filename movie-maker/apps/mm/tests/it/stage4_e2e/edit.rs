//! 通しのテストの編集。本編を話題で組み、その先頭の 2 話題を参照するショートを作り、本編をリップルする。

use std::path::{Path, PathBuf};
use std::process::Output;

use mm_doc::Project;
use mm_eval::{place_cuts, sequence_duration};
use mm_store::Store;
use mm_types::{Id, RationalTime};
use serde_json::{json, Value};

use super::plan::{self, Topic};

/// 本編に並べる順（話題の番号）。先頭の `SHORT_TOPICS` 個をショートが参照する。
pub const ORDER: [usize; plan::TOPICS] = [3, 1, 4, 0, 5, 2];
pub const SHORT_TOPICS: usize = 2;
/// 言い淀みを抜く話題の番号（ショートが参照しない話題）。
pub const STUTTER_TOPIC: usize = 0;
pub const FPS: i64 = 30;
/// 設定すると本編を 1080p、ショートを 1080x1920 で書き出す。無ければ縦横を 1/3 にする（CI のソフトウェア描画で時間内に収める）。
const FULL_HD_ENV: &str = "MM_E2E_FULL_HD";
/// 実寸に対する既定の縮尺。
const SMALL_SCALE: f64 = 1.0 / 3.0;
/// 実寸（1080p）でのショートの再フレーミングの横のずれ（画素）、字幕の置き場所と幅。
const FULL_SHIFTS: [f64; 2] = [-150.0, 100.0];
const FULL_CAPTION_AT: [f64; 2] = [540.0, 1700.0];
const FULL_CAPTION_WIDTH: f64 = 900.0;

fn unit() -> f64 {
    if std::env::var_os(FULL_HD_ENV).is_some() {
        1.0
    } else {
        SMALL_SCALE
    }
}

fn scaled(v: f64) -> f64 {
    (v * unit()).round()
}

/// 本編の大きさ（16:9）。
pub fn main_size() -> (u32, u32) {
    (scaled(1920.0) as u32, scaled(1080.0) as u32)
}

/// ショートの大きさ（9:16）。
pub fn short_size() -> (u32, u32) {
    (scaled(1080.0) as u32, scaled(1920.0) as u32)
}

/// ショートの 2 つのカットの、再フレーミングの横のずれ（画素）。
pub fn reframe_shifts() -> [f64; 2] {
    FULL_SHIFTS.map(scaled)
}

/// 縦の画布に縦横比を保って収めた本編の素材の高さ（画素）。`footage` の収め方の丸めと同じ。
pub fn place_h() -> u32 {
    let (mw, mh) = main_size();
    let (sw, sh) = short_size();
    let ratio = (f64::from(sw) / f64::from(mw)).min(f64::from(sh) / f64::from(mh));
    (f64::from(mh) * ratio).round() as u32
}
/// ショートの再フレーミングの拡大。縦の画布に収めた素材（実寸で高さ 608 画素）が画布の高さを少し超える。
pub const REFRAME_SCALE: f64 = 3.2;
/// 本編の 1 つ目のカットの out をリップルで縮める量（秒）。ショートの対応するカットは従う。
pub const RIPPLE_FOLLOWED: i64 = 2;
/// 本編の 2 つ目のカットの out をリップルで縮める量（秒）。ショートの対応するカットは上書きしているので変わらない。
pub const RIPPLE_OVERRIDDEN: i64 = 1;
/// 2 つ目のカットを上書きする in の進み（秒）と out の戻り（秒）。リップル後の本編のカットの中に収まる。
const OVERRIDE_IN: i64 = 1;
const OVERRIDE_OUT: i64 = 2;

/// プロジェクトのフォルダーと、作業のフォルダー。
pub struct Env {
    pub root: PathBuf,
    pub dir: String,
}

impl Env {
    pub fn new(root: PathBuf) -> Self {
        let dir = root.join("p").to_str().unwrap().to_owned();
        Self { root, dir }
    }
}

/// `mm` を実行する。結果は成否にかかわらず返す。
pub fn mm_try(args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_mm"))
        .args(args)
        .output()
        .expect("mm を実行できる")
}

/// `mm` を実行し、失敗したら止める。
pub fn mm(args: &[&str]) -> Output {
    let out = mm_try(args);
    assert!(
        out.status.success(),
        "mm {args:?} が失敗した: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

pub fn apply_txn(env: &Env, name: &str, ops: Vec<Value>) {
    let file = env.root.join(format!("{name}.json"));
    std::fs::write(&file, plan::txn(name, ops)).unwrap();
    mm(&["op", &env.dir, "--txn", file.to_str().unwrap()]);
}

pub fn project_of(dir: &str) -> Project {
    Store::open_read_only(Path::new(dir))
        .expect("プロジェクトを開ける")
        .project()
        .clone()
}

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

fn vec2(x: f64, y: f64) -> Value {
    json!({"value": {"type": "vec2", "value": [x, y]}})
}

fn number(v: f64) -> Value {
    json!({"value": {"type": "number", "value": v}})
}

/// sequence を入れ子にするコンポジション。`plan::add_composition` の 640x360 を作り直す。
fn composition(
    id: &str,
    layer: &str,
    sequence: &str,
    size: (u32, u32),
    len: RationalTime,
) -> Value {
    let mut op = plan::add_composition(sequence, len);
    let c = &mut op["composition"];
    c["id"] = json!(id);
    c["width"] = json!(size.0);
    c["height"] = json!(size.1);
    c["layers"][0]["id"] = json!(layer);
    op
}

/// 本編のコンポジション `main`（1080p）。話題を `ORDER` の順に並べ、コンポジションの長さが話題の和まで伸びること
/// （操作の層が `fitLayer` を足す）は呼ぶ側が確かめる。話題の和を返す。
pub fn build_main(env: &Env, topics: &[Topic]) -> RationalTime {
    let by_order: Vec<&Topic> = ORDER.iter().map(|i| &topics[*i]).collect();
    let build = vec![
        json!({"op": "addSequence", "sequence": {"id": "s1", "cuts": []}}),
        plan::insert_topics("s1", &by_order[..1]),
        composition("main", "l1", "s1", main_size(), by_order[0].len()),
    ];
    apply_txn(env, "build", build);
    apply_txn(
        env,
        "arrange",
        vec![plan::insert_topics("s1", &by_order[1..])],
    );
    topics
        .iter()
        .try_fold(RationalTime::ZERO, |a, t| a.checked_add(t.len()))
        .unwrap()
}

/// 本編の先頭の話題のカット ID と、ショートのカット ID（`k3` に対して `r3`）。
pub fn short_cut_ids() -> Vec<(String, String)> {
    ORDER[..SHORT_TOPICS]
        .iter()
        .map(|n| (format!("k{n}"), format!("r{n}")))
        .collect()
}

/// 本編の先頭の 2 話題を参照するショート `short` とそのコンポジション `vert`（縦 1080x1920）を作る。
/// 各カットに再フレーミングを置き、字幕の置き場所を置き、2 つ目のカットの in/out を上書きする。
pub fn build_short(env: &Env, topics: &[Topic]) {
    let ids = short_cut_ids();
    let short_ids: Vec<&String> = ids.iter().map(|c| &c.1).collect();
    let fork = json!({"op": "forkSequence", "sequence": "s1", "new_sequence": "short",
        "relation": "reference", "cuts": short_ids,
        "range": {"first": ids[0].0, "last": ids[SHORT_TOPICS - 1].0}});
    let t = &topics[ORDER[1]];
    let over = json!({"op": "overrideCut", "cut": ids[1].1,
        "in": plan::time(t.start.checked_add(secs(OVERRIDE_IN)).unwrap()),
        "out": plan::time(t.end.checked_sub(secs(OVERRIDE_OUT)).unwrap())});
    apply_txn(env, "fork", vec![fork, over]);
    // コンポジションの長さは、上書き後の sequence の長さに合わせて置く。
    let len = sequence_duration(project_of(&env.dir).sequence(&Id::new("short")).unwrap()).unwrap();
    let mut ops = vec![composition("vert", "l2", "short", short_size(), len)];
    for ((_, short_cut), shift) in ids.iter().zip(reframe_shifts()) {
        let transform = json!({"position": vec2(shift, 0.0), "scale": number(REFRAME_SCALE)});
        ops.push(
            json!({"op": "setReframe", "layer": "l2", "cut": short_cut, "transform": transform}),
        );
    }
    let caption = json!({"position": vec2(scaled(FULL_CAPTION_AT[0]), scaled(FULL_CAPTION_AT[1])), "width": number(scaled(FULL_CAPTION_WIDTH))});
    ops.push(json!({"op": "setCaption", "layer": "l2", "caption": caption}));
    apply_txn(env, "short", ops);
}

/// 本編の 2 つのカットの out を縮める（リップル）。
pub fn ripple_main(env: &Env) {
    let ids = short_cut_ids();
    let ops = [(&ids[0].0, RIPPLE_FOLLOWED), (&ids[1].0, RIPPLE_OVERRIDDEN)]
        .iter()
        .map(|(cut, by)| {
            json!({"op": "ripple", "cut": cut, "edge": "out", "by": plan::time(secs(-by))})
        })
        .collect();
    apply_txn(env, "ripple", ops);
}

/// sequence `seq` で、カット `cut` の始まり（sequence の時間軸）。
pub fn cut_start(p: &Project, seq: &str, cut: &str) -> RationalTime {
    let placed = place_cuts(p.sequence(&Id::new(seq)).unwrap()).unwrap();
    placed
        .iter()
        .find(|c| c.id.as_str() == cut)
        .unwrap_or_else(|| panic!("カット {cut} が {seq} に無い"))
        .range
        .start
}

/// 本編の話題ごとのチャプター（各話題の最初のカットの始まりに置くマーカー）の ID と時刻と名前。
pub fn chapter_marks(p: &Project) -> Vec<(String, RationalTime, String)> {
    ORDER
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let at = cut_start(p, "s1", &format!("k{n}"));
            (format!("m{i}"), at, format!("話題{n}"))
        })
        .collect()
}

fn target(id: &str, comp: &str, size: (u32, u32), lufs: f64, delivery: Value) -> Value {
    let mut op = plan::add_render_target();
    let t = &mut op["target"];
    t["id"] = json!(id);
    t["composition"] = json!(comp);
    t["width"] = json!(size.0);
    t["height"] = json!(size.1);
    t["output_path"] = json!(format!("{id}.mp4"));
    t["loudness_lufs"] = json!(lufs);
    t["delivery"] = delivery;
    op
}

/// チャプターのマーカーと、2 つのレンダーターゲット（YouTube 用 `yt` と Shorts 用 `shorts`）を足す。
pub fn add_targets(env: &Env, lufs: f64) {
    let p = project_of(&env.dir);
    let mut ops: Vec<Value> = chapter_marks(&p)
        .iter()
        .map(|(id, at, name)| {
            json!({"op": "addMarker", "composition": "main", "marker":
                {"id": id, "time": plan::time(*at), "duration": plan::time(RationalTime::ZERO), "name": name}})
        })
        .collect();
    let yt = json!({"subtitles": "sidecar", "chapters": true});
    ops.push(target("yt", "main", main_size(), lufs, yt));
    let shorts = json!({"subtitles": "sidecar"});
    ops.push(target("shorts", "vert", short_size(), lufs, shorts));
    apply_txn(env, "targets", ops);
}
