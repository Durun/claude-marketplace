# FFmpeg のライブラリの用意（mm-decode-ffmpeg / mm-encode-ffmpeg）

2 つのクレートの実装と契約テストは feature `ffmpeg` の下にあり、既定では切ってある。FFmpeg も libclang も無い環境（CI、ほかの担当）では、feature なしで `cargo fmt` / `clippy` / `test --workspace` が通る（この 2 クレートは空になる）。
FFmpeg を使うときは下の用意をして、`--features ffmpeg` を付ける。ワークスペース全体の検査は feature を明示する（`--all-features` は OCIO の SDK と CEF の配布物も要る）。
`mm-decode-ffmpeg` の feature `gpu`（`ffmpeg` を含む。`mm-runtime` の `ffmpeg` が入れる）は、GPU のコンテキストを渡されたとき YUV のプレーンをそのまま GPU に上げてシェーダーで線形にする。4K で CPU の RGBA への変換が 75 ms/フレームなのに対し 1.7 ms（`examples/bench_upload.rs`）。

```sh
cargo test -p mm-decode-ffmpeg -p mm-encode-ffmpeg --features ffmpeg
cargo clippy --workspace --all-targets -- -D warnings \
  --features mm-decode-ffmpeg/ffmpeg,mm-encode-ffmpeg/ffmpeg,mm-runtime/ffmpeg,mm-decode-d3d12va/d3d12va,mm-decode-cuvid/cuvid
```

ワークスペースのビルドでは `ffmpeg-sys-next` の build script が主スレッドのスタックを使い切ってクラッシュする（`STATUS_STACK_OVERFLOW`）。
`.cargo/config.toml` が MSVC のリンカーにスタック 32 MB を要求して防ぐ（`x86_64-pc-windows-msvc` のみ。リンクする全バイナリに効く）。

feature ありのビルドは `ffmpeg-next`（FFmpeg 9.0 の系）でビルドする。必要なのは FFmpeg の shared ビルド（ヘッダー、import ライブラリ、DLL）と libclang。
テストの素材づくりと検証には ffmpeg / ffprobe の CLI も PATH に要る。

## Windows

1. FFmpeg 9.0 の shared ビルドを展開する。BtbN の `ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip`（`FFmpeg-Builds` のリリース）で動作確認した。libx264 入り。
2. LLVM を入れる（`winget install LLVM.LLVM`）。bindgen が `libclang.dll` を使う。
3. 環境変数を置く。

| 変数 | 値 |
|---|---|
| `FFMPEG_DIR` | 展開した FFmpeg のルート（`include/` と `lib/` がある所） |
| `LIBCLANG_PATH` | `C:/Program Files/LLVM/bin` |
| `PATH` | 展開した FFmpeg の `bin/`（実行時に DLL を探す。`cargo test` にも要る） |

```sh
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:$FFMPEG_DIR/bin"
cargo test -p mm-decode-ffmpeg -p mm-encode-ffmpeg --features ffmpeg
```

## Linux / macOS

`pkg-config` で見つかる位置に FFmpeg の開発パッケージ（libav*-dev）と libclang を入れる。`FFMPEG_DIR` は要らない。

## テストの素材

`fixtures/media/generate.sh`（ffmpeg の CLI）が `fixtures/media/out/` に作る。生成物は git に入れない。
テストは `out/.stamp` が無いときだけ `sh generate.sh` を自分で呼ぶ。手で作り直すなら `out/` を消して同じスクリプトを実行する。
フレームの番号は左上の 16x16 の箱 16 個の白黒に焼き込んであり、デコード後の画素からリードバックする（`tests/it/common/mod.rs`）。
