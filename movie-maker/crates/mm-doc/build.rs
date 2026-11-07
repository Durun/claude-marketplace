//! `src/migrations/` の名前つきの移行を集め、名前の順の表 `migrations.rs` を `OUT_DIR` に書く。
//! 1 つの移行を 1 つのファイルにし、表をディレクトリから作るので、並行の PR が同じ行を書き換えない（sqlx の `migrate!` と同じ型）。

use std::fmt::Write as _;
use std::path::Path;

/// 移行のファイルの名前の日付の桁数（`YYYYMMDD`）。
const DATE_DIGITS: usize = 8;

fn main() {
    let src = Path::new(&env("CARGO_MANIFEST_DIR")).join("src");
    // 移行のディレクトリはまだ無いことがあり、無いパスを見張ると毎回実行し直すので、必ずある src を見張る。
    println!("cargo:rerun-if-changed={}", src.display());
    let dir = src.join("migrations");
    let mut names = migration_names(&dir);
    names.sort();
    let out = Path::new(&env("OUT_DIR")).join("migrations.rs");
    std::fs::write(&out, table(&dir, &names)).expect("migrations.rs を書けない");
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} が無い"))
}

/// `src/migrations/*.rs` のファイルの名前（拡張子なし）。ディレクトリが無ければ空。
fn migration_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .map(|e| e.expect("src/migrations を読めない").path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            assert!(
                valid_name(stem),
                "移行のファイルの名前 {stem}.rs は YYYYMMDD_<英小文字・数字・_> にする"
            );
            stem.to_owned()
        })
        .collect()
}

fn valid_name(stem: &str) -> bool {
    let (date, slug) = stem.split_at(stem.len().min(DATE_DIGITS));
    date.len() == DATE_DIGITS
        && date.bytes().all(|b| b.is_ascii_digit())
        && slug.len() > 1
        && slug.starts_with('_')
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 各ファイルを `m<名前>` のモジュールとして読み込み、`(名前, up)` を名前の順に並べた表。
fn table(dir: &Path, names: &[String]) -> String {
    let mut s = String::new();
    for n in names {
        let path = dir.join(format!("{n}.rs"));
        writeln!(s, "#[path = {:?}]\nmod m{n};", path.display().to_string()).unwrap();
    }
    s.push_str("/// 名前つきの移行の全部。名前（日付が先頭）の順。\n");
    s.push_str("pub(crate) static MIGRATIONS: &[Migration] = &[\n");
    for n in names {
        writeln!(s, "    Migration {{ name: {n:?}, up: m{n}::up }},").unwrap();
    }
    s.push_str("];\n");
    s
}
