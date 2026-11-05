//! 最小の Tauri 2 のアプリ。WebView から 200ms ごとに呼ぶコマンドが差し替わるかを見る。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mm_doc::Project;
use tauri::State;

/// コマンド側（先端のクレート）の印。
const CMD_MARK: &str = "cmd-v1";

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// 文書と呼び出しの通し番号。Tauri の管理する状態に置くので patch をまたいで残る。
struct Doc {
    project: Mutex<Project>,
    seq: AtomicU64,
}

/// Tauri のコマンド。本体は subsecond::call で包み、patch 後は新しい関数に入る。
/// subsecond はポインタ 1 本分の大きさのクロージャを関数ポインタとみなして差し替えを素通りするので、
/// 参照に通し番号を足して 2 本分にする。
#[tauri::command]
fn apply_op(doc: State<'_, Doc>) -> String {
    let doc: &Doc = &doc;
    let seq = doc.seq.fetch_add(1, Ordering::Relaxed);
    subsecond::call(|| op_body(doc, seq))
}

fn op_body(doc: &Doc, seq: u64) -> String {
    let mut project = doc.project.lock().expect("文書の錠");
    let lib = ops_sample::set_opacity(&mut project, 0.5);
    let line = format!("{CMD_MARK} | {lib} | seq={seq}");
    println!("TICK {} | {line}", epoch_ms());
    line
}

/// Tauri を介さない対照。別スレッドから同じ形で call して、patch が届いているかを切り分ける。
const BG_MARK: &str = "bg-v1";

fn main() {
    // patch を受ける口。dx が起動時に渡す環境変数で接続先が決まる。
    dioxus_devtools::connect_subsecond();
    std::thread::spawn(|| loop {
        subsecond::call(|| println!("TICK {} | {BG_MARK}", epoch_ms()));
        std::thread::sleep(std::time::Duration::from_millis(200));
    });
    tauri::Builder::default()
        .manage(Doc {
            project: Mutex::new(ops_sample::demo_project()),
            seq: AtomicU64::new(0),
        })
        .invoke_handler(tauri::generate_handler![apply_op])
        .run(tauri::generate_context!())
        .expect("Tauri の起動");
}
