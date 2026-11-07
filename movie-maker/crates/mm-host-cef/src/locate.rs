//! アプリが起こすホストの実行ファイルを探す。CEF の配布物（`CEF_PATH`）が無い環境では探さない。

use std::path::{Path, PathBuf};

use crate::launcher::CEF_PATH_ENV;

/// ホストの実行ファイルを直に指す環境変数。`--features cef` で作ったものを指す。
pub const HOST_ENV: &str = "MM_CEF_HOST";
/// ホストの実行ファイルの名前（拡張子を除く）。
const HOST_STEM: &str = "mm-host-cef";

/// 起こすホストの実行ファイル。`CEF_PATH` が無ければ `None`。
/// `MM_CEF_HOST` があればそれ、無ければ今の実行ファイルと同じフォルダー（テストの実行ファイルなら 1 つ上も）の
/// `mm-host-cef`。どれも無ければ `None`。
pub fn find_host() -> Option<PathBuf> {
    std::env::var_os(CEF_PATH_ENV)?;
    if let Some(path) = std::env::var_os(HOST_ENV) {
        return Some(PathBuf::from(path)).filter(|p| p.is_file());
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = format!("{HOST_STEM}{}", std::env::consts::EXE_SUFFIX);
    let found = [Some(dir), dir.parent()]
        .into_iter()
        .flatten()
        .map(|d: &Path| d.join(&name))
        .find(|p| p.is_file());
    found
}
