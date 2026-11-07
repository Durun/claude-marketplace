# mm-host-cef

CEF のオフスクリーン描画を、共有テクスチャで別プロセスのアプリへ渡すホスト（`mm-host-api` の実装）。
試作の結果とつまずいた所は [docs/design/spikes/cef.md](../../docs/design/spikes/cef.md)。

## 分け方

| 部分 | 場所 | CEF の配布物 |
|---|---|---|
| アプリ側のランチャー（`CefLauncher`、`PipeHost`） | `src/launcher.rs` | 要らない。既定のビルドでコンパイルする |
| ホストの実行ファイル（`serve` と `bench`） | `src/host/` | 要る（`--features cef`） |
| 試験（契約、期待画像、古いフレーム、往復の時間） | `tests/serve.rs` | 要る。`CEF_PATH` が無ければ飛ばす（`MM_REQUIRE_CEF=1` で失敗にする） |

既定のワークスペースのビルド、clippy、テストは CEF を取りに行かない。`--features cef` なしの実行ファイルは何もせず終了コード 2。

## 受け渡しの約束

- `CefLauncher::spawn` は `mm-host-cef serve` を起こし、環境変数 `MM_HOST_SPEC` に `HostSpec` の JSON を渡す。制御は標準入出力の JSON の 1 行ずつ。
- GPU: ホストは `HostSpec::gpu`（無ければ高性能のアダプター）で wgpu を作り、その LUID を Chromium のスイッチ `use-adapter-luid=<HighPart>,<LowPart>` で CEF の GPU プロセスにも使わせる。
- `client_pid` があると、フレームごとに D3D12 の共有ヒープのテクスチャを作ってコピーし、その NT ハンドルを相手のプロセスへ `DuplicateHandle` して `SharedFrame::SharedTexture` で返す。形式は `DXGI_FORMAT_B8G8R8A8_UNORM`（premultiplied、sRGB の符号化）、状態は COMMON、`fence` は `None`（ホストがコピーの完了を待ってから返す）。受け取った側がコピーしてハンドルを閉じる。
- `client_pid` が無いと、CPU の画像（RGBA f32、premultiplied、線形）で返す。
- `Prepare` の `params` は `{"html": "..."}` か `{"url": "..."}`。ページは `window.mmSetTime(t, params)` を定義し、時刻 `t`（秒）と `params` の関数としてだけ画面を作る。ビューポートは `RenderFrame` の `size`。
- 時刻を変えても画面が変わらないページは CEF がフレームを出さないので、60 ms 待って前のフレームを返す。

## CEF を入れて動かす（Windows）

配布物（展開後 約 430 MB）は git に入れない。配置先は `CEF_PATH` で決める（以下は `D:/dev/cef/156`）。

1. 配布物を取る。`cef` クレートの付属ツールで、バージョンは `cef` クレートと同じ 156.x に揃う。
   ```bash
   cargo install export-cef-dir
   export-cef-dir --force D:/dev/cef/156
   ```
2. C++ のラッパー（`cef-dll-sys`）のビルドに Ninja と CMake が要る。
   ```bash
   winget install Ninja-build.Ninja Kitware.CMake
   ```
3. 作る。`CEF_PATH` を見て配布物を使う（未設定だと `cef-dll-sys` が `OUT_DIR` へ自分で取りに行く）。
   `cargo` の target はパスを短くする。長いと CMake が `Cannot open compiler generated file` で失敗する。
   ```bash
   export CEF_PATH=D:/dev/cef/156 CARGO_TARGET_DIR=D:/dev/mmt
   cargo build --release -p mm-host-cef --features cef
   ```
4. 試験。ランチャーが `CEF_PATH` を子プロセスの `PATH` の先頭に足すので、そのまま走る。
   ```bash
   cargo test --release -p mm-host-cef --features cef -- --nocapture
   ```
   期待画像は `fixtures/golden/rtx/cef-frame.png`。更新は `MM_GOLDEN_UPDATE=1` を付けて走らせ、目で確かめてからコミットする。
5. 測る。`libcef.dll` を `PATH` から見つけさせるので、`run.sh` を通す。
   ```bash
   crates/mm-host-cef/run.sh bench --frames 300 --warmup 30 --verify --out D:/dev/mm-spike-out
   ```
   `serve` のスパンは、環境変数 `MM_HOST_METRICS=<ファイル>` を付けると終わるときに `metrics.jsonl` の 1 行で書く。

## bench の指定

| 指定 | 既定 | 意味 |
|---|---|---|
| `--path accel\|cpu` | accel | `OnAcceleratedPaint`（共有テクスチャ）か `OnPaint`（BGRA のアップロード） |
| `--share` | 切 | コピー先を共有ヒープに作り、自分のプロセスへハンドルを複製するところまで測る（`serve` と同じ経路） |
| `--frames` `--warmup` | 300 / 20 | 測るフレーム数と、測らない助走 |
| `--size WxH` | 1920x1080 | 描く大きさ |
| `--fps` | 30 | フレームの時刻の刻み（フレーム `n` は `n / fps` 秒） |
| `--rate` | 1000 | `windowless_frame_rate`。フレームを受け取る間隔の下限になる（下の注意） |
| `--resend-us` | 3000 | フレームが来ないとき BeginFrame を送り直すまでの待ち |
| `--adapter` | 空 | wgpu の GPU を名前の一部で選ぶ。CEF も同じ GPU（LUID）に揃える |
| `--page-css` | 空 | ページの先頭に足す CSS（原因の切り分け用） |
| `--verify` | 切 | 全フレームをリードバックして古いフレームと再描画の同一を確かめる。`--out` で PNG と `metrics-*.jsonl` |

`--名前=値` は Chromium のスイッチとして CEF に渡る。`MM_CEF_LOG=<ファイル>` で CEF の詳細ログを書く。

## 速さに効く設定

- タイマーの粒度: 実行ファイルの先頭で `timeBeginPeriod(1)` を呼ぶ。CEF の GPU とレンダラのプロセスも同じ実行ファイルなので全部に効く。呼ばないと、ほかのアプリが粒度を上げていない PC では 1 フレームが 15 ms 前後になる。
- `windowless_frame_rate` は 1000。240 だと 4.2 ms より短い間隔で描けたフレームの受け取りが約 20 ms 遅れ、720p で p95 が 26 ms に跳ねる。
