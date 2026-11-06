//! HTTP の合い言葉の置き場所。プロジェクトの外の、持ち主だけが読めるフォルダーに置く。
//!
//! プロジェクトのフォルダーは同期や共有の対象になりうるうえ、親フォルダーの ACL によっては
//! 別のアカウントにも読める。ユーザー専用のフォルダー（Windows は `%LOCALAPPDATA%`、
//! Unix は `$XDG_RUNTIME_DIR` か `~/.local/state`）に、フォルダーごと持ち主専用の権限で作る。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use mm_types::{MmError, Result};

const APP_DIR: &str = "movie-maker";
const TOKENS_DIR: &str = "tokens";
/// 識別子の 16 進の桁数。パスのハッシュの前半で、衝突しても別プロジェクトの合い言葉を上書きするだけ。
const ID_HEX_LEN: usize = 32;

/// プロジェクトのフォルダーから合い言葉の識別子を作る。
pub(crate) fn token_id(project_dir: &Path) -> String {
    let canonical = fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let mut hex = String::with_capacity(ID_HEX_LEN);
    for b in digest.iter().take(ID_HEX_LEN / 2) {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// 識別子の合い言葉のファイルのパス。
pub(crate) fn token_path(id: &str) -> Result<PathBuf> {
    Ok(tokens_root()?.join(id))
}

/// 合い言葉を持ち主専用のファイルに書く。
pub(crate) fn write_token(id: &str, token: &str) -> Result<PathBuf> {
    let root = tokens_root()?;
    ensure_private_dir(&root)?;
    let path = root.join(id);
    let mut file = create_private_file(&path)?;
    file.write_all(token.as_bytes())?;
    Ok(path)
}

/// 識別子の合い言葉を読む。無ければ `None`。
pub(crate) fn read_token(id: &str) -> Option<String> {
    let text = fs::read_to_string(token_path(id).ok()?).ok()?;
    let token = text.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

/// 合い言葉のファイルを消す。
pub(crate) fn remove_token(id: &str) {
    if let Ok(path) = token_path(id) {
        let _ = fs::remove_file(path);
    }
}

fn no_home() -> MmError {
    MmError::Internal(
        "合い言葉の置き場所（LOCALAPPDATA か XDG_RUNTIME_DIR か HOME）が分からない".into(),
    )
}

#[cfg(windows)]
fn tokens_root() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or_else(no_home)?;
    Ok(PathBuf::from(base).join(APP_DIR).join(TOKENS_DIR))
}

#[cfg(not(windows))]
fn tokens_root() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME").ok_or_else(no_home)?)
            .join(".local")
            .join("state"),
    };
    Ok(base.join(APP_DIR).join(TOKENS_DIR))
}

/// ファイルを作った後に権限を絞ると、その間は他のアカウントに読める。
/// フォルダーに先に権限を付け、中のファイルは作成時にそれを引き継ぐ。
#[cfg(unix)]
fn ensure_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?)
}

/// `icacls` で継承を切り、現在のユーザーだけにフルコントロールを与える。
#[cfg(windows)]
fn ensure_private_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let grant = format!("{}:(OI)(CI)F", current_user()?);
    let status = std::process::Command::new("icacls")
        .arg(dir)
        .args(["/inheritance:r", "/grant:r", &grant])
        .stdout(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(MmError::Internal(format!(
            "合い言葉のフォルダーの権限を絞れなかった: {status}"
        )))
    }
}

#[cfg(windows)]
fn current_user() -> Result<String> {
    let name = std::env::var("USERNAME").map_err(|_| no_home())?;
    Ok(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{name}"),
        _ => name,
    })
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> Result<fs::File> {
    Ok(OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?)
}
