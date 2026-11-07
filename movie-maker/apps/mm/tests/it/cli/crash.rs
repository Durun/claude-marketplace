//! `mm op` と `mm undo` の書き込みの途中で kill しても、開き直すと最後に確定したバージョンに戻り、続けて書けること。
//! `mm-store` の kill の試験を、CLI の経路でもう一度行う。
//!
//! 親は子の WAL を見張り、確定していないフレームが現れた（SQLite が書き始めた）直後に kill する。
//! これで「書き込みの最中に止めた」回を数え、十分な数あることを確かめる。
//! 操作は大きな文字の `setParam` にして、1 回の書き込みが SQLite のキャッシュ（既定 2 MB）からあふれ、
//! 確定の前からフレームが WAL に出るようにする。

use std::fs;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use mm_store::{uncommitted_frames, OpenMode, Store};
use mm_types::ParamValue;

use super::common::{mm_command, mm_ok, stage0_json};

/// 1 回のトランザクションの操作の数。
const OPS_PER_TXN: usize = 400;
/// 1 操作の文字の長さ。操作と逆操作で 1 行 40 KB、トランザクションで 16 MB になる。
const TEXT_LEN: usize = 20_000;
const ROUNDS: usize = 16;
/// 書き込みの最中に止めた回がこれより少なければ、見張りが書き込みの時間幅に当たっていない。
const MIN_MID_WRITE: usize = 6;
/// 見張りの間隔。
const POLL: Duration = Duration::from_millis(1);
/// 子が終わるまでの待ちの上限（無事に終わる時間のこの倍）。
const DEADLINE_FACTOR: u32 = 3;
/// 書き始めを見てから kill するまでの、ばらけた余分の待ち（ミリ秒）の上限。
const EXTRA_WAIT_MS: u64 = 200;
const LAYER: &str = "l-text";

/// 擬似乱数（xorshift）。kill の位置をばらす。
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn text_value(i: usize) -> String {
    format!("{i}:{}", "字".repeat(TEXT_LEN))
}

fn write_txn_file(path: &Path) {
    let ops: Vec<_> = (1..=OPS_PER_TXN)
        .map(|i| {
            serde_json::json!({
                "op": "setParam",
                "target": {"layer": LAYER, "path": "source.text"},
                "value": {"type": "text", "value": text_value(i)}
            })
        })
        .collect();
    fs::write(
        path,
        serde_json::json!({"name": "大きなトランザクション", "ops": ops}).to_string(),
    )
    .unwrap();
}

fn new_project(dir: &Path) {
    mm_ok(&[
        "import",
        dir.to_str().unwrap(),
        stage0_json().to_str().unwrap(),
    ]);
}

/// レイヤー l-text の文字。
fn text_of(store: &Store) -> String {
    let layer = store
        .project()
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .find(|l| l.id.as_str() == LAYER)
        .expect("l-text がある");
    match layer.source.params.get("text").map(|p| &p.value) {
        Some(ParamValue::Text(t)) => t.clone(),
        other => panic!("text でない: {other:?}"),
    }
}

/// 子の WAL を見張り、書き始めを見たら少し待って kill する。戻りは書き込みの最中に止めたか。
/// 見ないまま子が終われば `false`。
fn kill_mid_write(child: &mut Child, db: &Path, deadline: Duration, seed: &mut u64) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if child.try_wait().unwrap().is_some() {
            return false;
        }
        if uncommitted_frames(db).unwrap_or(0) > 0 {
            std::thread::sleep(Duration::from_millis(next(seed) % EXTRA_WAIT_MS));
            let _ = child.kill();
            child.wait().unwrap();
            return uncommitted_frames(db).unwrap_or(0) > 0;
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    child.wait().unwrap();
    false
}

fn spawn_mm(args: &[&str]) -> Child {
    mm_command()
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// 復旧した DB を書き手で開き、バージョンが途中でないことを見る。
fn recovered(dir: &Path, round: usize, versions: [i64; 2]) -> (i64, String) {
    let store = Store::open(dir, 0).unwrap();
    assert_eq!(
        store.mode(),
        OpenMode::Writer,
        "round {round}: ロックが残っている"
    );
    let version = store.doc_version();
    assert!(
        versions.contains(&version),
        "round {round}: トランザクションの途中のバージョン {version}"
    );
    (version, text_of(&store))
}

#[test]
#[ignore = "通しの試験。main への push と手動実行で走らせる"]
fn op_と_undo_の書き込みの最中で_kill_しても最後に確定したバージョンに戻り続けて書ける() {
    let tmp = tempfile::tempdir().unwrap();
    let txn_file = tmp.path().join("big.json");
    write_txn_file(&txn_file);
    let txn_arg = txn_file.to_str().unwrap();

    // 何事もなく終わるまでの時間を測り、見張りの上限にする。
    let probe = tmp.path().join("probe");
    new_project(&probe);
    let (base_version, original) = {
        let store = Store::open_read_only(&probe).unwrap();
        (store.doc_version(), text_of(&store))
    };
    let started = Instant::now();
    mm_ok(&["op", probe.to_str().unwrap(), "--txn", txn_arg]);
    let deadline = started.elapsed() * DEADLINE_FACTOR + Duration::from_secs(1);
    let full_version = base_version + OPS_PER_TXN as i64;
    let last = text_value(OPS_PER_TXN);

    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let (mut mid_op, mut mid_undo) = (0, 0);
    for round in 0..ROUNDS {
        let dir = tmp.path().join(format!("p{round}"));
        new_project(&dir);
        let db = dir.join("project.mmdb");
        let dir_arg = dir.to_str().unwrap();

        // `mm op` の最中に kill。
        let mut child = spawn_mm(&["op", dir_arg, "--txn", txn_arg]);
        mid_op += usize::from(kill_mid_write(&mut child, &db, deadline, &mut seed));
        let (version, text) = recovered(&dir, round, [base_version, full_version]);
        if version == base_version {
            assert_eq!(text, original, "round {round}");
            mm_ok(&["op", dir_arg, "--txn", txn_arg]);
        } else {
            assert_eq!(text, last, "round {round}");
        }

        // `mm undo` の最中に kill。
        let mut child = spawn_mm(&["undo", dir_arg]);
        mid_undo += usize::from(kill_mid_write(&mut child, &db, deadline, &mut seed));
        let undone_version = full_version + OPS_PER_TXN as i64;
        let (version, text) = recovered(&dir, round, [full_version, undone_version]);
        let expected = if version == full_version {
            &last
        } else {
            &original
        };
        assert_eq!(&text, expected, "round {round}");

        // 続けて書ける。
        mm_ok(&[
            "op",
            dir_arg,
            "lockLayer",
            "--layer",
            LAYER,
            "--lock",
            "true",
        ]);
        assert_eq!(
            Store::open_read_only(&dir).unwrap().doc_version(),
            version + 1,
            "round {round}"
        );
    }
    println!("{ROUNDS} 回のうち、op は {mid_op} 回、undo は {mid_undo} 回を書き込みの最中に止めた");
    assert!(
        mid_op >= MIN_MID_WRITE,
        "op の書き込みの最中に止めた回が {mid_op} 回しか無い"
    );
    assert!(
        mid_undo >= MIN_MID_WRITE,
        "undo の書き込みの最中に止めた回が {mid_undo} 回しか無い"
    );
}
