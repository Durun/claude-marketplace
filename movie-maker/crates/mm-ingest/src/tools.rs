//! 外部のツール（ffmpeg と ffprobe の CLI）の場所と起動。
//!
//! ライブラリの FFmpeg（`mm-decode-ffmpeg`）に依存しないので、SDK なしの既定の設定でもビルドでき、
//! エンコードは NVENC など CLI が持つ全部のエンコーダーを使える。

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mm_types::{MmError, Result};

/// ffmpeg の場所を指す環境変数。
pub const ENV_FFMPEG: &str = "MM_FFMPEG";
/// ffprobe の場所を指す環境変数。
pub const ENV_FFPROBE: &str = "MM_FFPROBE";
/// エラーに載せる標準エラー出力の末尾の文字数。
const STDERR_TAIL_CHARS: usize = 800;

/// ffmpeg と ffprobe の実行ファイル。
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Default for Tools {
    /// 環境変数 `MM_FFMPEG`、`MM_FFPROBE` があればそれ、なければ PATH の `ffmpeg`、`ffprobe`。
    fn default() -> Self {
        let pick = |var: &str, name: &str| {
            std::env::var_os(var).map_or_else(|| PathBuf::from(name), PathBuf::from)
        };
        Self {
            ffmpeg: pick(ENV_FFMPEG, "ffmpeg"),
            ffprobe: pick(ENV_FFPROBE, "ffprobe"),
        }
    }
}

impl Tools {
    /// 両方の実行ファイルが起動できるか。
    pub fn available(&self) -> bool {
        [&self.ffmpeg, &self.ffprobe].into_iter().all(|exe| {
            Command::new(exe)
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
    }

    /// ffmpeg のコマンド。ログは誤りだけにして、進み具合の行で標準エラーを埋めない。
    pub(crate) fn ffmpeg_command(&self) -> Command {
        let mut cmd = Command::new(&self.ffmpeg);
        cmd.args(["-hide_banner", "-nostdin", "-v", "error", "-y"]);
        cmd
    }
}

/// 終わるまで待ち、標準出力を返す。失敗したら標準エラーの末尾を載せたエラーにする。
pub(crate) fn run(cmd: &mut Command, what: &str) -> Result<Output> {
    let output = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| spawn_error(cmd, what, &e))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(MmError::Corrupt(failure_message(
            what,
            &output.stderr,
            output.status.code(),
        )))
    }
}

/// 起動できなかったときのエラー。実行ファイルが無ければ入れ方を促す。
pub(crate) fn spawn_error(cmd: &Command, what: &str, e: &std::io::Error) -> MmError {
    let exe = Path::new(cmd.get_program()).display().to_string();
    if e.kind() == std::io::ErrorKind::NotFound {
        MmError::Unsupported(format!(
            "{what}: {exe} が見つからない（PATH に通すか {ENV_FFMPEG} / {ENV_FFPROBE} で指す）"
        ))
    } else {
        MmError::Io(std::io::Error::new(e.kind(), format!("{what}: {exe}: {e}")))
    }
}

/// 失敗の説明。標準エラーは末尾だけ（先頭は設定の複製で、原因は末尾に出る）。
pub(crate) fn failure_message(what: &str, stderr: &[u8], code: Option<i32>) -> String {
    let text = String::from_utf8_lossy(stderr);
    let chars: Vec<char> = text.trim().chars().collect();
    let skip = chars.len().saturating_sub(STDERR_TAIL_CHARS);
    let tail: String = chars[skip..].iter().collect();
    format!("{what}: 終了コード {code:?}: {tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 標準エラーは末尾だけ載せる() {
        let long = "x".repeat(STDERR_TAIL_CHARS * 2) + "原因";
        let msg = failure_message("preview", long.as_bytes(), Some(1));
        assert!(msg.ends_with("原因"));
        assert!(msg.chars().count() < STDERR_TAIL_CHARS + 60);
    }

    #[test]
    fn 実行ファイルが無ければ入れ方を促す() {
        let mut cmd = Command::new("mm-no-such-exe");
        let e = cmd.output().unwrap_err();
        let err = spawn_error(&cmd, "probe", &e);
        assert!(matches!(err, MmError::Unsupported(m) if m.contains(ENV_FFMPEG)));
    }
}
