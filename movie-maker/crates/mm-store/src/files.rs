//! プロジェクトを場所から読む。入力は 1 つの JSON のファイルか、`project.mmdb` のあるフォルダー（`project.mmdb` の直指しも可）。
//! 素材と出力の相対パスは、プロジェクトのデータのあるフォルダーを基準に直す（どこから呼んでも同じ素材を指すため）。
//! `mm` とアプリが同じ決まりで開くよう、ここに 1 つだけ置く。

use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_types::{MmError, Result};

use crate::store::Store;

/// `project.mmdb` のファイル名。
pub const DB_FILE: &str = "project.mmdb";

/// フォルダーか `project.mmdb` を指していれば、その `project.mmdb` のあるフォルダー。
pub fn store_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return path.join(DB_FILE).exists().then(|| path.to_path_buf());
    }
    if path.file_name().is_some_and(|n| n == DB_FILE) && path.exists() {
        return Some(path.parent().unwrap_or(Path::new(".")).to_path_buf());
    }
    None
}

/// プロジェクトのデータを読み、相対パスを直す。ストアは書き手のロックを取らずに読む（描画や再生は編集を止めない）。
pub fn load_project(path: &Path) -> Result<Project> {
    let Some(dir) = store_dir(path) else {
        return load_json(path);
    };
    let store = Store::open_read_only(&dir)?;
    let mut project = store.project().clone();
    rebase_project(&mut project, &dir);
    Ok(project)
}

/// 1 つの JSON から読み、相対パスを JSON のあるフォルダーからの位置に直す。
pub fn load_json(path: &Path) -> Result<Project> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        let kind = e.kind();
        let detail = format!("プロジェクト {}: {e}", path.display());
        // 無いことと読めないこと（権限、ディレクトリ、壊れた文字コード）を区別して返す。
        if kind == std::io::ErrorKind::NotFound {
            MmError::NotFound(detail)
        } else {
            MmError::Io(std::io::Error::new(kind, detail))
        }
    })?;
    let mut project = mm_doc::load(&text)?;
    rebase_project(&mut project, path.parent().unwrap_or(Path::new(".")));
    Ok(project)
}

fn rebase_project(project: &mut Project, base: &Path) {
    for asset in &mut project.assets {
        asset.path = rebase(base, &asset.path);
    }
    for target in &mut project.render_targets {
        target.output_path = rebase(base, &target.output_path);
    }
}

fn rebase(base: &Path, path: &str) -> String {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        path.to_owned()
    } else {
        base.join(p).to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/projects")
            .join(name)
            .join("project.json")
    }

    #[test]
    fn json_の素材の相対パスが_json_のある所からになる() {
        let path = fixture("stage0");
        let project = load_project(&path).unwrap();
        assert!(project.assets[0].path.ends_with("cfr.mp4"));
        assert!(Path::new(&project.assets[0].path).starts_with(path.parent().unwrap()));
    }

    #[test]
    fn 読めないファイルは_not_found_に畳まず_io_で返す() {
        let dir = std::env::temp_dir().join(format!("mm_store_files_dir_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let result = load_project(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(result, Err(MmError::Io(_))), "{result:?}");
    }

    #[test]
    fn 無いファイルは_not_found() {
        assert!(matches!(
            load_project(Path::new("no/such/project.json")),
            Err(MmError::NotFound(_))
        ));
    }
}
