# CI

`.github/workflows/movie-maker.yml`。`movie-maker/**` か workflow 自体を変えた PR、main への push、手動の起動（`workflow_dispatch`）で走る。権限は `contents: read` だけで、秘密の値は使わない。Rust の版は `rust-toolchain.toml` に従う。

| ジョブ | 環境 | いつ | 中身 |
|---|---|---|---|
| `check` | windows-latest | PR、main、手動 | `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。外の SDK（FFmpeg、OCIO、CEF）無しの既定の設定。GPU は無いので GPU のテストは飛ぶ（`MM_REQUIRE_GPU` は付けない） |
| `ffmpeg` | windows-latest | PR、main、手動 | BtbN の FFmpeg 9.0 の shared ビルドを展開し、feature `ffmpeg` などを入れた clippy とテスト、`mm render fixtures/projects/stage0/project.json` の書き出し。LLVM は runner のものを使い、無ければ choco で入れる。手順の元は [mm-decode-ffmpeg の README](../crates/mm-decode-ffmpeg/README.md) |
| `semver` | ubuntu-latest | PR のみ | `cargo-semver-checks` で `crates/mm-*-api` の全部を PR の基点（`origin/<base>`）と比べる |

## 口の版の検査

公開しないクレート（`publish = false`、版 0.1.0）でも `--baseline-rev` で動くことを確かめた。公開メソッドを消すと「次の版（0.x では minor）を上げろ」で落ちる。
口を壊す PR は、版を上げるか、壊さない形にする。`workspace.package.version` は全クレートで共有なので、版の上げ方はルートの `Cargo.toml` の担当（統合）に合わせる。

## 手元で回す

`movie-maker/` で実行する。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
sh scripts/check-ffmpeg.sh clippy   # FFmpeg あり（FFMPEG_DIR などは README のとおり）
sh scripts/check-ffmpeg.sh render
cargo install cargo-semver-checks --locked
cargo semver-checks $(for d in crates/mm-*-api; do printf -- '-p %s ' "$(basename "$d")"; done) --baseline-rev origin/main
```

Windows でワークツリーのパスが長いと、semver-checks の基点のビルドが `LNK1104` で落ちる。`CARGO_TARGET_DIR=C:/t/sc` のように短い場所を指す。
