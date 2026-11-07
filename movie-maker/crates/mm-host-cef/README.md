# mm-host-cef

CEF のオフスクリーン描画を、共有テクスチャで別プロセスのアプリへ渡すホスト（`mm-host-api` の実装）。
試作の結果とつまずいた所は [docs/design/spikes/cef.md](../../docs/design/spikes/cef.md)。

## 分け方

| 部分 | 場所 | CEF の配布物 |
|---|---|---|
| アプリ側のランチャー（`CefLauncher`、`PipeHost`）、ホストの探し方（`find_host`）、URL の決まり（`policy`） | `src/launcher.rs`、`src/locate.rs`、`src/policy.rs` | 要らない。既定のビルドでコンパイルする |
| ホストの本体（`serve`、`bench`、`probe`） | `src/host/`（`--features cef` のときだけライブラリに入る） | 要る |
| sandbox つきの起動の DLL | `examples/sandboxed.rs`、`bundle.sh` | 要る |
| テスト（URL の決まり、能力の問い合わせ、契約、期待画像、古いフレーム、守り、往復の時間） | `tests/it/` | `serve` と `harden` は要る。`CEF_PATH` が無ければスキップする（`MM_REQUIRE_CEF=1` で失敗にする） |

既定のワークスペースのビルド、clippy、テストは CEF を取りに行かない。`--features cef` なしの実行ファイルは何もせず終了コード 2。

## 受け渡しの約束

- `CefLauncher::spawn` は `mm-host-cef serve` を起こし、環境変数 `MM_HOST_SPEC` に `HostSpec` の JSON を、`MM_HOST_WORK_DIR` に一時フォルダーを渡す。制御は標準入出力の JSON の 1 行ずつ。
- 一時フォルダー（CEF のキャッシュとページ）はランチャーが作り、ホストを止めた後に消す。ホストはジョブオブジェクトに入れ、kill のときは CEF の子のプロセス（GPU、レンダラ）ごと止める。
- GPU: ホストは `HostSpec::gpu`（無ければ高性能のアダプター）で wgpu を作り、その LUID を Chromium のスイッチ `use-adapter-luid=<HighPart>,<LowPart>` で CEF の GPU プロセスにも使わせる。
- `client_pid` があると、フレームごとに D3D12 の共有ヒープのテクスチャを作ってコピーし、その NT ハンドルを相手のプロセスへ `DuplicateHandle` して `SharedFrame::SharedTexture` で返す。複製するハンドルは読み取りの権限（`DXGI_SHARED_RESOURCE_READ`）だけ（D3D12 の `CreateSharedHandle` は `GENERIC_ALL` しか受けないので、絞るのは複製のとき）。形式は `DXGI_FORMAT_B8G8R8A8_UNORM`（premultiplied、sRGB の符号化）、状態は COMMON、`fence` は `None`（ホストがコピーの完了を待ってから返す）。受け取った側がコピーしてハンドルを閉じる。
- `client_pid` が無いと、CPU の画像（RGBA f32、premultiplied、線形）で返す。
- `Prepare` の `params` は `{"html": "..."}`。ホストは一時フォルダーに書いたファイルとして開く。ページは `window.mmSetTime(t, params)` を定義し、時刻 `t`（秒）と `params` の関数としてだけ画面を作る。ビューポートは `RenderFrame` の `size`。
- 読み込める資源はページそのものと `data:`、`blob:` だけ。ほかの要求（http、https、ほかのファイル）と、ページの遷移、ポップアップは `RequestHandler` と `LifeSpanHandler` で拒否する（`src/policy.rs`、`src/host/guard.rs`）。遷移を始めた時点の先読みの接続（preconnect）はその前に出るので、Chromium のネットワークを閉じたポートのプロキシ（`--proxy-server=http://127.0.0.1:9`、`--proxy-bypass-list=<-loopback>`）へ向け、行き先へは接続も名前解決も出さない。
- 1 フレームの手順: `mmSetTime` を呼んだ後に組み直し、`document.fonts` の読み込みが終わるまで待つ（初めて使う Web フォントを代わりの書体で描かない）。左上の透明な 1 画素の canvas を消し直してダメージを作り、画面が変わらないフレームでも CEF にフレームを出させる。BeginFrame からフレームが来るまで 4 秒まで待ち、来なければ誤りを返す（前のフレームを黙って返さない）。
- アプリ側（`mm-runtime`）は `find_host()` でホストを探す。`CEF_PATH` が無ければ探さない。`MM_CEF_HOST` があればそれ、無ければ今の実行ファイルと同じフォルダー（テストの実行ファイルなら 1 つ上も）の `mm-host-cef`。見つけたら `mm-host-cef probe` で能力を問い合わせ、`cef` の feature 無しのもの（何も答えない）や `libcef.dll` が見つからないものは使わない（`None` を返し、`web.cef` はモックになる）。

## sandbox

CEF の sandbox は、`bundle.sh` で組み立てたホストで有効になる。

```bash
CEF_PATH=D:/dev/cef/156 CARGO_TARGET_DIR=D:/dev/mmt crates/mm-host-cef/bundle.sh D:/dev/mm-host-cef
export MM_CEF_HOST=D:/dev/mm-host-cef/mm-host-cef.exe
```

- Windows の CEF（M138 以降）は、sandbox を Chromium と同じツールチェーンで作った起動の実行ファイル（配布物の `bootstrapc.exe`）に持たせ、アプリの処理は同じ名前の DLL の `RunConsoleMain` で受け取る。`bundle.sh` は `bootstrapc.exe` を `mm-host-cef.exe` の名前で、本体（`examples/sandboxed.rs` の DLL）を `mm-host-cef.dll` の名前で、CEF の実行時のファイルと並べ、LPAC の ACL を付ける（CEF の cmake の `SET_LPAC_ACLS` と同じ）。
- 組み立てたホストの GPU、レンダラ、ユーティリティのプロセスには `--no-sandbox` が付かない。1 フレームの時間は sandbox なしと同じ（1080p で 4.97 ms/frame、`bench --frames 3000`）。
- `cargo build` が作る実行ファイルの `mm-host-cef` を直に起こすと sandbox なしで動く（`cargo test` の `CARGO_BIN_EXE_mm-host-cef` はこちら）。`probe` の答えに ` sandbox` が付くかで見分ける。
- 使う側: `web.cef` のノード（[mm-nodes-cef](../mm-nodes-cef/README.md)）と、CSS で描く文字（[mm-text-cef](../mm-text-cef/README.md)）。

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
4. テスト。ランチャーが `CEF_PATH` を子プロセスの `PATH` の先頭に足すので、そのまま動く。
   ```bash
   cargo test --release -p mm-host-cef --features cef -- --nocapture
   ```
   期待画像は `fixtures/golden/rtx/cef-frame.png`。更新は `MM_GOLDEN_UPDATE=1` を付けて実行し、目で確かめてからコミットする。
   `MM_CEF_SANDBOX_HOST` に `bundle.sh` のホストを渡すと、sandbox つきのホストも同じ画素で描くかを確かめる。
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
