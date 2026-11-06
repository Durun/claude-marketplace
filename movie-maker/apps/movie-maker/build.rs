//! Tauri は `frontendDist`（`ui/dist/`）が無いとビルドできない。`ui/dist/` は `pnpm build` の生成物で git に入れないので、
//! 無いときだけ画面の無い仮の index.html を置く（CI のように UI を作らない設定でも Rust が通る）。

use std::path::Path;

const DIST_INDEX: &str = "../../ui/dist/index.html";
const PLACEHOLDER: &str = "<!doctype html><meta charset=\"utf-8\"><title>movie-maker</title>\
<body>ui/dist/ がありません。movie-maker/ui で pnpm install && pnpm build を実行してください。";

fn main() {
    let index = Path::new(DIST_INDEX);
    if !index.exists() {
        if let Some(dir) = index.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(index, PLACEHOLDER);
    }
    println!("cargo:rerun-if-changed={DIST_INDEX}");
    tauri_build::build();
}
