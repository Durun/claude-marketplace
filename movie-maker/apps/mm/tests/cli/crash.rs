//! `mm op` の書き込みの途中で kill しても、開き直すと最後に確定した版に戻り、続けて `mm op` できること。
//! `mm-store` の kill の試験を、CLI の経路でもう一度行う。

use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use mm_store::{OpenMode, Store};
use mm_types::RationalTime;

use crate::common::{mm_command, mm_ok, sequence_json};

/// 1 回のまとまりの命令の数。書き込みの窓を広げるため多めにする。
const OPS_PER_TXN: i64 = 2000;
/// 命令 1 つで尻を伸ばす時間（1000 分の 1 秒）。版ごとに文書の中身が違って見える。
const STEP_DEN: i64 = 1000;
const ROUNDS: usize = 30;
/// kill までの待ちの上限は、無事に終わるまでの時間のこの割合（百分率）。
const MAX_WAIT_PERCENT: u64 = 100;
/// 所要時間を測る回数。
const PROBES: usize = 4;
/// 並び s1 を足した後の版。
const BASE_VERSION: i64 = 1;
const BASE_OUT_SECS: i64 = 4;

/// 擬似乱数（xorshift）。kill の位置をばらす。
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn write_txn_file(path: &Path) {
    let op = serde_json::json!({
        "op": "ripple", "cut": "c1", "edge": "out", "by": {"num": 1, "den": STEP_DEN}
    });
    let ops = vec![op; OPS_PER_TXN as usize];
    fs::write(
        path,
        serde_json::json!({"name": "大きなまとまり", "ops": ops}).to_string(),
    )
    .unwrap();
}

fn new_project(dir: &Path) {
    mm_ok(&["new", dir.to_str().unwrap()]);
    mm_ok(&[
        "op",
        dir.to_str().unwrap(),
        "addSequence",
        "--json",
        &sequence_json(),
    ]);
}

/// c1 の尻が `version` に合う長さであること。
fn assert_state(store: &Store, version: i64) {
    let steps = version - BASE_VERSION;
    let expected = RationalTime::new(
        i128::from(BASE_OUT_SECS * STEP_DEN + steps),
        STEP_DEN as u32,
    )
    .unwrap();
    let cut = &store.project().sequences[0].cuts[0];
    assert_eq!(cut.out_point, expected, "版 {version}");
}

#[test]
fn op_の書き込みの途中で_kill_しても最後に確定した版に戻り続けて書ける() {
    let tmp = tempfile::tempdir().unwrap();
    let txn_file = tmp.path().join("big.json");
    write_txn_file(&txn_file);

    // 何事もなく終わるまでの時間（数回の最短）を測り、kill の待ちの幅にする。
    let full_ms = (0..PROBES)
        .map(|i| {
            let probe = tmp.path().join(format!("probe{i}"));
            new_project(&probe);
            let started = Instant::now();
            mm_ok(&[
                "op",
                probe.to_str().unwrap(),
                "--txn",
                txn_file.to_str().unwrap(),
            ]);
            started.elapsed().as_millis() as u64 + 1
        })
        .min()
        .unwrap();

    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rolled_back = 0usize;
    for round in 0..ROUNDS {
        let dir = tmp.path().join(format!("p{round}"));
        new_project(&dir);
        let mut child = mm_command()
            .args([
                "op",
                dir.to_str().unwrap(),
                "--txn",
                txn_file.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let wait_ms = next(&mut seed) % (full_ms * MAX_WAIT_PERCENT / 100);
        std::thread::sleep(Duration::from_millis(wait_ms));
        let _ = child.kill();
        child.wait().unwrap();

        let version = {
            let store = Store::open(&dir, 0).unwrap();
            assert_eq!(
                store.mode(),
                OpenMode::Writer,
                "round {round}: ロックが残っている"
            );
            let version = store.doc_version();
            assert!(
                version == BASE_VERSION || version == BASE_VERSION + OPS_PER_TXN,
                "round {round}: まとまりの途中の版 {version}"
            );
            assert_state(&store, version);
            version
        };
        rolled_back += usize::from(version == BASE_VERSION);

        mm_ok(&[
            "op",
            dir.to_str().unwrap(),
            "ripple",
            "--cut",
            "c1",
            "--edge",
            "out",
            "--by",
            "1",
        ]);
        let store = Store::open(&dir, 0).unwrap();
        assert_eq!(store.doc_version(), version + 1, "round {round}");
    }
    println!("{rolled_back}/{ROUNDS} 回は書き込みの前か途中で止まり、書きかけは残らなかった");
    assert!(rolled_back > 0, "書き込みの途中で止められていない");
}
