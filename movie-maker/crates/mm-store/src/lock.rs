//! 書き手のロックファイル。`.mm/lock` に OS のファイルロックをかけ、pid と HTTP のポートは `.mm/lock.info` に書く。
//!
//! ロックはクラッシュしたプロセスのものも OS が解くので、古いロックファイルが残って締め出すことが無い。
//! HTTP の合い言葉はプロジェクトの外の持ち主専用のフォルダーに置き（`token.rs`）、
//! `lock.info` にはその識別子（`token_id`）だけを書く。
//! Windows では排他ロックされたファイルが読めないため、読む側が見る内容は別のファイルに置く。

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use mm_types::{MmError, Result};

use crate::token;

/// 書き手の動作中を示すファイルの保存先（プロジェクトの下）。
const MM_DIR: &str = ".mm";
const LOCK_FILE: &str = "lock";
const INFO_FILE: &str = "lock.info";

/// 書き手の情報。`mm` はこれを見てアプリに操作を送る。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub http_port: Option<u16>,
    /// 合い言葉の置き場所の識別子。合い言葉そのものは持たない。
    #[serde(default)]
    pub token_id: Option<String>,
}

/// 持っている間だけ書き手でいられる。ドロップするとロックが解ける。
#[derive(Debug)]
pub(crate) struct WriterLock {
    file: File,
    info_path: PathBuf,
    token_id: String,
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
            token_id: token::token_id(project_dir),
        };
        // 前の書き手がクラッシュして残した合い言葉は、新しい書き手の物ではない。
        token::remove_token(&lock.token_id);
        lock.write_info(None)?;
        Ok(Some(lock))
    }

    /// 情報（HTTP のポート）を書き直す。
    pub(crate) fn write_info(&self, http_port: Option<u16>) -> Result<()> {
        let info = LockInfo {
            pid: std::process::id(),
            http_port,
            token_id: Some(self.token_id.clone()),
        };
        fs::write(&self.info_path, serde_json::to_vec(&info)?)?;
        Ok(())
    }

    /// HTTP の合い言葉を書く。置き場所は持ち主だけが読める。
    pub(crate) fn write_token(&self, secret: &str) -> Result<()> {
        token::write_token(&self.token_id, secret)?;
        Ok(())
    }

    /// 合い言葉を消す。
    pub(crate) fn clear_token(&self) {
        token::remove_token(&self.token_id);
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        token::remove_token(&self.token_id);
        let _ = fs::remove_file(&self.info_path);
        let _ = self.file.unlock();
    }
}

/// 動作中の書き手の HTTP の合い言葉を読む。無ければ `None`。
pub(crate) fn read_token(project_dir: &Path) -> Option<String> {
    let id = read_info(project_dir)?.token_id?;
    token::read_token(&id)
}

/// 合い言葉のファイルのパス（権限の検査用）。
pub(crate) fn token_file(project_dir: &Path) -> Option<PathBuf> {
    token::token_path(&read_info(project_dir)?.token_id?).ok()
}

/// 動作中の書き手の情報を読む。無ければ `None`。
pub(crate) fn read_info(project_dir: &Path) -> Option<LockInfo> {
    let bytes = fs::read(project_dir.join(MM_DIR).join(INFO_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 書き手のプロセスが生きているか。ロックが取れてしまえば、残っている情報はクラッシュの跡。
pub(crate) fn writer_alive(project_dir: &Path) -> bool {
    let Ok(file) = File::open(project_dir.join(MM_DIR).join(LOCK_FILE)) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}
