//! FFmpeg 系の統合テストが共有する補助。フィクスチャの素材の用意と、フレームの番号の読み書き。
//!
//! 実素材のフレーム番号は、左上の 16x16 の箱 16 個（下位ビットが左）の白黒に焼き込んである
//! （`fixtures/media/generate.sh`）。モックの実装の「G 成分に焼き込む」やり方は、
//! 圧縮と色変換で値が崩れる実素材では使えない。

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use mm_types::RgbaImage;

/// 番号を表す箱の数と、1 つの箱の一辺（画素）。
pub const BIT_COUNT: u32 = 16;
pub const BIT_SIZE: u32 = 16;

fn media_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/media")
}

/// 生成済みのフィクスチャの素材のディレクトリ。無ければ `generate.sh` で作る。
pub fn fixtures() -> PathBuf {
    let root = media_root();
    let out = root.join("out");
    let stamp = out.join(".stamp-3");
    let lock = root.join(".generate.lock");
    let deadline = Instant::now() + Duration::from_secs(180);
    while !stamp.exists() {
        // 複数のテストのプロセスが同時に作らないよう、ロックを取れた 1 つだけが作る
        if OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .is_ok()
        {
            let result = run_generate(&root);
            let _ = std::fs::remove_file(&lock);
            result.expect("フィクスチャの素材を作れた");
        } else if Instant::now() > deadline {
            panic!(
                "フィクスチャの素材の生成を待ち切れない（{} が残っていないか）",
                lock.display()
            );
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    out
}

fn run_generate(root: &std::path::Path) -> Result<(), String> {
    let script = root.join("generate.sh");
    let shells = [
        "sh",
        "C:/Program Files/Git/bin/sh.exe",
        "C:/Program Files/Git/usr/bin/sh.exe",
    ];
    for sh in shells {
        match Command::new(sh).arg(&script).status() {
            Ok(s) if s.success() => return Ok(()),
            Ok(s) => return Err(format!("{sh} {}: {s}", script.display())),
            Err(_) => continue,
        }
    }
    Err(format!(
        "sh が見つからない。先に {} を実行する",
        script.display()
    ))
}

/// デコードしたフレームから焼き込んだ番号を読む。
pub fn read_frame_number(image: &RgbaImage) -> i64 {
    (0..BIT_COUNT)
        .map(|k| {
            let white = image.pixel(k * BIT_SIZE + BIT_SIZE / 2, BIT_SIZE / 2).r > 0.5;
            (white as i64) << k
        })
        .sum()
}
