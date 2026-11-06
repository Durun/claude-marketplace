//! クラウド同期のフォルダーとネットワークのファイルシステムの検出。
//!
//! 同期ツールは DB のロックを守らずにファイルを書き換えるので、置かれたら警告する（開くのは拒否しない）。
//! 検出はパスの名前だけで決める。ファイルの属性（`RECALL_ON_DATA_ACCESS`）は見ない。

use std::path::{Component, Path};

/// 同期の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    OneDrive,
    Dropbox,
    GoogleDrive,
    ICloud,
    /// SMB、NFS など。WAL は同じホストの中でしか動かない。
    NetworkFs,
}

/// 検出の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFolder {
    pub kind: SyncKind,
    /// 適用されたパスの部品。
    pub matched: String,
}

/// 環境変数で教えられる OneDrive の場所。
const ONEDRIVE_ENV: [&str; 3] = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"];

/// パスが同期のフォルダーかネットワークの下なら、その種類を返す。
pub fn detect_sync_folder(path: &Path) -> Option<SyncFolder> {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    detect_network(&abs)
        .or_else(|| detect_by_env(&abs))
        .or_else(|| detect_by_name(&abs))
}

fn detect_network(path: &Path) -> Option<SyncFolder> {
    let first = path.components().next()?;
    let Component::Prefix(prefix) = first else {
        return None;
    };
    use std::path::Prefix::{VerbatimUNC, UNC};
    matches!(prefix.kind(), UNC(..) | VerbatimUNC(..)).then(|| SyncFolder {
        kind: SyncKind::NetworkFs,
        matched: prefix.as_os_str().to_string_lossy().into_owned(),
    })
}

fn detect_by_env(path: &Path) -> Option<SyncFolder> {
    ONEDRIVE_ENV.iter().find_map(|var| {
        let root = std::env::var_os(var)?;
        if root.is_empty() || !path.starts_with(&root) {
            return None;
        }
        Some(SyncFolder {
            kind: SyncKind::OneDrive,
            matched: root.to_string_lossy().into_owned(),
        })
    })
}

fn detect_by_name(path: &Path) -> Option<SyncFolder> {
    path.components().find_map(|c| {
        let name = c.as_os_str().to_string_lossy();
        classify_name(&name.to_lowercase()).map(|kind| SyncFolder {
            kind,
            matched: name.into_owned(),
        })
    })
}

fn classify_name(lower: &str) -> Option<SyncKind> {
    if lower.starts_with("onedrive") {
        Some(SyncKind::OneDrive)
    } else if lower == "dropbox" || lower.starts_with("dropbox (") {
        Some(SyncKind::Dropbox)
    } else if matches!(lower, "my drive" | "googledrive") || lower.starts_with("google drive") {
        Some(SyncKind::GoogleDrive)
    } else if matches!(
        lower,
        "iclouddrive" | "icloud drive" | "com~apple~clouddocs" | "mobile documents"
    ) {
        Some(SyncKind::ICloud)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(p: &str) -> Option<SyncKind> {
        detect_by_name(Path::new(p)).map(|s| s.kind)
    }

    #[test]
    fn 同期のフォルダーを名前で見つける() {
        assert_eq!(
            kind("C:/Users/a/OneDrive - 会社/proj"),
            Some(SyncKind::OneDrive)
        );
        assert_eq!(kind("C:/Users/a/Dropbox/proj"), Some(SyncKind::Dropbox));
        assert_eq!(kind("G:/My Drive/proj"), Some(SyncKind::GoogleDrive));
        assert_eq!(kind("C:/Users/a/iCloudDrive/proj"), Some(SyncKind::ICloud));
        assert_eq!(kind("E:/git-repos/proj"), None);
    }

    #[test]
    fn 名前が似ているだけのフォルダーは適用しない() {
        assert_eq!(kind("E:/dev/dropbox-clone/proj"), None);
    }

    #[cfg(windows)]
    #[test]
    fn unc_はネットワークとして適用する() {
        let found = detect_sync_folder(Path::new(r"\\server\share\proj")).unwrap();
        assert_eq!(found.kind, SyncKind::NetworkFs);
    }
}
