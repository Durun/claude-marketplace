//! 大きさの見積もりの確かめ。操作を 10 万件書いて DB の大きさと開く時間を測る。
//! `cargo test -p mm-store --release --test store scale -- --ignored --nocapture` で動かす。

use std::time::Instant;

use mm_store::Store;

use crate::common::{locked, project_with, NOW};

const TOTAL_OPS: usize = 100_000;
/// ドラッグ 1 回ぶんの操作の数。
const OPS_PER_TXN: usize = 20;
const LAYERS: usize = 2000;

#[test]
#[ignore = "測定用。時間がかかる"]
fn 操作_10_万件の大きさと開く時間() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(LAYERS), NOW).unwrap();
    let mut project = project_with(LAYERS);
    let start = Instant::now();
    let mut worst = std::time::Duration::ZERO;
    for t in 0..TOTAL_OPS / OPS_PER_TXN {
        let ops: Vec<usize> = (0..OPS_PER_TXN)
            .map(|i| (t * OPS_PER_TXN + i) % LAYERS)
            .collect();
        let input = locked(&mut project, &format!("t{t}"), &ops);
        let at = Instant::now();
        s.commit(&project, &input).unwrap();
        worst = worst.max(at.elapsed());
    }
    let wrote = start.elapsed();
    s.close(NOW).unwrap();
    let size = std::fs::metadata(tmp.path().join("project.mmdb"))
        .unwrap()
        .len();
    let open = Instant::now();
    let s = Store::open(tmp.path(), NOW).unwrap();
    let opened = open.elapsed();
    let backup_start = Instant::now();
    let mut s = s;
    s.backup(NOW + 1).unwrap();
    println!(
        "ops={TOTAL_OPS} layers={LAYERS} db={:.1}MB write_total={wrote:?} worst_commit={worst:?} open={opened:?} backup={:?}",
        size as f64 / 1e6,
        backup_start.elapsed()
    );
}
