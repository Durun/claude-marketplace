# 進め方

| 段階 | 中身 | 終わりの条件 | 並行に進められる領域 |
|---|---|---|---|
| 0 | 難所の試作（下）と、縦の 1 本: 素材 1 本を切って文字を載せ、音を付けて書き出す。API クレートの最初のバージョンと `mm-testkit` のモックの実装。`mm-trace` の最初のバージョン（スパンの決まり、`metrics.jsonl`、Chrome の形の書き出し、wgpu のタイムスタンプ）と `mm perf run` の `startup` と `export` のシナリオ。難所の数字は全部この計装で取る | 難所の数字と可否がこの設計書に戻っている（[計装の予算の表](17-instrumentation.md#予算の表)の〔推〕が実測に置き換わっている）。書き出した MP4 が再生できる。インターフェースが固定されている | 難所は 5 つとも独立（ウィンドウ、CEF、NVDEC、subsecond、OCIO）。縦の 1 本はモックの実装で先に通す。`mm-trace` は基礎なので最初に |
| 1 | プロジェクトのデータのスキーマ、パラメータとキーフレーム、操作（逆操作、`touches`、トランザクション）、保存と復旧、CLI | 操作の性質テストと移行テストが通る。クラッシュさせた後に復旧できる | プロジェクトのデータと操作 ‖ デコード（d3d12va と cuvid を契約テストで） ‖ 色（OCIO の bindgen） ‖ 文字（glyphon） ‖ 音（DSP） ‖ `ui/theme` と `tasks` パネル |
| 2 | レンダラー（`footage`、`solid`、`image`、`composition`、`sequence`、`text`、`shader.wgsl`、`color.*`）、OCIO、ミキサー、プレビュー、フレームのキャッシュ。`mm perf` の `playback` と `scrub`、WebView の時計の補正、予算の自動の起票 | 入れ子のコンポジションをウィンドウで通して再生できる。期待画像のテストが通る。性能の回帰のゲートが CI で動いている | レンダラーと `mm-nodes-basic` ‖ `mm-cache-fs` ‖ `mm-ui-wgpu` ‖ 取り込み（`mm-ingest`、`tools/`） ‖ `Inspector`（スキーマから作る所） |
| 3 | `Media`、`Story`、`Source`、`Timeline` と取り込み | 長い素材から話題を切り出して並べ、そのまま再生できる | 各パネルは別々（`story`、`media`、`Timeline`） ‖ 書き出し（`mm-export`） ‖ `mm-serve` |
| 4 | `Inspector`、`Graph`、`Mixer`、`Deliver`、書き出し（並行、音、ラウドネス、字幕、チャプター、ProRes）、本編とショートの参照 | 1 つのプロジェクトから本編とショートを書き出せ、プレビューと一致する | パネルごと ‖ エンコード（nvenc と ffmpeg） ‖ `mm-host-cef`（段階 6 の準備） |
| 5 | `Review`、MCP と対応のトランザクション、OTIO、ep03 の再現、durun-dashboard の API | Claude が MCP で同じ編集をでき、承認・却下できる。ep03 の再現が前の書き出しと一致する（2026-11-07 にテストの閾値を満たした。[結果と足りない機能](11-testing.md#ep03-の再現の結果)） | `review` パネル ‖ MCP ‖ OTIO（`mm-doc` への変換） ‖ `packs/vlog` |
| 6 | `web.cef` と vlog のパック、ep04 | ep04 を movie-maker で作れる | `mm-nodes-cef` と `mm-text-cef` ‖ パック ‖ `mm-host-ofx`（段階 8 の準備） |
| 7 | 自動の改善（worktree の囲い、Feedback、subsecond） | 要望を起票すると、自動で直って返事が付く | `mm-improve` ‖ `feedback` パネル ‖ `areas.toml` の検査 |
| 8 | `ofx`、VST3、CLAP、`lottie`、`gen.ai`、HLSL、HDR | 種類を足すだけで使える | 全部が別の実装のクレート。互いに独立 |
| 9 | モバイル版 | 出先で Story とコメントと操作の発行ができる | |
| 10 | 機能の網羅性テスト: ep02（`D:/dev/DTM_Vlog/ep02/`）を movie-maker で再現する。再現の途中で見つけた機能の不足は、見つけたそばから足す | ep02 を movie-maker で作り直せ、前の書き出しと見比べて同じ内容になっている。足りなかった機能の一覧と、それぞれを足した PR が設計書に戻っている | 不足ごとに別の worktree（見つけた順に起こす） |

「‖」で区切ったものは別の worktree で同時に進められる。段階の順は「終わりの条件」を確かめる順で、各領域はそれより早く始めてよい（インターフェースとモックの実装があるので）。

## 段階ごとの手順

各段階は、次の順で閉じる（2026-11-06、ユーザー）。

1. 実装: 領域ごとに worktree と PR を分け、並行に進める。
2. レビュー: 実装した担当とは別のレビュアーが、その段階の差分を読む。重い順に指摘を出し、終わりの条件を本当に満たしたかも判定する。
3. 指摘の直し: 指摘を直す PR を作ってマージする。後の段階に回す指摘は、その段階の「後の段階の注意」に書く。
4. 用語のクリーンアップ: その段階の差分（コード、コメント、設計書）に、次の 4 つの手順を適用する。
   1. `口` や `版` のように、それ単体では意味が分かりにくい用語を、全部洗い出す。
   2. 正式な用語へのマッピング表を作る（例: 版 → バージョン）。表は [用語集](glossary.md) に足す。
   3. 正式な用語へ置き換える PR を作る。PR の本文にマッピング表を載せる。
   4. PR をマージする。
5. 次の段階に進む。

初回の用語のクリーンアップは、段階 1 を閉じるときに、段階 0 と段階 1 のコード全体へまとめて適用する。それより後は、その段階の差分だけに適用すれば、全体が正式な用語にそろう。新しく書くコードと設計書は、最初から用語集の正式な用語を使う。

## 段階 0 で最初に確かめる難所

結果は 2026-11-06 に試作（[spikes/](spikes/)）で取った。5 つとも可。フォールバックに切り替わったのは、NVDEC の一部の素材と OCIO の一部の変換だけ。

| 難所 | 確かめること | ダメだったときのフォールバック | 結果 |
|---|---|---|---|
| Tauri のウィンドウに wgpu のサーフェスと WebView2 | 子 HWND に `wgpu::Surface` を張り、`unstable` の子 WebView と並べる。大きさの変更、DPI、フォーカス、キーの振り分け。再生中に WebView の画面を更新してもサーフェスがクラッシュしないか。再生位置の同期の遅れ（Rust の状態 → IPC → 画面）を測る | wry の `wgpu.rs` の例 [28] から始める。並べられなければ、wgpu のサーフェスを別のウィンドウにする。それもダメなら対案 C | 可。フォールバックには切り替わっていない。Tauri 2.12.5 の `Window::add_child` と自前の子 HWND のサーフェス。wry の例は使っていない。サーフェスは 60 秒の連続で 59.97 fps、サーフェスを取れなかった数 0。通知が WebView の rAF に届くまで中央値 9.5 ms、99% 11.6 ms。大きさを変え続けると WebView の同期が最大 0.25 秒遅れ、サーフェスは 1 フレーム落ちる程度。未確認は 150% の DPI、倍率の違うモニター間、複数の子 WebView。[window](spikes/window.md) |
| CEF の共有テクスチャを wgpu へ | `cef` クレートの `osr_texture_import` で、`OnAcceleratedPaint` の D3D11 の共有ハンドルを D3D12 の `OpenSharedHandle` → `texture_from_raw` → `create_texture_from_hal` でラップし、コピーして描く [2][3]。CEF はハンドルをコールバックの外で使えないので、その中でコピーする [7]。1080p で何 ms か、`SendExternalBeginFrame` で時刻を決められるか | CEF の `OnPaint`（CPU の BGRA）でアップロードする。遅ければ `web.cef` はパラメータごとに 1 回描くキャッシュに限る | 可。共有テクスチャを採る。コピーは 1080p で p50 0.60 ms、p95 0.79 ms（予算 3 ms）。1 フレームの全体は p50 4.84 ms、約 190 fps で、描く時刻を決められ、同じ入力の画素は一致した。`OnPaint` のフォールバックも動き、1080p で 9.18 ms、4K で 22.6 ms（共有テクスチャは 4.74 ms）。保険として残し、切り替えてはいない。未確かめは別プロセスのアプリへの受け渡し。[cef](spikes/cef.md) |
| NVDEC → wgpu をコピーなしで | FFmpeg の `d3d12va` でデコードした `ID3D12Resource`（NV12）を wgpu にラップできるか（プレーンの配列、フォーマット、同期） | `cuvid` + CUDA の外部メモリで 1 コピー。最後は CPU 経由（実測 4K で 700〜800 fps の NVDEC に対して、CPU のコピーがどこまで落とすかを測る） | 可。`d3d12va` をコピーなしでラップできた（ラップの費用 1 µs、画素は CPU 経由と一致）。既定は `d3d12va`。NVENC で作った 4K の HEVC は `d3d12va` でデコードできず、その素材だけ `cuvid`（1 コピー、18 µs）に切り替わる。最後は CPU 経由。4K は `d3d12va` 540〜590 fps、`cuvid` 560〜600 fps、CPU 経由 250〜265 fps で、700 fps には届かない。[nvdec](spikes/nvdec.md) |
| Rust の関数の差し替え | subsecond で `mm-ops` の操作と WebView から呼ぶ Rust の関数を差し替え、リロードが何秒で済むか。Tauri のアプリで動くか（対応の表は Android、iOS、Linux で、Windows は未確認 [8]） | 差し替えは WebView 側だけにし、Rust は `cargo build` の増分と再起動（プロジェクトのデータは自動保存から復元） | 可。Windows 11 で単体の実行ファイルでも Tauri でも動き、メモリの中のプロジェクトのデータは残った。保存から反映まで単体の実行ファイルで中央値 0.7 秒、Tauri で 1.5 秒（フォールバックの増分ビルドと再起動は 1.1 秒と 2.8 秒）。速さより状態とウィンドウを失わないことが利点。Tauri は `tauri-build` と `dx` の Windows のリソースが二重になり、手当てが要る。フォールバックには切り替わっていない。[subsecond](spikes/subsecond.md) |
| OCIO のシェーダー | OCIO 2.5 を `bindgen` で呼び、GLSL を naga で WGSL にできるか | 3D LUT にベイクして WGSL で適用する | 可。ただし一部はフォールバック。C の薄いブリッジ（約 150 行）に `bindgen` をかけて呼べた。式だけの変換は naga で WGSL になり、GPU で OCIO の CPU と最大差 2.4e-5。テクスチャを持つ ACES 2.0 の表示変換は naga が読めず、3D LUT にベイクする（65³ で最大差 1.3e-3）。OCIO は 2.6.0 で確かめた。[ocio](spikes/ocio.md) |

## 段階 0 の終わりの条件の判定

2026-11-06 時点。

| 条件 | 判定 | 残り |
|---|---|---|
| 難所の数字と可否がこの設計書に戻っている（[計装の予算の表](17-instrumentation.md#予算の表)の〔推〕が実測に置き換わっている） | 難所に当たる行は満たした。5 つの可否と数字は上の表と [spikes/](spikes/) に戻り、予算の表は 26 行のうち 5 行を実測に置き換えた（CEF、起動、デコード 1080p が達成、デコード 4K と書き出しが超過）。6 行は条件が違う参考値だけを足し、15 行は〔推〕のまま | 超過の 2 行は段階 1（デコードの実素材）と段階 4（NVENC）で測り直す。残りの行は、測る対象（操作、キャッシュ、取り込み、保存など）が段階 1 以降で出来るので、そこで置き換える。ウィンドウの 150% の DPI などは人が確かめる |
| 書き出した MP4 が再生できる | 機械的には満たした。`mm render` の出力を、統合テスト（`apps/mm/tests/it/render.rs`）が ffprobe と `mm-decode-ffmpeg` で読み直す（PR #15） | 人が再生ソフトで見た記録が無い。書き出しは libx264 と CPU のリードバックで、NVENC は未接続 |
| インターフェースが固定されている | 未了。API クレートの最初のバージョンと `mm-testkit` のモックの実装はあるが、試作でインターフェースの変更が必要と分かった | インターフェースの変更を RFC で決めて直す（別の担当が進めている）。決まった方針は `GpuHelper` トレイトで wgpu を囲うこと。ほかに、NVDEC のフレームの受け渡しと GPU のコンテキストと寿命と同期（`mm-decode-api`）、CEF のホストへの GPU の指定と共有ハンドルの受け渡し（`mm-host-api`）、`TransformProgram` のテクスチャの置き場所（`mm-color-api`）、`Engine` から実装の一式を取り出す関数 |

縦の 1 本（素材を切って文字を載せ、音を付けて書き出す）、`mm-trace`、`mm perf run` の `startup` と `export` は動いている（PR #15）。

## 試作で分かった、後の段階の注意

| 段階 | 注意 |
|---|---|
| 1 デコード | `d3d12va` を既定にする。`open` の後に最初の数フレームをデコードして `Err` なら `cuvid`、最後は CPU 経由に切り替える（NVENC で作った 4K の HEVC は 1 フレーム目でデコードに失敗する）。この素材（NVENC の 4K、CTB 32）を契約テストのフィクスチャに入れ、OBS の NVENC の録画が同じ形かを確かめる。FFmpeg の既定の D3D12 アダプターは内蔵 GPU なので、wgpu の `ID3D12Device` を渡して同じ GPU でデコードする。実素材で 4K の fps を測り直す。[nvdec](spikes/nvdec.md#詰まった所) |
| 1 デコード | d3d12va と cuvid に「wgpu が読み終えた」フェンスが無く（`mm-decode-d3d12va`、`mm-decode-cuvid`）、GPU が遅れると別のフレームが混ざる。段階 1 で直す。 |
| 1 デコード | d3d12va を `Decoder` のインターフェースだけで使うと壊れる（`take_frame` を呼ばないと `held` が増えてプールが尽きる、長さを整数秒で切り捨てる、シークにコンテナの絶対時刻を使う、渡された `gpu` を無視する）。`mm-decode-cuvid` の `convert.rs` は 50 行を超える。段階 1 で直す。 |
| 1 デコード | 10 bit の素材も 8 bit の RGBA を経由し、バンディングと HDR の喪失が出る（`mm-decode-ffmpeg` の `convert.rs`）。段階 1 で直す。 |
| 1 色 | `bt709` は Rec.1886 の表示の式を指す（2026-11-06 の決定）。BT.709 の OETF は別の名前で持ち、`mm-color-lut` のフィクスチャもそれに合わせる。契約の「白は白」はログと PQ の空間に合わないので、空間ごとに期待値を持たせる。OCIO の環境（vcpkg、LLVM、`OCIO_ROOT`、DLL の同梱）は `mm-color-ocio` の README。[ocio](spikes/ocio.md#詰まった所) |
| 2 レンダラー | NVDEC のフレームは `submit` が終わるまでの借用にし、`Queue::submit` の前に使うフレームすべての待ちを登録する。フレームのキャッシュは NV12 のままにせず RGBA で置く。ACES 2.0 の表示変換を WGSL で出すなら、結合サンプラーの書き換えと `TransformProgram` のテクスチャの置き場所が要る。 |
| 2 レンダラー | 済（RFC 0009）: ベクターの倍率は `region_of_definition` の領域だけに掛かり、素材の縮小は合成のシェーダーで 1 回だけ面積平均、ffmpeg のフレームは YUV のまま GPU に上げ、書き出しは GPU で NV12 にしてリードバックを 3 枚先行させる。残り: GPU の無い CPU の経路（`FfmpegCpuDecoderFactory`、契約テスト）は `DecodedFrame` の clone と双線形の縮小のまま。デコードと描画はまだ同じスレッド。 |
| 2 レンダラー | 済（RFC 0010）: PTS の索引を素材の指紋で `cache/` に置き、2 回目からパケットを読まない。`invalidate` は操作の `touches` に頼ったキャッシュと、その素材だけを閉じる。元の問題: 開くたびに全パケットを読み（1 時間の 4K 50 Mbps で約 22 GB）、`invalidate` が全部の素材を閉じる（`mm-decode-ffmpeg` の `timeline.rs`、`mm-render` の `renderer.rs`）。コンテナの索引を持ち、該当する素材だけ閉じる。 |
| 2 レンダラー | 済: `text` はシェーピングの結果と描いた画布をパラメータをキーに覚え、2 フレーム目からはシェーピングも描画も画布の確保もしない（`mm-nodes-basic` の `text.rs`）。残り: 画布は CPU の画像のまま毎フレーム GPU に上げる。GPU のテクスチャとして覚えるのはフレームのキャッシュ（`layers.rs` の呼び出し箇所）の担当。 |
| 2 ウィンドウ | Tauri のコマンドは `async` にする（同期だと 60 Hz の通知がメッセージループをブロックする）。サーフェスの大きさと WebView の矩形は物理ピクセルから 1 か所で決める。キーはサーフェスの `WM_KEYDOWN` と WebView の `keydown` を 1 つのエントリーポイントに入れる。サーフェスを取れなかったとき（`Outdated`、`Lost`）の経路は未試験。150% の DPI と倍率の違うモニター間を人が確かめる。連続で大きさを変えると WebView の同期が最大 0.25 秒遅れる。[window](spikes/window.md#実装のとき気をつける所) |
| 2〜3 アプリ | 済（PR #39）: 描画スレッドは毎周で HWND の生死を確かめ、閉じる前に止めて待つので、破棄された HWND に present しない（`apps/movie-maker` の `render.rs`、`app.rs`）。元の問題: 終了時に破棄された HWND に present し続けた。 |
| 2〜3 アプリ | 済（PR #39）: ペースは Fifo の垂直同期に任せ、自前の 60 Hz の sleep と spin を持たない。映像は音の時計に合わせる。50 行を超えた関数は分けた。元の問題: 垂直同期に加えて自前で 60 Hz に回し、120/144 Hz でうなって CPU も使った。 |
| 3 取り込み | 済: `mm ingest` が `.cube` を素材として取り込み、`contentHash`（BLAKE3）を入れる（`mm-ingest` の `lut.rs`）。入れ子の画像がキャッシュに当たることは `apps/mm/tests/lut_cache.rs` が確かめる。残り: 既存のプロジェクトの移行済みの LUT の素材（`contentHash` が空、ID は `lut-…`）に取り込んだ値を入れる操作が無く、取り込むと別の素材（`a_…`）が足される。元の問題: v4 → v5 の移行（`migrate_lut`）が作る LUT の素材は `contentHash` が空で、フレームのキャッシュはそれに頼る画像をキャッシュしない（段階 2 のレビュー、[RFC 0013](../rfcs/0013-render-order-and-cache-keys.md)）。 |
| 3 `mm-serve` | CLI の `mm op` は操作ごとの別のプロセスで `Engine` を持たないので、アプリが開いているプロジェクトに CLI の操作を届けて `invalidate_touched` を呼ぶ経路を、ストアの変更をアプリに伝える経路と一緒に作る（段階 2 のレビュー、[RFC 0012](../rfcs/0012-stage2-leftovers.md)）。 |
| 3 Timeline | Timeline の端のつまみのドラッグでレイヤーのイン点とアウト点を動かす操作が `mm-ops` に無い（`ripple` と `roll` はシーケンスのカットの操作）。`mm-ops` に操作を足す（段階 2 のレビュー、[RFC 0012](../rfcs/0012-stage2-leftovers.md)）。 |
| 3 書き出し | 済: レンダーターゲットごと（`mm render --all`、`--target` を繰り返す）、sequence のまま（`--sequence`）、区間（`--range`）、進み具合、Ctrl+C の取り消し（一時ファイルは `.partial` に書いて完成してから置き換え、失敗と取り消しでは消す）、ffprobe の検証（フレーム数、映像と音の長さ、大きさ）。並行の描画は段階 4。 |
| 4 書き出し | 済: ラウドネスの正規化（2 パス、トゥルーピークのリミッター）、字幕（SRT・WebVTT・MP4 のトラック）、チャプター（MP4 と YouTube の説明欄の文字列）。`mm render` と `mm subtitles`。通しは `apps/mm/tests/captions_loudness.rs`。残り: 字幕の焼き込み、TTML、字幕の修正を入れる画面。 |
| 4 アプリ | WebView の時刻の表示が、画面の上ではサーフェスの画像より約 2 フレーム遅れて出る（WebView2 の合成の遅れと推定）。スクリーンショットの画像は `--play-test` の報告の `screenshot_at`（再生位置と Program に渡したフレームの時刻）と比べる。表示をサーフェスに合わせる直しは未着手。 |
| 4 書き出し | 1080p で 13.2 fps（予算 120 fps）。CPU のリードバックと libx264 を NVENC と GPU 上の NV12 変換に替えて測り直す。段ごとの計測のため、`Engine` から実装の一式を取り出せる関数が要る。 |
| 4 `mm-host-cef`（段階 6 の準備） | CEF と wgpu を同じ GPU に揃える（既定は内蔵の AMD を選び、`OpenSharedHandle` が `E_INVALIDARG` で失敗する）。`windowless_frame_rate` は 240 以上にする（既定の 60 だと BeginFrame からフレームが来るまで p50 35 ms）。BeginFrame は捨てられることがあるので、フレームが来るまで送り直し、`seq` のシーケンス番号で古いフレームを検出する。別プロセスのアプリへは、`DuplicateHandle` で複製するか、ホストが自前の D3D12 共有テクスチャへコピーして渡す。1280x720 で p95 が 26 ms に跳ねた原因を調べる。[cef](spikes/cef.md#つまずいた所) 済（段階 4）: `timeBeginPeriod(1)`、`windowless_frame_rate` 1000（720p の p95 26 ms の原因）、`use-adapter-luid` で GPU を揃え、自前の共有テクスチャへコピーして `DuplicateHandle` で渡す。[cef](spikes/cef.md#段階-4-の追加mm-host-cef) |
| 6 `web.cef` | 実際のページ（動画、`<canvas>`、Web フォント）と透過の扱いで 1 フレームの時間を取り直す。CEF をクラッシュさせたときの再起動と複数ブラウザの同時描画を試す。 |
| 6 ホスト | レンダラーがデバイスを作り直したとき、別プロセスのホストも新しいデバイスに持ち替える。段階 2 ではホストが再生の経路に入っていない（段階 2 のレビュー、[RFC 0012](../rfcs/0012-stage2-leftovers.md)）。 |
| 7 subsecond | 差し替えは `subsecond::call` の中だけで効く。ポインタ 1 本分だけを捕まえるクロージャは差し替えを素通りして古いまま動く（エラーも出ない）。試作では捕まえる値に通し番号を足して 2 本分にしたら効いた。Tauri では `tauri-build` と `dx` の `resource.lib` が二重になる（試作は `tauri-build` をコピーして `[patch.crates-io]` で差し替え、`MM_SKIP_WINRES` で省いた）。差し替えが効くのは `dx` で起動したワークスペースのメンバーだけ。実物の大きさで測り直す。[subsecond](spikes/subsecond.md#詰まった所) |
| 8 HDR | 線形の光で 1 を超える値が 3D LUT で切られる（`color.lut` と、3D LUT にベイクした色の変換）。`TransformProgram::Lut3d` に shaper か定義域を持たせる（段階 2 のレビュー）。 |

---

出典の番号（[1] など）は [references.md](references.md) を見る。
