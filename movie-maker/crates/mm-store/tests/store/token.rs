//! HTTP の合い言葉の置き場所と権限。

use mm_store::Store;

use crate::common::{project_with, NOW};

#[test]
fn 合い言葉はプロジェクトの外に置かれ_lock_info_には識別子だけが載る() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = Store::create(tmp.path(), &project_with(1), NOW).unwrap();
    writer.publish_token("top-secret-token").unwrap();
    let path = Store::writer_token_file(tmp.path()).unwrap();
    assert!(!path.starts_with(tmp.path()), "プロジェクトの外: {path:?}");
    assert!(!tmp.path().join(".mm").join("token").exists());
    let info = std::fs::read_to_string(tmp.path().join(".mm").join("lock.info")).unwrap();
    assert!(!info.contains("top-secret-token"));
    assert!(info.contains("token_id"));
    assert!(Store::writer_alive(tmp.path()));
    drop(writer);
    assert!(!path.exists());
    assert!(!Store::writer_alive(tmp.path()));
}

#[cfg(windows)]
#[test]
fn 合い言葉のファイルは持ち主だけの_dacl_になる() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = Store::create(tmp.path(), &project_with(1), NOW).unwrap();
    writer.publish_token("secret").unwrap();
    let path = Store::writer_token_file(tmp.path()).unwrap();
    let out = std::process::Command::new("icacls")
        .arg(&path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let user = std::env::var("USERNAME").unwrap().to_lowercase();
    let entries: Vec<&str> = text.lines().filter(|l| l.contains(":(")).collect();
    assert_eq!(entries.len(), 1, "ACE は 1 つだけ: {text}");
    assert!(entries[0].to_lowercase().contains(&user), "{text}");
    // 先頭の行にはパス（C:\Users\...）が入るので、ACE の行だけで確かめる。
    for other in ["Everyone", "BUILTIN", "Authenticated", "SYSTEM"] {
        assert!(!entries[0].contains(other), "{other} が入っている: {text}");
    }
}

#[cfg(unix)]
#[test]
fn 合い言葉のファイルは_0600_でフォルダーは_0700() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let writer = Store::create(tmp.path(), &project_with(1), NOW).unwrap();
    writer.publish_token("secret").unwrap();
    let path = Store::writer_token_file(tmp.path()).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);
}
