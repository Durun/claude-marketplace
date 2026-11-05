# 進め方

| 段階 | 中身 | 終わりの条件 | 並行に進められる話題 |
|---|---|---|---|
| 0 | 難所の試作（下）と、縦の 1 本: 素材 1 本を切って文字を載せ、音を付けて書き出す。口のクレートの最初の版と `mm-testkit` の見本の実装。`mm-trace` の最初の版（スパンの決まり、`metrics.jsonl`、Chrome の形の書き出し、wgpu のタイムスタンプ）と `mm perf run` の `startup` と `export` の場面。難所の数字は全部この計装で取る | 難所の数字と可否がこの設計書に戻っている（[計装の予算の表](17-instrumentation.md#予算の表)の〔推〕が実測に置き換わっている）。書き出した MP4 が再生できる。口が固定されている | 難所は 5 つとも独立（窓、CEF、NVDEC、subsecond、OCIO）。縦の 1 本は見本の実装で先に通す。`mm-trace` は基礎なので最初に |
| 1 | 文書のスキーマ、パラメータとキーフレーム、命令（逆命令、`touches`、トランザクション）、保存と復旧、CLI | 命令の性質テストと移行テストが通る。落とした後に復旧できる | 文書と命令 ‖ デコード（d3d12va と cuvid を契約テストで） ‖ 色（OCIO の bindgen） ‖ 文字（glyphon） ‖ 音（DSP） ‖ `ui/theme` と `tasks` パネル |
| 2 | 描画器（`footage`、`solid`、`image`、`composition`、`sequence`、`text`、`shader.wgsl`、`color.*`）、OCIO、ミキサー、プレビュー、コマのキャッシュ。`mm perf` の `playback` と `scrub`、WebView の時計の補正、予算の自動の起票 | 入れ子のコンポジションを窓で通して再生できる。見本の絵のテストが通る。性能の回帰のゲートが CI で動いている | 描画器と `mm-nodes-basic` ‖ `mm-cache-fs` ‖ `mm-ui-wgpu` ‖ 取り込み（`mm-ingest`、`tools/`） ‖ `Inspector`（スキーマから作る所） |
| 3 | `Media`、`Story`、`Source`、`Timeline` と取り込み | 長い素材から話題を切り出して並べ、そのまま再生できる | 各パネルは別々（`story`、`media`、`Timeline`） ‖ 書き出し（`mm-export`） ‖ `mm-serve` |
| 4 | `Inspector`、`Graph`、`Mixer`、`Deliver`、書き出し（並行、音、ラウドネス、字幕、チャプター、ProRes）、本編とショートの参照 | 1 つのプロジェクトから本編とショートを書き出せ、プレビューと一致する | パネルごと ‖ エンコード（nvenc と ffmpeg） ‖ `mm-host-cef`（段階 6 の準備） |
| 5 | `Review`、MCP と対応のまとまり、OTIO、ep03 の再現、durun-dashboard の API | Claude が MCP で同じ編集をでき、承認・却下できる。ep03 の再現が前の書き出しと一致する | `review` パネル ‖ MCP ‖ OTIO（`mm-doc` の写し） ‖ `packs/vlog` |
| 6 | `web.cef` と vlog のパック、ep04 | ep04 を movie-maker で作れる | `mm-nodes-cef` と `mm-text-cef` ‖ パック ‖ `mm-host-ofx`（段階 8 の準備） |
| 7 | 自動の改善（worktree の囲い、Feedback、subsecond） | 要望を起票すると、自動で直って返事が付く | `mm-improve` ‖ `feedback` パネル ‖ `areas.toml` の検査 |
| 8 | `ofx`、VST3、CLAP、`lottie`、`gen.ai`、HLSL、HDR | 種類を足すだけで使える | 全部が別の実装のクレート。互いに独立 |
| 9 | モバイル版 | 出先で Story とコメントと命令の発行ができる | |

「‖」で区切ったものは別の worktree で同時に進められる。段階の順は「終わりの条件」を確かめる順で、各話題はそれより早く始めてよい（口と見本の実装があるので）。

## 段階 0 で最初に確かめる難所

| 難所 | 確かめること | ダメだったときの逃げ道 |
|---|---|---|
| Tauri の窓に wgpu の面と WebView2 | 子 HWND に `wgpu::Surface` を張り、`unstable` の子 WebView と並べる。大きさの変更、DPI、フォーカス、キーの振り分け。再生中に WebView の画面を更新しても面が落ちないか。再生位置の同期の遅れ（Rust の状態 → IPC → 画面）を測る | wry の `wgpu.rs` の例 [28] から始める。並べられなければ、wgpu の面を別の窓にする。それもダメなら対案 C |
| CEF の共有テクスチャを wgpu へ | `cef` クレートの `osr_texture_import` で、`OnAcceleratedPaint` の D3D11 の共有ハンドルを D3D12 の `OpenSharedHandle` → `texture_from_raw` → `create_texture_from_hal` で包み、コピーして描く [2][3]。CEF はハンドルをコールバックの外で使えないので、その中でコピーする [7]。1080p で何 ms か、`SendExternalBeginFrame` で時刻を決められるか | CEF の `OnPaint`（CPU の BGRA）でアップロードする。遅ければ `web.cef` はパラメータごとに 1 回描くキャッシュに限る |
| NVDEC → wgpu をコピーなしで | FFmpeg の `d3d12va` で復号した `ID3D12Resource`（NV12）を wgpu に包めるか（面の配列、フォーマット、同期） | `cuvid` + CUDA の外部メモリで 1 コピー。最後は CPU 経由（実測 4K で 700〜800 fps の NVDEC に対して、CPU のコピーがどこまで落とすかを測る） |
| Rust の関数の差し替え | subsecond で `mm-ops` の命令と WebView の裏の関数を差し替え、リロードが何秒で済むか。Tauri のアプリで動くか（対応の表は Android、iOS、Linux で、Windows は未確認 [8]） | 差し替えは WebView 側だけにし、Rust は `cargo build` の増分と再起動（文書は自動保存から復元） |
| OCIO のシェーダー | OCIO 2.5 を `bindgen` で呼び、GLSL を naga で WGSL にできるか | 3D LUT に焼いて WGSL で当てる |


---

出典の番号（[1] など）は [references.md](references.md) を見る。
