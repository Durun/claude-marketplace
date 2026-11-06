//! feature `native` のときだけ、C のブリッジ（native/mm_ocio.cpp）をビルドして OCIO に繋ぎ、bindgen でバインディングを作る。

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OCIO_ROOT");
    if env::var_os("CARGO_FEATURE_NATIVE").is_none() {
        return;
    }
    native::build();
}

#[cfg(feature = "native")]
mod native {
    use std::env;
    use std::path::PathBuf;

    pub fn build() {
        let root = PathBuf::from(env::var("OCIO_ROOT").expect(
            "feature native には OCIO_ROOT（vcpkg の installed/x64-windows など）が要る。README を見る",
        ));
        let header = "native/mm_ocio.h";
        println!("cargo:rerun-if-changed={header}");
        println!("cargo:rerun-if-changed=native/mm_ocio.cpp");

        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .file("native/mm_ocio.cpp")
            .include("native")
            .include(root.join("include"))
            .compile("mm_ocio_shim");
        println!(
            "cargo:rustc-link-search=native={}",
            root.join("lib").display()
        );
        println!("cargo:rustc-link-lib=OpenColorIO");

        // 生成物に警告を出させないため、使う関数と型だけを許可し、レイアウト試験とドキュメントコメントを切る。
        let bindings = bindgen::Builder::default()
            .header(header)
            .allowlist_function("mm_ocio_.*")
            .allowlist_type("MmOcio.*")
            .layout_tests(false)
            .generate_comments(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("bindgen が失敗。LIBCLANG_PATH を確かめる");
        let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
        bindings
            .write_to_file(out.join("bindings.rs"))
            .expect("bindings.rs を書けない");

        copy_runtime_dlls(&root, &out);
    }

    /// Windows の実行時に OpenColorIO の DLL が見つかるよう、テストと実行のディレクトリへコピーする。
    fn copy_runtime_dlls(root: &std::path::Path, out: &std::path::Path) {
        // OUT_DIR は target/<profile>/build/<crate>-<hash>/out。3 つ上が target/<profile>。
        let Some(profile_dir) = out.ancestors().nth(3) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(root.join("bin")) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "dll") {
                for dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
                    if let Some(name) = path.file_name() {
                        // 失敗しても実行時に PATH で解決できるので、ビルドは止めない。
                        let _ = std::fs::copy(&path, dir.join(name));
                    }
                }
            }
        }
    }
}

#[cfg(not(feature = "native"))]
mod native {
    pub fn build() {}
}
