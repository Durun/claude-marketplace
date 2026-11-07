//! OpenFX のプラグインの置き場所とバンドルの形（OpenFX の仕様の「Packaging OFX Plug-ins」）。
//! 探す場所は環境変数 `OFX_PLUGIN_PATH`（区切りは OS の `PATH` と同じ）と OS ごとの既定のフォルダー。

use std::path::{Path, PathBuf};

/// 追加の置き場所を並べる環境変数。
pub const PLUGIN_PATH_ENV: &str = "OFX_PLUGIN_PATH";
/// バンドルのフォルダー名の末尾。
pub const BUNDLE_SUFFIX: &str = ".ofx.bundle";
/// バンドルを探す深さの上限（ベンダーのフォルダーの下に置く製品がある）。
const MAX_DEPTH: usize = 4;

/// OS ごとの既定の置き場所。
pub fn default_dir() -> PathBuf {
    if cfg!(windows) {
        let common = std::env::var_os("CommonProgramFiles").map_or_else(
            || PathBuf::from(r"C:\Program Files\Common Files"),
            PathBuf::from,
        );
        common.join("OFX").join("Plugins")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/OFX/Plugins")
    } else {
        PathBuf::from("/usr/OFX/Plugins")
    }
}

/// 探す場所の一覧。`OFX_PLUGIN_PATH` を先に、既定の置き場所を後に。
pub fn plugin_dirs() -> Vec<PathBuf> {
    let extra = std::env::var_os(PLUGIN_PATH_ENV).unwrap_or_default();
    std::env::split_paths(&extra)
        .filter(|p| !p.as_os_str().is_empty())
        .chain(std::iter::once(default_dir()))
        .collect()
}

/// `dirs` の下のバンドル（`*.ofx.bundle`）を名前の順で並べる。読めないフォルダーは飛ばす。
pub fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in dirs {
        walk(dir, 0, &mut found);
    }
    found.sort();
    found.dedup();
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        if is_bundle(&path) {
            found.push(path);
        } else if depth + 1 < MAX_DEPTH {
            walk(&path, depth + 1, found);
        }
    }
}

fn is_bundle(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(BUNDLE_SUFFIX))
}

/// バンドルの中のアーキテクチャのフォルダー名。
pub fn arch_dir() -> &'static str {
    if cfg!(windows) {
        "Win64"
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else {
        "Linux-x86-64"
    }
}

/// バンドルの中の共有ライブラリ（`<名前>.ofx.bundle/Contents/<アーキテクチャ>/<名前>.ofx`）。
pub fn bundle_binary(bundle: &Path) -> Option<PathBuf> {
    let name = bundle.file_name()?.to_str()?.strip_suffix(".bundle")?;
    Some(bundle.join("Contents").join(arch_dir()).join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn バンドルの中の共有ライブラリの場所() {
        let bin = bundle_binary(Path::new("plugins/Foo.ofx.bundle")).expect("バンドル");
        assert_eq!(
            bin,
            Path::new("plugins/Foo.ofx.bundle/Contents")
                .join(arch_dir())
                .join("Foo.ofx")
        );
        assert_eq!(bundle_binary(Path::new("plugins/Foo")), None);
    }

    #[test]
    fn 入れ子のフォルダーのバンドルも見つける() {
        let root = std::env::temp_dir().join(format!("mm-host-ofx-scan-{}", std::process::id()));
        let nested = root.join("Vendor").join("B.ofx.bundle").join("Contents");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("A.ofx.bundle")).unwrap();
        std::fs::create_dir_all(root.join("not-a-bundle")).unwrap();
        let found = find_bundles(std::slice::from_ref(&root));
        let names: Vec<_> = found
            .iter()
            .filter_map(|p| p.file_name()?.to_str())
            .collect();
        assert_eq!(names, ["A.ofx.bundle", "B.ofx.bundle"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
