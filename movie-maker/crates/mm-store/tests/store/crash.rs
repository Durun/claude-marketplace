//! 書き込みの途中でプロセスを kill しても、開き直すと最後に確定した版に戻ること。
//!
//! 子のプロセスはこのテストの実行ファイル自身を `crash_child` だけ走らせて作る。

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use mm_store::{OpenMode, Store};

use crate::common::{project_with, txn, NOW};

const DIR_ENV: &str = "MM_STORE_CRASH_DIR";
/// 1 回のトランザクションの命令の数。書き込みの窓を広げるため多めにする。
const OPS_PER_TXN: usize = 500;
const ROUNDS: u64 = 60;
/// 子を起動してから kill するまでの待ち（ミリ秒）の下限と幅。何回目の書き込みの途中に当たるかがばらける。
const MIN_RUN_MS: u64 = 40;
const RUN_SPREAD_MS: u64 = 160;

/// 子として動く。親が `DIR_ENV` を付けたときだけ、書き始めと確定のたびに 1 行出し続ける。
#[test]
fn crash_child() {
    let Ok(dir) = std::env::var(DIR_ENV) else {
        return;
    };
    let mut store = Store::open(Path::new(&dir), NOW).unwrap();
    for i in 1.. {
        let ops: Vec<usize> = ((i - 1) * OPS_PER_TXN..i * OPS_PER_TXN).collect();
        println!("begin {i}");
        store
            .commit(&project_with(i), &txn(&format!("t{i}"), &ops))
            .unwrap();
        println!("committed {i}");
    }
}

/// 擬似乱数（xorshift）。乱数の crate を足さずに kill の位置をばらす。
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// 子が出した行から読んだ進み具合。
#[derive(Default)]
struct Progress {
    began: u64,
    confirmed: u64,
}

impl Progress {
    fn feed(&mut self, line: &str) {
        if let Some(n) = line.strip_prefix("begin ") {
            self.began = n.parse().unwrap();
        } else if let Some(n) = line.strip_prefix("committed ") {
            self.confirmed = n.parse().unwrap();
        }
    }
}

/// 子を動かし、ばらけた時間の後に kill する。
fn run_round(seed: &mut u64) -> (tempfile::TempDir, Progress) {
    let tmp = tempfile::tempdir().unwrap();
    drop(Store::create(tmp.path(), &project_with(0), NOW).unwrap());
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "crash::crash_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(DIR_ENV, tmp.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let run_ms = MIN_RUN_MS + next(seed) % RUN_SPREAD_MS;
    std::thread::sleep(Duration::from_millis(run_ms));
    child.kill().unwrap();
    child.wait().unwrap();
    let mut progress = Progress::default();
    let lines = BufReader::new(child.stdout.take().unwrap()).lines();
    for line in lines.map_while(Result::ok) {
        progress.feed(&line);
    }
    (tmp, progress)
}

#[test]
fn 書き込みの途中で_kill_しても最後に確定した版に戻る() {
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    let mut rolled_back = 0u32;
    for round in 0..ROUNDS {
        let (tmp, progress) = run_round(&mut seed);
        let mut store = Store::open(tmp.path(), NOW).unwrap();
        assert_eq!(store.mode(), OpenMode::Writer, "round {round}");
        let version = store.doc_version();
        let per = OPS_PER_TXN as i64;
        assert_eq!(
            version % per,
            0,
            "round {round}: トランザクションの途中で止まっている"
        );
        let n = (version / per) as u64;
        // 確定の表示の後に落ちれば n は確定の回数、確定した直後で表示の前なら 1 多い。
        assert!(
            n == progress.confirmed || n == progress.confirmed + 1,
            "round {round}: 確定 {} 回に対して復旧後は {n}",
            progress.confirmed
        );
        assert_eq!(store.project(), &project_with(n as usize), "round {round}");
        let txns = store.txns_between(0, version).unwrap();
        assert_eq!(txns.len() as u64, n, "round {round}");
        assert!(txns.iter().all(|t| t.ops.len() == OPS_PER_TXN));
        rolled_back += u32::from(progress.began > n);
        store
            .commit(
                &project_with(n as usize + 1),
                &txn("after-crash", &[0, 1, 2]),
            )
            .unwrap();
        assert_eq!(store.doc_version(), version + 3);
    }
    println!("{rolled_back}/{ROUNDS} 回は書き込みの途中で止まり、書きかけは消えた");
    assert!(rolled_back > 0, "書き込みの途中で止められていない");
}
