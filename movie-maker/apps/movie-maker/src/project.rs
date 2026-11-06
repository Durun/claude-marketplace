//! プロジェクトを開く。入力は 1 つの JSON のファイルか、`project.mmdb` のあるフォルダー（`project.mmdb` の直指しも可）。
//! 素材の相対パスは、プロジェクトのあるフォルダーを基準に直す（どこから起動しても同じ素材を指すため）。

use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_store::Store;
use mm_types::{CompositionId, Id, MmError, Result};

/// `project.mmdb` のファイル名。
const DB_FILE: &str = "project.mmdb";
/// 再生するコンポジションを指定しなかったときの既定の ID。
const DEFAULT_COMPOSITION: &str = "main";

/// 開いたプロジェクト。
pub struct Opened {
    pub project: Project,
    /// 表示用の名前（フォルダー名か、JSON のあるフォルダー名）。
    pub name: String,
}

fn store_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return path.join(DB_FILE).exists().then(|| path.to_path_buf());
    }
    if path.file_name().is_some_and(|n| n == DB_FILE) && path.exists() {
        return Some(path.parent().unwrap_or(Path::new(".")).to_path_buf());
    }
    None
}

fn rebase(base: &Path, path: &str) -> String {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        path.to_owned()
    } else {
        base.join(p).to_string_lossy().into_owned()
    }
}

fn rebase_assets(project: &mut Project, base: &Path) {
    for asset in &mut project.assets {
        asset.path = rebase(base, &asset.path);
    }
}

/// プロジェクトを開く。ストアは書き手のロックを取らずに読む（再生は編集を止めない）。
pub fn open(path: &Path) -> Result<Opened> {
    if let Some(dir) = store_dir(path) {
        let store = Store::open_read_only(&dir)?;
        let mut project = store.project().clone();
        rebase_assets(&mut project, &dir);
        return Ok(Opened {
            project,
            name: folder_name(&dir),
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| {
        MmError::Io(std::io::Error::new(
            e.kind(),
            format!("{}: {e}", path.display()),
        ))
    })?;
    let mut project = mm_doc::load(&text)?;
    let base = path.parent().unwrap_or(Path::new("."));
    rebase_assets(&mut project, base);
    project.validate()?;
    Ok(Opened {
        project,
        name: folder_name(base),
    })
}

fn folder_name(dir: &Path) -> String {
    std::path::absolute(dir)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "project".into())
}

/// 再生するコンポジション。指定が無ければ `main`、それも無ければ先頭。
pub fn pick_composition(project: &Project, wanted: Option<&str>) -> Result<CompositionId> {
    if let Some(id) = wanted {
        let id: CompositionId = Id::new(id);
        project.composition(&id)?;
        return Ok(id);
    }
    let main: CompositionId = Id::new(DEFAULT_COMPOSITION);
    if project.composition(&main).is_ok() {
        return Ok(main);
    }
    project
        .compositions
        .first()
        .map(|c| c.id.clone())
        .ok_or_else(|| MmError::NotFound("コンポジションが 1 つも無い".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage2() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/stage2/project.json")
    }

    #[test]
    fn 入れ子の見本のプロジェクトが開けて素材のパスが直る() {
        let opened = open(&stage2()).expect("開ける");
        let id = pick_composition(&opened.project, None).unwrap();
        assert_eq!(id.as_str(), "main");
        assert!(Path::new(&opened.project.assets[0].path).starts_with(stage2().parent().unwrap()));
        assert_eq!(opened.name, "stage2");
    }

    #[test]
    fn main_は_scene_を_5_回入れ子にして_10_秒() {
        let project = open(&stage2()).unwrap().project;
        let main = project.composition(&Id::new("main")).unwrap();
        assert_eq!(main.duration.to_f64(), 10.0);
        let nested: Vec<_> = main
            .layers
            .iter()
            .filter(|l| l.source.kind == mm_doc::NESTED_KIND)
            .collect();
        assert_eq!(nested.len(), 5);
        let scene = project.composition(&Id::new("scene")).unwrap();
        let kinds: Vec<_> = scene
            .layers
            .iter()
            .map(|l| l.source.kind.as_str())
            .collect();
        assert_eq!(kinds, ["footage", "text"], "入れ子の中は素材と文字");
    }

    #[test]
    fn 指定したコンポジションが無ければ_not_found() {
        let project = open(&stage2()).unwrap().project;
        assert!(matches!(
            pick_composition(&project, Some("nope")),
            Err(MmError::NotFound(_))
        ));
    }

    #[test]
    fn 無いファイルはエラー() {
        assert!(open(Path::new("no/such/project.json")).is_err());
    }
}
