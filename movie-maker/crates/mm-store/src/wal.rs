//! WAL（`project.mmdb-wal`）の内容の見分け。確定していない書きかけのフレームを数える。
//!
//! 書き込みの途中で止まったプロセスの WAL には、最後の確定の後ろにフレームが残る。
//! kill の試験が「本当に書き込みの最中に止めた」ことを確かめるのに使う。

use std::path::Path;

use mm_types::Result;

const HEADER_LEN: usize = 32;
const FRAME_HEADER_LEN: usize = 24;
/// 1 回の WAL の書き始めで決まる識別値（ヘッダーの 16..24）。フレームの 8..16 と一致しないものは古い残り。
const SALT: std::ops::Range<usize> = 16..24;
const FRAME_SALT: std::ops::Range<usize> = 8..16;
/// 確定のフレームだけ、確定後のページ数が 0 以外になる。
const FRAME_COMMIT: std::ops::Range<usize> = 4..8;
const PAGE_SIZE: std::ops::Range<usize> = 8..12;

/// `db` の WAL の、最後の確定より後ろにあるフレームの数。WAL が無ければ 0。
pub fn uncommitted_frames(db: &Path) -> Result<usize> {
    let path = db.with_file_name(format!(
        "{}-wal",
        db.file_name().unwrap_or_default().to_string_lossy()
    ));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    if bytes.len() < HEADER_LEN {
        return Ok(0);
    }
    let page_size = u32::from_be_bytes(bytes[PAGE_SIZE].try_into().unwrap_or([0; 4])) as usize;
    if page_size == 0 {
        return Ok(0);
    }
    let salt = &bytes[SALT];
    let mut since_commit = 0;
    let mut at = HEADER_LEN;
    while at + FRAME_HEADER_LEN + page_size <= bytes.len() {
        let frame = &bytes[at..at + FRAME_HEADER_LEN];
        if &frame[FRAME_SALT] != salt {
            break;
        }
        let commit = u32::from_be_bytes(frame[FRAME_COMMIT].try_into().unwrap_or([0; 4]));
        since_commit = if commit == 0 { since_commit + 1 } else { 0 };
        at += FRAME_HEADER_LEN + page_size;
    }
    Ok(since_commit)
}
