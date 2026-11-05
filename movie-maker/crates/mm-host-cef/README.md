# mm-host-cef

CEF のオフスクリーン描画を、共有テクスチャ（D3D11 → D3D12 → wgpu）で受け取る試作。
結果は [docs/design/spikes/cef.md](../../docs/design/spikes/cef.md)。

## ビルドの切り替え

| ビルド | CEF の配布物 | 動作 |
|---|---|---|
| 既定（`cargo build --workspace` など） | 要らない | 何もせず終了コード 2。`cef` と `wgpu` はコンパイルしない |
| `--features cef` | 要る | CEF を起動して測る |

`cef` は optional な依存の先にあり、既定のワークスペースのビルド、clippy、テストは CEF を取りに行かない。

## CEF を入れて動かす（Windows）

配布物（展開後 約 430 MB）は git に入れない。置き場は `CEF_PATH` で決める（以下は `D:/dev/cef/156`）。

1. 配布物を取る。`cef` クレートの付属ツールで、版は `cef` クレートと同じ 156.x に揃う。
   ```bash
   cargo install export-cef-dir
   export-cef-dir --force D:/dev/cef/156
   ```
2. C++ のラッパー（`cef-dll-sys`）のビルドに Ninja と CMake が要る。
   ```bash
   winget install Ninja-build.Ninja Kitware.CMake
   ```
3. 作る。`CEF_PATH` を見て配布物を使う（未設定だと `cef-dll-sys` が `OUT_DIR` へ自分で取りに行く）。
   `cargo` の target はパスを短くする。長いと CMake が `Cannot open compiler generated file` で落ちる。
   ```bash
   export CEF_PATH=D:/dev/cef/156 CARGO_TARGET_DIR=D:/dev/mmt
   cargo build --release -p mm-host-cef --features cef
   ```
4. 実行する。`libcef.dll` を `PATH` から見つけさせるので、`run.sh` を通す。
   ```bash
   crates/mm-host-cef/run.sh bench --frames 300 --warmup 30 --verify --out D:/dev/mm-spike-out
   ```

## bench の指定

| 指定 | 既定 | 意味 |
|---|---|---|
| `--path accel\|cpu` | accel | `OnAcceleratedPaint`（共有テクスチャ）か `OnPaint`（BGRA のアップロード） |
| `--frames` `--warmup` | 300 / 20 | 測るコマ数と、測らない助走 |
| `--size WxH` | 1920x1080 | 描く大きさ |
| `--fps` | 30 | コマの時刻の刻み（コマ `n` は `n / fps` 秒） |
| `--rate` | 240 | `windowless_frame_rate`。外部 BeginFrame でも描画の間隔の下限になる（60 だと 1 コマ 35 ms） |
| `--resend-us` | 3000 | 絵が来ないとき BeginFrame を送り直すまでの待ち |
| `--adapter` | 空 | wgpu の GPU を名前の一部で選ぶ。CEF は `force-high-performance-gpu` で外付けの GPU に揃える |
| `--verify` | 切 | 全コマを読み戻して古い絵と再描画の同一を確かめる。`--out` で PNG と `metrics-*.jsonl` |

`--名前=値` は Chromium のスイッチとして CEF に渡る。`MM_CEF_LOG=<ファイル>` で CEF の詳細ログを書く。
