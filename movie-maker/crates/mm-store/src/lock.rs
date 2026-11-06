//! 書き手のロックファイル。`.mm/lock` に OS のファイルロックをかけ、pid と HTTP のポートは `.mm/lock.info` に書く。
//!
//! ロックはクラッシュしたプロセスのものも OS が解くので、古いロックファイルが残って締め出すことが無い。
//! HTTP の合い言葉は `.mm/token` に置く。
//! Windows では排他ロックされたファイルが読めないため、読む側が見る内容は別のファイルに置く。

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use mm_types::{MmError, Result};

/// 書き手の動作中を示すファイルの保存先（プロジェクトの下）。
const MM_DIR: &str = ".mm";
const LOCK_FILE: &str = "lock";
const INFO_FILE: &str = "lock.info";
const TOKEN_FILE: &str = "token";

/// 書き手の情報。`mm` はこれを見てアプリに操作を送る。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub http_port: Option<u16>,
}

/// 持っている間だけ書き手でいられる。ドロップするとロックが解ける。
#[derive(Debug)]
pub(crate) struct WriterLock {
    file: File,
    info_path: PathBuf,
    token_path: PathBuf,
}

impl WriterLock {
    /// ロックを取る。別のプロセスが持っていたら `None`。
    pub(crate) fn try_acquire(project_dir: &Path) -> Result<Option<Self>> {
        let dir = project_dir.join(MM_DIR);
        fs::create_dir_all(&dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => return Err(MmError::Io(e)),
        }
        let lock = Self {
            file,
            info_path: dir.join(INFO_FILE),
            token_path: dir.join(TOKEN_FILE),
        };
        // 前の書き手がクラッシュして残した合い言葉は、新しい書き手の物ではない。
        let _ = fs::remove_file(&lock.token_path);
        lock.write_info(&LockInfo {
            pid: std::process::id(),
            http_port: None,
        })?;
        Ok(Some(lock))
    }

    /// 情報（HTTP のポート）を書き直す。
    pub(crate) fn write_info(&self, info: &LockInfo) -> Result<()> {
        fs::write(&self.info_path, serde_json::to_vec(info)?)?;
        Ok(())
    }

    /// HTTP の合い言葉を書く。ファイルは書き手の持ち主だけが読める権限で作る。
    pub(crate) fn write_token(&self, token: &str) -> Result<()> {
        fs::write(&self.token_path, token)?;
        restrict_to_owner(&self.token_path)
    }

    /// 合い言葉を消す。
    pub(crate) fn clear_token(&self) {
        let _ = fs::remove_file(&self.token_path);
    }
}

/// Unix では 0600 にする。Windows はユーザーのプロファイルの ACL に任せる。
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<()> {
    Ok(())
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.token_path);
        let _ = fs::remove_file(&self.info_path);
        let _ = self.file.unlock();
    }
}

/// 動作中の書き手の HTTP の合い言葉を読む。無ければ `None`。
pub(crate) fn read_token(project_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(project_dir.join(MM_DIR).join(TOKEN_FILE)).ok()?;
    let token = text.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

/// 動作中の書き手の情報を読む。無ければ `None`。
pub(crate) fn read_info(project_dir: &Path) -> Option<LockInfo> {
    let bytes = fs::read(project_dir.join(MM_DIR).join(INFO_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}
