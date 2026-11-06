//! 単体の実行ファイル。200ms ごとに操作を適用し、マーカーと時刻（epoch ms）を 1 行出す。
//! バイナリのクレート（ここ）の関数と、別クレート（ops-sample）の関数の両方が差し替わるかを見る。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mm_doc::Project;

/// バイナリのクレート側のマーカー。
const TIP_MARK: &str = "tip-v1";

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// バイナリのクレートに置いた操作。
fn tip_op(project: &mut Project) -> String {
    let layer = &mut project.compositions[0].layers[0];
    layer.lock = !layer.lock;
    let opacity = layer.opacity.value.clone();
    format!("{TIP_MARK} lock={} opacity={opacity:?}", layer.lock)
}

fn main() {
    // patch を受けるエントリーポイント。dx が起動時に渡す環境変数で接続先が決まる。
    dioxus_devtools::connect_subsecond();
    let mut project = ops_sample::demo_project();
    let mut value = 0.0;
    loop {
        // 周ごとに call し直すので、patch 後の最初の周から新しい関数に入る。
        let (tip, lib) = subsecond::call(|| {
            value += 0.01;
            (
                tip_op(&mut project),
                ops_sample::set_opacity(&mut project, value),
            )
        });
        println!("TICK {} | {tip} | {lib}", epoch_ms());
        std::thread::sleep(Duration::from_millis(200));
    }
}
