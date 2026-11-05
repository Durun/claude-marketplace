//! `VACUUM INTO` のバックアップと、世代の整理。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use mm_types::Result;

use crate::SqlExt;

/// バックアップの置き場（プロジェクトの下）。
pub(crate) const BACKUPS_DIR: &str = "backups";
const PREFIX: &str = "project.";
const SUFFIX: &str = ".mmdb";
const MS_PER_DAY: i64 = 86_400_000;
/// 新しい順に無条件で残す個数。
const KEEP_RECENT: usize = 10;
/// 日ごとに 1 個を残す日数。
const KEEP_DAILY_DAYS: i64 = 30;

/// 置き場にあるバックアップ 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    pub path: PathBuf,
    pub at_ms: i64,
}

/// UNIX のミリ秒を `20261106T120000123Z` の形にする（UTC。名前の並びが時刻の並びになる）。
pub fn format_timestamp(ms: i64) -> String {
    let days = ms.div_euclid(MS_PER_DAY);
    let rem = ms.rem_euclid(MS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// [`format_timestamp`] の逆。形が違えば `None`。
pub fn parse_timestamp(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 19 || b[8] != b'T' || b[18] != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let days = days_from_civil(num(0..4)?, num(4..6)?, num(6..8)?);
    let ms = num(9..11)? * 3_600_000 + num(11..13)? * 60_000 + num(13..15)? * 1000 + num(15..18)?;
    Some(days * MS_PER_DAY + ms)
}

// Howard Hinnant の civil 日付の算法。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// 整理で消すものを選ぶ。新しい順に 10 個と、それより古いものは日ごと（UTC）に 1 個（その日の最後）を
/// 30 日分残す。入力の順は問わない。
pub fn select_backups_to_delete(files: &[BackupFile], now_ms: i64) -> Vec<BackupFile> {
    let mut sorted: Vec<&BackupFile> = files.iter().collect();
    sorted.sort_by_key(|f| std::cmp::Reverse(f.at_ms));
    let mut delete = Vec::new();
    let mut last_day = None;
    for f in sorted.into_iter().skip(KEEP_RECENT) {
        let day = f.at_ms.div_euclid(MS_PER_DAY);
        let recent_enough = now_ms - f.at_ms <= KEEP_DAILY_DAYS * MS_PER_DAY;
        if recent_enough && last_day != Some(day) {
            last_day = Some(day);
        } else {
            delete.push(f.clone());
        }
    }
    delete
}

/// 置き場のバックアップを新しい順に返す。名前が形に合わないファイルは数えない。
pub(crate) fn list(project_dir: &Path) -> Vec<BackupFile> {
    let Ok(rd) = fs::read_dir(project_dir.join(BACKUPS_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<BackupFile> = rd
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path.file_name()?.to_str()?;
            let stamp = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
            Some(BackupFile {
                at_ms: parse_timestamp(stamp)?,
                path,
            })
        })
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.at_ms));
    out
}

/// 一貫した写しを `backups/project.<時刻>.mmdb` に作り、世代を整理する。
pub(crate) fn run(conn: &Connection, project_dir: &Path, now_ms: i64) -> Result<PathBuf> {
    let dir = project_dir.join(BACKUPS_DIR);
    fs::create_dir_all(&dir)?;
    let mut at = now_ms;
    let path = loop {
        let candidate = dir.join(format!("{PREFIX}{}{SUFFIX}", format_timestamp(at)));
        if !candidate.exists() {
            break candidate;
        }
        at += 1;
    };
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()])
        .sql()?;
    for f in select_backups_to_delete(&list(project_dir), now_ms) {
        let _ = fs::remove_file(f.path);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(at_ms: i64) -> BackupFile {
        BackupFile {
            path: PathBuf::from(format!("b{at_ms}")),
            at_ms,
        }
    }

    #[test]
    fn 時刻の文字は往復する() {
        for ms in [0, 1_793_880_000_123, 951_782_400_000, 4_102_444_799_999] {
            assert_eq!(parse_timestamp(&format_timestamp(ms)), Some(ms));
        }
        assert_eq!(format_timestamp(0), "19700101T000000000Z");
        assert_eq!(parse_timestamp("garbage"), None);
    }

    #[test]
    fn 直近_10_個と日ごとの_1_個を残す() {
        let now = 100 * MS_PER_DAY + 23 * 3_600_000;
        // 今日に 12 個（1 時間おき）。直近 10 個が残り、古い 2 個は同じ日の最後の 1 個だけ残る。
        let mut files: Vec<BackupFile> = (0..12).map(|i| file(now - i * 3_600_000)).collect();
        // 5 日前に 3 個、40 日前に 2 個。
        files.extend((0..3).map(|i| file(now - 5 * MS_PER_DAY - i * 1000)));
        files.extend((0..2).map(|i| file(now - 40 * MS_PER_DAY - i * 1000)));
        let del = select_backups_to_delete(&files, now);
        let kept: Vec<i64> = files
            .iter()
            .filter(|f| !del.contains(f))
            .map(|f| f.at_ms)
            .collect();
        assert_eq!(kept.len(), 10 + 1 + 1, "{kept:?}");
        assert!(kept.contains(&(now - 5 * MS_PER_DAY)), "その日の最後を残す");
        assert!(!kept.iter().any(|t| now - t > 30 * MS_PER_DAY));
    }

    #[test]
    fn 少なければ消さない() {
        let files: Vec<_> = (0..5).map(|i| file(i * 1000)).collect();
        assert!(select_backups_to_delete(&files, 5000).is_empty());
    }
}
