//! プロジェクトを読む。場所からの読み方（JSON か `project.mmdb` のフォルダー、相対パスの直し）は
//! アプリと共有する `mm_store::load_project`。ここは別のフォルダーに作るための絶対パスへの直しだけを持つ。

use std::path::{Component, Path, PathBuf};

use mm_doc::Project;
use mm_types::Result;

pub use mm_store::load_project;

/// 1 つの JSON から読み、素材と出力を絶対パスに直す。別のフォルダーに作るプロジェクトのデータにする。
pub fn load_json_absolute(path: &Path) -> Result<Project> {
    let mut project = mm_store::load_json(path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 絶対パスに直すと_dotdot_が消える() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/projects/stage0/project.json");
        let project = load_json_absolute(&path).unwrap();
        let asset = Path::new(&project.assets[0].path);
        assert!(asset.is_absolute());
        assert!(!project.assets[0].path.contains(".."), "{asset:?}");
        assert_eq!(project.render_targets[0].codec, "h264");
    }
}
