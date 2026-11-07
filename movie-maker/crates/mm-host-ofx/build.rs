//! C の部品を作る。
//! - `csrc/shim.c`: Rust の安定版では可変長引数の関数を定義できないので、OpenFX の可変長の関数
//!   （`paramGetValue`、`message` など）を C で受けて Rust の固定長の関数へ渡す。
//! - `tests/plugins/`: 試験に使う OpenFX のプラグインを、公式のヘッダーでバンドルの形に作る。
//!   ヘッダーで作ったプラグインと突き合わせることで、Rust 側の構造体の並びの誤りを捕まえる。
//!   置き場所は環境変数 `MM_OFX_TEST_PLUGINS` で試験に渡す。

use std::path::{Path, PathBuf};
use std::process::Command;

const INCLUDE: &str = "vendor/openfx/include";
/// 試験のプラグイン（ソース、バンドルの名前、C++ か）。
const TEST_PLUGINS: [(&str, &str, bool); 2] = [
    ("tests/plugins/invert.cpp", "Invert", true),
    ("tests/plugins/gain.c", "MmTestGain", false),
];

fn main() {
    println!("cargo:rerun-if-changed=csrc/shim.c");
    println!("cargo:rerun-if-changed={INCLUDE}");
    cc::Build::new()
        .file("csrc/shim.c")
        .include(INCLUDE)
        .warnings(true)
        .compile("mm_ofx_shim");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("plugins");
    for (src, name, cpp) in TEST_PLUGINS {
        println!("cargo:rerun-if-changed={src}");
        build_plugin(Path::new(src), name, cpp, &out);
    }
    println!("cargo:rustc-env=MM_OFX_TEST_PLUGINS={}", out.display());
}

/// OpenFX のバンドルの中のアーキテクチャのフォルダー名（OpenFX の仕様の「Packaging」）。
fn arch_dir() -> &'static str {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match os.as_str() {
        "windows" => "Win64",
        "macos" => "MacOS",
        _ => "Linux-x86-64",
    }
}

/// `<out>/<name>.ofx.bundle/Contents/<arch>/<name>.ofx` に共有ライブラリを作る。
fn build_plugin(src: &Path, name: &str, cpp: bool, out: &Path) {
    let dir = out
        .join(format!("{name}.ofx.bundle"))
        .join("Contents")
        .join(arch_dir());
    let obj = out.join(format!("obj-{name}"));
    for d in [&dir, &obj] {
        std::fs::create_dir_all(d).expect("出力のフォルダー");
    }
    let binary = dir.join(format!("{name}.ofx"));
    let tool = cc::Build::new()
        .cpp(cpp)
        .cargo_metadata(false)
        .get_compiler();
    let mut cmd: Command = tool.to_command();
    if tool.is_like_msvc() {
        cmd.args(["/nologo", "/LD", "/O2", "/EHsc"])
            .arg(format!("/I{INCLUDE}"))
            .arg(src)
            .arg(format!("/Fe{}", binary.display()))
            .arg(format!("/Fo{}\\", obj.display()))
            .arg("/link")
            .arg(format!(
                "/IMPLIB:{}",
                obj.join(format!("{name}.lib")).display()
            ));
    } else {
        cmd.args(["-shared", "-fPIC", "-O2"])
            .arg(format!("-I{INCLUDE}"))
            .arg(src)
            .arg("-o")
            .arg(&binary);
    }
    let status = cmd.status().expect("C コンパイラーを起動できない");
    assert!(status.success(), "{} を作れない: {status}", src.display());
}
