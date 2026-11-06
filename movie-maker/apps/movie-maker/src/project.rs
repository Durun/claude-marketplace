//! プロジェクトを開く。読み方（JSON か `project.mmdb` のフォルダー、相対パスの直し）は `mm` と共有する
//! `mm_store::load_project`。ここは表示の名前と、再生するコンポジションの選び方を持つ。

use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_store::{OpenMode, Store};
use mm_types::{CompositionId, Id, MmError, Result};

/// 再生するコンポジションを指定しなかったときの既定の ID。
const DEFAULT_COMPOSITION: &str = "main";

/// 開いたプロジェクト。
pub struct Opened {
    /// 素材のパスを絶対に直したプロジェクトのデータ。
    pub project: Project,
    /// 表示用の名前（フォルダー名か、JSON のあるフォルダー名）。
    pub name: String,
    /// 書き手として開いた `project.mmdb`。JSON のプロジェクトと、別のプロセスが書き手のときは `None`
    /// （そのときは操作を保存せず、メモリの上だけで適用する）。
    pub store: Option<Store>,
}

/// プロジェクトを開く。`project.mmdb` のフォルダーは書き手として開き、操作を保存して HTTP サーバーに出す。
/// 別のプロセスが書き手のときは、ロックを取らずに読む（再生は編集を止めない）。
pub fn open(path: &Path) -> Result<Opened> {
    let store_dir = mm_store::store_dir(path);
    let store = match &store_dir {
        Some(dir) => Some(Store::open(dir, now_ms())?).filter(|s| s.mode() == OpenMode::Writer),
        None => None,
    };
    let project = match &store {
        Some(s) => {
            let mut p = s.project().clone();
            mm_store::rebase_project(&mut p, s.dir());
            p
        }
        None => mm_store::load_project(path)?,
    };
    project.validate()?;
    let dir: PathBuf = match store_dir {
        Some(dir) => dir,
        None => path.parent().unwrap_or(Path::new(".")).to_path_buf(),
    };
    Ok(Opened {
        project,
        name: folder_name(&dir),
        store,
    })
}

fn now_ms() -> i64 {
    mm_serve::now_ms()
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
    fn 入れ子のフィクスチャのプロジェクトが開けて素材のパスが直る() {
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
