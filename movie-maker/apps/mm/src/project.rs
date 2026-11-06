//! プロジェクトを読み、相対パスをプロジェクトのあるディレクトリからの位置に直す。
//! 入力は 1 つの JSON のファイルか、`project.mmdb` のあるフォルダー（`project.mmdb` の直指しも可）。

use std::path::{Component, Path, PathBuf};

use mm_doc::Project;
use mm_store::Store;
use mm_types::{MmError, Result};

/// `project.mmdb` のファイル名。
const DB_FILE: &str = "project.mmdb";

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

/// プロジェクトのデータを読む。素材と出力の相対パスは、プロジェクトのデータのあるディレクトリを基準にする
/// （どこから `mm` を呼んでも同じ素材を指すため）。
pub fn load_project(path: &Path) -> Result<Project> {
    let Some(dir) = store_dir(path) else {
        return load_json(path);
    };
    // 描画は数分かかるので、書き手のロックを取らずに読む（アプリの編集を止めない）。
    let store = Store::open_read_only(&dir)?;
    let mut project = store.project().clone();
    rebase_project(&mut project, &dir);
    Ok(project)
}

/// 1 つの JSON から読み、素材と出力を絶対パスに直す。別のフォルダーに作るプロジェクトのデータにする。
pub fn load_json_absolute(path: &Path) -> Result<Project> {
    let mut project = load_json(path)?;
    // 作業フォルダーを基準に絶対にしてから、`..` を字面で正規化する。
    for asset in &mut project.assets {
        asset.path = absolute(&asset.path)?;
    }
    for target in &mut project.render_targets {
        target.output_path = absolute(&target.output_path)?;
    }
    Ok(project)
}

fn absolute(path: &str) -> Result<String> {
    let abs = std::path::absolute(path)?;
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    Ok(out.to_string_lossy().into_owned())
}

fn rebase_project(project: &mut Project, base: &Path) {
    for asset in &mut project.assets {
        asset.path = rebase(base, &asset.path);
    }
    for target in &mut project.render_targets {
        target.output_path = rebase(base, &target.output_path);
    }
}

fn load_json(path: &Path) -> Result<Project> {
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
    let base = path.parent().unwrap_or(Path::new("."));
    rebase_project(&mut project, base);
    Ok(project)
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

    #[test]
    fn フィクスチャのプロジェクトが読めて相対パスが直る() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/projects/stage0/project.json");
        let project = load_project(&path).unwrap();
        assert_eq!(project.assets.len(), 1);
        assert!(project.assets[0].path.ends_with("cfr.mp4"));
        assert!(Path::new(&project.assets[0].path).starts_with(path.parent().unwrap()));
        assert_eq!(project.render_targets[0].codec, "h264");
    }

    #[test]
    fn 読めないファイルは_not_found_に畳まず_io_で返す() {
        // ディレクトリは存在するがプロジェクトのデータとしては読めない。
        let dir = std::env::temp_dir().join(format!("mm_project_dir_{}", std::process::id()));
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
