# movie-maker

汎用の動画編集・合成ソフト（ムービーメーカー）。Claude と人が同じ命令で編集し、Rust の描画器でその場で再生して書き出す。

設計は [docs/design/README.md](docs/design.md)。実装はまだない。

## 検査

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

FFmpeg 入りの検査は [crates/mm-decode-ffmpeg/README.md](crates/mm-decode-ffmpeg/README.md) の用意をしてから行う。
`--all-features` は OCIO の SDK（`mm-color-ocio/native`）と CEF の配布物（`mm-host-cef/cef`）も要るので、そろった環境でだけ使う。
