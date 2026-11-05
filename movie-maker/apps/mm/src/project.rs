//! プロジェクトの JSON を読み、相対パスをプロジェクトのあるディレクトリからの位置に直す。

use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_types::{MmError, Result};

/// 文書を読む。素材と出力の相対パスは、文書のあるディレクトリを基準にする
/// （どこから `mm` を呼んでも同じ素材を指すため）。
pub fn load_project(path: &Path) -> Result<Project> {
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
    for asset in &mut project.assets {
        asset.path = rebase(base, &asset.path);
    }
    for target in &mut project.render_targets {
        target.output_path = rebase(base, &target.output_path);
    }
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
    fn 見本のプロジェクトが読めて相対パスが直る() {
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
        // ディレクトリは存在するが文書としては読めない。
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
