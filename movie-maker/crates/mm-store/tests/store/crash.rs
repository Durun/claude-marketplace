//! 書き込み、取り消し、バックアップの途中でプロセスを kill しても、開き直すと最後に確定した版に戻ること。
//!
//! 子のプロセスはこのテストの実行ファイル自身を `crash_child` だけ走らせて作る。子は操作を当てる時間を
//! 省くため、全レイヤーのロックを反転する記録をあらかじめ作り、書き込みだけを繰り返す。
//! 親は子の WAL を見張り、確定していないフレームが現れた（SQLite が書き始めた）直後にばらけた間を置いて kill する。
//! 「本当に書き込みの最中に止めた」回は、kill の直後の WAL にそのフレームが残っていることで数える。
//! SQLite はページのキャッシュに収まる書き込みを確定の瞬間にまとめて書くので、窓を広げるために
//! レイヤーごとに大きな文字を持たせ、1 回の書き込みがキャッシュ（既定 2 MB）からあふれるようにする。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mm_doc::{Node, Project};
use mm_ops::OpContext;
use mm_store::{uncommitted_frames, OpenMode, Store, TxnInput, TxnKind};
use mm_types::ParamValue;

use crate::common::{actor, lock_records, locked, project_with, NOW};

const DIR_ENV: &str = "MM_STORE_CRASH_DIR";
const MODE_ENV: &str = "MM_STORE_CRASH_MODE";
/// 1 回のまとまりの操作の数（レイヤーの数）。書き込みの窓を広げるため多めにする。
const OPS_PER_TXN: usize = 500;
const ROUNDS: u64 = 20;
/// 書き込みの最中に止めた回がこれより少なければ、見張りが窓に当たっていない。
const MIN_MID_WRITE: u32 = 10;
/// 見張りの間隔と、子が終わるまでの待ちの上限。
const POLL: Duration = Duration::from_millis(1);
const DEADLINE: Duration = Duration::from_secs(20);
/// 書き始めを見てから kill するまでの、ばらけた余分の待ち（ミリ秒）の上限。
const EXTRA_WAIT_MS: u64 = 60;
/// 取り消しの試験で、親が先に書いておくまとまりの数。
const PREPARED_TXNS: usize = 4;
/// レイヤーごとの文字の長さ。全レイヤーで 3 MB になり、1 回の書き込みが SQLite のキャッシュからあふれる。
const TEXT_LEN: usize = 6_000;

/// 全レイヤーのロックを `toggles` 回反転した文書。
fn toggled(toggles: usize) -> Project {
    let mut p = project_with(OPS_PER_TXN);
    for l in &mut p.compositions[0].layers {
        l.lock = toggles % 2 == 1;
        // 検索の索引に入らない JSON のパラメータで行を大きくする（文字のパラメータは trigram の索引が遅い）。
        l.source = Node::new("solid", 1).with_param(
            "pad",
            ParamValue::Json(serde_json::Value::String("x".repeat(TEXT_LEN))),
        );
    }
    p
}

/// `toggles - 1` 回から `toggles` 回に反転する記録。
fn records_to(toggles: usize) -> Vec<mm_ops::OpRecord> {
    lock_records(OPS_PER_TXN, toggles % 2 == 1)
}

/// 子として動く。親が `DIR_ENV` を付けたときだけ、書き始めと確定のたびに 1 行出し続ける。
#[test]
fn crash_child() {
    let Ok(dir) = std::env::var(DIR_ENV) else {
        return;
    };
    let mode = std::env::var(MODE_ENV).unwrap_or_default();
    let mut store = Store::open(Path::new(&dir), NOW).unwrap();
    let base_toggles = (store.doc_version() / OPS_PER_TXN as i64) as usize;
    for i in 1.. {
        println!("begin {i}");
        match mode.as_str() {
            "undo" => {
                let Some(plan) = store.undo_plan().unwrap() else {
                    return;
                };
                let toggles = base_toggles - i as usize;
                let mut input = TxnInput::normal(&plan.name, actor(), NOW, records_to(toggles));
                input.kind = TxnKind::Undo {
                    of: plan.txn_id as u64,
                };
                store.commit(&toggled(toggles), &input).unwrap();
            }
            "backup" => {
                store.backup(NOW + i).unwrap();
            }
            _ => {
                let toggles = base_toggles + i as usize;
                let input = TxnInput::normal(format!("t{i}"), actor(), NOW, records_to(toggles));
                store.commit(&toggled(toggles), &input).unwrap();
            }
        }
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

/// kill の直後に見た様子。
struct Aftermath {
    /// 子が確定を表示した回数。
    confirmed: u64,
    /// WAL に確定していないフレームが残っていた（書き込みの最中に止めた）。
    mid_write: bool,
    /// 書きかけのバックアップが残っていた。
    mid_backup: bool,
}

fn backup_names(dir: &Path) -> Vec<String> {
    fs::read_dir(dir.join("backups")).map_or_else(
        |_| Vec::new(),
        |rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        },
    )
}

fn has_tmp_backup(dir: &Path) -> bool {
    backup_names(dir).iter().any(|n| n.contains(".tmp"))
}

/// 1 つ目の写しが残った後の、2 つ目以降の書きかけ。
fn writing_second_backup(dir: &Path) -> bool {
    let names = backup_names(dir);
    names.iter().any(|n| n.contains(".tmp")) && names.iter().any(|n| !n.contains(".tmp"))
}

/// 子を `mode` で動かし、書き始め（WAL の確定していないフレーム。バックアップは `.tmp`）を見たら
/// ばらけた間を置いて kill する。見ないまま子が終われば kill しない。
fn run_round(dir: &Path, mode: &str, seed: &mut u64) -> Aftermath {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "crash::crash_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(DIR_ENV, dir)
        .env(MODE_ENV, mode)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let db = dir.join("project.mmdb");
    let started = Instant::now();
    while started.elapsed() < DEADLINE && child.try_wait().unwrap().is_none() {
        let writing = if mode == "backup" {
            writing_second_backup(dir)
        } else {
            uncommitted_frames(&db).unwrap_or(0) > 0
        };
        if writing {
            std::thread::sleep(Duration::from_millis(next(seed) % EXTRA_WAIT_MS));
            break;
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    child.wait().unwrap();
    let mut confirmed = 0;
    let lines = BufReader::new(child.stdout.take().unwrap()).lines();
    for line in lines.map_while(Result::ok) {
        if let Some(n) = line.strip_prefix("committed ") {
            confirmed = n.parse().unwrap();
        }
    }
    Aftermath {
        confirmed,
        mid_write: uncommitted_frames(&db).unwrap() > 0,
        mid_backup: has_tmp_backup(dir),
    }
}

fn open_writer(dir: &Path, round: u64) -> Store {
    let store = Store::open(dir, NOW).unwrap();
    assert_eq!(
        store.mode(),
        OpenMode::Writer,
        "round {round}: ロックが残っている"
    );
    store
}

#[test]
fn 書き込みの途中で_kill_しても最後に確定した版に戻る() {
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    let mut mid_write = 0u32;
    for round in 0..ROUNDS {
        let tmp = tempfile::tempdir().unwrap();
        drop(Store::create(tmp.path(), &toggled(0), NOW).unwrap());
        let after = run_round(tmp.path(), "commit", &mut seed);
        mid_write += u32::from(after.mid_write);

        let mut store = open_writer(tmp.path(), round);
        let version = store.doc_version();
        let per = OPS_PER_TXN as i64;
        assert_eq!(
            version % per,
            0,
            "round {round}: まとまりの途中で止まっている"
        );
        let n = (version / per) as u64;
        // 確定の表示の後に落ちれば n は確定の回数、確定した直後で表示の前なら 1 多い。
        let confirmed = after.confirmed;
        assert!(
            n == confirmed || n == confirmed + 1,
            "round {round}: 確定 {confirmed} 回に対して復旧後は {n}"
        );
        assert_eq!(store.project(), &toggled(n as usize), "round {round}");
        let txns = store.txns_between(0, version).unwrap();
        assert_eq!(txns.len() as u64, n, "round {round}");
        assert!(txns.iter().all(|t| t.ops.len() == OPS_PER_TXN));

        let mut project = store.project().clone();
        let input = locked(&mut project, "after-crash", &[0, 1, 2]);
        store.commit(&project, &input).unwrap();
        assert_eq!(store.doc_version(), version + 3);
    }
    println!("{mid_write}/{ROUNDS} 回は書き込みの最中に止めた");
    assert!(
        mid_write >= MIN_MID_WRITE,
        "書き込みの最中に止めた回が {mid_write} 回しか無い"
    );
}

#[test]
fn 取り消しの途中で_kill_しても対で残らず_続けて取り消せる() {
    let mut seed = 0x2545_F491_4F6C_DD1D;
    let mut mid_write = 0u32;
    for round in 0..ROUNDS {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::create(tmp.path(), &toggled(0), NOW).unwrap();
        for i in 1..=PREPARED_TXNS {
            let input = TxnInput::normal(format!("p{i}"), actor(), NOW, records_to(i));
            store.commit(&toggled(i), &input).unwrap();
        }
        drop(store);
        let after = run_round(tmp.path(), "undo", &mut seed);
        mid_write += u32::from(after.mid_write);

        let mut store = open_writer(tmp.path(), round);
        let per = OPS_PER_TXN as i64;
        let version = store.doc_version();
        assert_eq!(
            version % per,
            0,
            "round {round}: 取り消しの途中で止まっている"
        );
        let undone = (version / per) as usize - PREPARED_TXNS;
        assert!(undone <= PREPARED_TXNS, "round {round}");
        let remaining = PREPARED_TXNS - undone;
        assert_eq!(store.undo_depth() as usize, remaining, "round {round}");
        assert_eq!(store.project(), &toggled(remaining), "round {round}");
        assert_eq!(store.txn_log().unwrap().len(), PREPARED_TXNS + undone);
        assert_eq!(
            store.txns_between(0, version).unwrap().len(),
            remaining,
            "round {round}: 取り消しは対で畳まれる"
        );
        let human = OpContext::human();
        if undone > 0 {
            store.redo_last(&human, actor(), NOW).unwrap().unwrap();
            assert_eq!(store.project(), &toggled(remaining + 1), "round {round}");
        } else {
            store.undo_last(&human, actor(), NOW).unwrap().unwrap();
            assert_eq!(store.project(), &toggled(remaining - 1), "round {round}");
        }
    }
    println!("{mid_write}/{ROUNDS} 回は取り消しの書き込みの最中に止めた");
    assert!(
        mid_write >= MIN_MID_WRITE,
        "取り消しの最中に止めた回が {mid_write} 回しか無い"
    );
}

#[test]
fn バックアップの途中で_kill_しても壊れた写しが残らない() {
    let mut seed = 0x1234_5678_9ABC_DEF1;
    let mut mid_backup = 0u32;
    let mut finished = 0u32;
    let rounds = ROUNDS / 2;
    for round in 0..rounds {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::create(tmp.path(), &toggled(0), NOW).unwrap();
        let input = TxnInput::normal("p", actor(), NOW, records_to(1));
        store.commit(&toggled(1), &input).unwrap();
        drop(store);
        let after = run_round(tmp.path(), "backup", &mut seed);
        mid_backup += u32::from(after.mid_backup);

        let store = open_writer(tmp.path(), round);
        assert_eq!(store.project(), &toggled(1));
        drop(store);
        let mut checked = 0;
        for entry in fs::read_dir(tmp.path().join("backups")).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap().to_string_lossy().contains(".tmp") {
                continue;
            }
            let copy = tempfile::tempdir().unwrap();
            fs::copy(&path, copy.path().join("project.mmdb")).unwrap();
            let restored = Store::open(copy.path(), NOW)
                .unwrap_or_else(|e| panic!("round {round}: {} が開けない: {e}", path.display()));
            assert_eq!(restored.project(), &toggled(1));
            checked += 1;
        }
        finished += u32::from(checked > 0);
    }
    println!("{mid_backup}/{rounds} 回はバックアップの最中に止め、{finished} 回は写しが残った");
    assert!(mid_backup > 0, "バックアップの最中に止められていない");
    assert!(finished > 0, "写しが 1 つも残らなかった");
}
