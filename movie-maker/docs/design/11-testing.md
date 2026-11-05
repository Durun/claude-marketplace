# テスト

テストは層に分け、層ごとに「何を確かめるか、どこに置くか、道具、見本、いつ回すか」を決める。GPU の要る層は裏ごとの見本と許容差で扱う。性能のテストは[計装](17-instrumentation.md)の予算をゲートにし、超えたら落とす。

## 層の一覧

| 層 | 確かめること | 置き場所 | 道具 | 回す時 |
|---|---|---|---|---|
| 基礎の単体と性質 | 時刻の有理数の演算、コマの選択の規則、ドロップフレームの往復、キーフレームの補間（単調、端で値が一致、イージングの対称）、時間の写像、`sequence` の配置 | `mm-types`、`mm-eval` の `#[cfg(test)]` | `cargo test`、proptest [55] | 保存ごと |
| 命令の取り消し | apply → undo で元に戻る。apply → undo → redo が apply と同じ。同じ文書と命令なら同じ結果。断った命令は文書を変えない。`touches` が実際に触った所と一致する。トランザクションの取り消しがまとまりで戻る。古い版を元にした命令は `touches` が重なれば断られる | `mm-ops/tests/` | proptest の状態機械テスト（`proptest-state-machine`。参照の状態機械は「文書の写し」、遷移は命令の列。失敗した列は後ろから削って縮む）[55] | 保存ごと |
| スキーマの移行 | 版ごとの見本の文書（`fixtures/doc/v1/` …）が今の版まで上がる。移行は 1 版ずつの純関数で、1 つ飛ばしでも同じ結果。動画オブジェクトの `migrate` も同じ型 | `mm-doc/tests/`、`mm-nodes-*/tests/` | insta のスナップショット（移行後の JSON を `assert_yaml_snapshot!`。差は `cargo insta review` で見て承認する）[56] | 保存ごと |
| 保存と復旧 | アトミックな書き込み（一時ファイル → fsync → rename）。途中で落ちた書き込みからの復旧。スナップショット + `ops.jsonl` の再生が文書と一致する | `mm-store/tests/` | `cargo test`。落ち方は「書き込みの途中で `panic`」と「一時ファイルを書き終えて rename の前に止める」の 2 通りを作る | 保存ごと |
| 口ごとの契約 | 口の実装が全部、同じ約束を守る（下の「契約テスト」）。動画オブジェクトは全種類に共通の契約 | `mm-testkit` に本体、各実装の `tests/contract.rs` が 1 行で呼ぶ | `cargo test`、GPU の要るものは nextest のグループ | push 前。実装を変えた PR で |
| 描画の見本の絵 | 小さい固定の解像度（320x180）で決まった時刻のコマを描き、裏ごとの見本の絵と許容差つきで比べる。プレビューの経路と `mm frame` の経路で同じコマ | `mm-render/tests/`、見本は `fixtures/golden/<裏>/` | `image-compare`（SSIM と RMS）[57]、PNG の見本。承認は `cargo xtask golden-update`（下） | push 前（WARP）。夜間（RTX） |
| 音の数値 | ミキサーの決定性（2 回回して同じ Float32）、バスの音量、ダッキングの曲線、マイクロフェードの長さ、ラウドネス（EBU の test set で BS.1770 のメーターが Tech 3341・3342 の許容に入る）[69] | `mm-audio-dsp/tests/` | `cargo test`。比べるのはサンプル列の最大の差と RMS | 保存ごと（短い信号）、push 前（test set） |
| デコーダーのコマの正確さ | シークした先のコマの番号と PTS が合う。VFR の素材で PTS の索引どおりに引ける。OBS の mkv の音と映像の頭のずれを扱える。`d3d12va`、`cuvid`、`ffmpeg` の 3 実装で同じコマが出る（コマ番号を焼き込んだ見本で） | `mm-decode-*/tests/contract.rs` | 契約テスト。見本は `fixtures/media/` の生成した動画（下） | push 前（`ffmpeg`）。夜間（NVDEC） |
| 通しの書き出し | 見本のプロジェクトを `mm render` で書き出し、長さ、コマ数、音のサンプル数、ラウドネスの目標、チャプター、字幕が合う。区間に分けた書き出しと 1 本の書き出しが同じコマ（つなぎ目に欠けと重なりがない）。途中で止めて再開できる | `mm-export/tests/`、`apps/mm/tests/` | `assert_cmd`（`mm` を起動して終了コードと出力を見る）、`trycmd`（CLI の使い方の文書をそのままテストに）[66] | push 前（短い見本、WARP）。夜間（長い見本、RTX、NVENC） |
| Claude の入口 | HTTP と MCP の命令が CLI と同じ結果。文書を直接開く経路とアプリに頼む経路で同じ。断る命令（ロック、費用のかかる命令）が断られる | `mm-serve/tests/` | `httpmock`（同期と非同期の両方がある）[67]、`assert_cmd` | 保存ごと |
| UI の計算 | wgpu の面の計算（端のつまみ、範囲、スナップ、刃）は純関数。WebView の画面の計算（Story の選択、Review の範囲） | `mm-ui-layout` の `#[cfg(test)]`、`ui/panels/*/test/` | `cargo test`、proptest。WebView は `node --test` と `@testing-library/preact`（DOM は jsdom。happy-dom は `getByRole` が落ちる報告がある）[68] | 保存ごと |
| UI の面（WebView と wgpu） | 主な流れを本物の窓で通す。素材を足す → Story で選ぶ → Timeline に置く → 再生 → 書き出し。再生中に WebView を更新しても面が落ちない。キーの振り分け（J/K/L がどちらにフォーカスがあっても効く） | `apps/movie-maker/tests/e2e/` | WebView2 は Playwright で CDP から叩く（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` で起動し `chromium.connectOverCDP`。並列は worker ごとにポートと `WEBVIEW2_USER_DATA_FOLDER` を分ける）[61]。wgpu の面は Rust の側で入力を注入する口（`mm-ui-wgpu` の `inject_input`）を持ち、面の絵は見本の絵の層で比べる。tauri-driver + msedgedriver は WebView2 の版と合わせる手間があるので採らない [61] | 夜間。`ui/` か `apps/` を変えた PR |
| 実データ | 終わった回（ep03）を再現して、前の書き出しと比べる（コマの差、音の差、長さ） | `fixtures/ep03/`（素材は外。下の「見本の素材」） | `mm render` と `image-compare` | 夜間。段階 5 から |
| 性能 | 予算（[計装の予算の表](17-instrumentation.md#予算の表)）を超えたら落とす。測るのは、描画の 1 コマ、デコードとシーク、命令の適用、キャッシュの当たり率、書き出しの fps、起動 | `benches/`（ワークスペースの別クレート `mm-bench`） | divan（`#[divan::bench]` と `args`）[60] で基礎の層、通しは `mm perf run`（[計装](17-instrumentation.md#claude-が読む)）。比較は `mm perf compare`。criterion にも `--baseline` の比較があるが [60]、通しの数字は同じ道具（`mm perf`）で取ってそろえる | push 前（基礎）、夜間（通し）、自動の改善の回の前後 |
| 長時間の耐久 | 8 時間の再生とスクラブのくり返し、書き出しのくり返し、ホストの落としと復帰のくり返しで、メモリと VRAM が増え続けない、ハンドルが漏れない、キャッシュの大きさが上限に収まる | `mm-testkit` の `soak` の実行ファイル | `mm perf soak --hours 8`。メモリは[計装](17-instrumentation.md)の数（RSS、VRAM、ハンドル数）を 1 分おきに記録し、最初の 1 時間と最後の 1 時間の中央値を比べる。ヒープの中身を追うときは `dhat` のテストモード（`Profiler::builder().testing()`、release で、同時に 1 つ）[64] | 夜間（週 1 で 8 時間、ほかの夜は 1 時間） |
| ファズ | 文書の JSON、`ops.jsonl`、付属ファイルの JSON、OTIO の読み、字幕（SRT、VTT）、シェーダーの文字列、デコーダーに渡す壊れた動画で落ちない | `fuzz/`（`cargo fuzz` の目標） | cargo-fuzz + `arbitrary` [59]。cargo-fuzz の README は Unix 系だけと書くが、rust-fuzz の book に Windows（MSVC の ASan）の章がある [59]。CI では Linux で回し、手元の Windows は試して動けば使う | 夜間（各目標 10 分）。壊れた入力は `fuzz/corpus/` に残し、見つけたものは単体テストに写す |
| 計装の自己テスト | スパンの入れ子が崩れない、時計の補正がずれない、計装の負担が上限（[計装](17-instrumentation.md#負担の上限)）に収まる、`metrics.jsonl` の形が JSON Schema に合う | `mm-trace/tests/` | `cargo test`。負担は「計装あり」と「なし」で同じベンチを回して差を見る | push 前 |

「保存ごと」は手元で変えたクレートだけ（`cargo nextest run -p <crate>`）。「push 前」は変えたクレートと依存の逆引き。「夜間」は全部。自動の改善の回は「push 前」と同じ組に性能の比較を足す（[自動の改善](10-review-improve.md#自動の改善)）。

## 道具の決まり

- 実行は cargo-nextest。テストごとに別のプロセスなので、GPU の文脈や環境変数が漏れず、落ちたテストと時間切れを安全に止められる [58]。
- GPU の要るテストは `[test-groups]` の `gpu = { max-threads = 1 }` に入れ、同時に 1 つだけ動かす [58]。NVENC と NVDEC も同じグループ。
- 時間切れは `slow-timeout = { period = "60s", terminate-after = 2 }`。通しの書き出しと耐久は `[[profile.default.overrides]]` で別の値にする [58]。
- `--retries` は使わない。ゆらぐテストは直すか、ゆらぎの元（時刻、乱数、GPU の裏）を固定する。例外は WebView2 の起動だけ（`--retries 1`）。
- JUnit は `junit.path` で CI に出す [58]。
- 性質テストの失敗した入力は proptest の regressions ファイルに残り、次回から先に試される [55]。コミットに入れる。
- カバレッジは `cargo llvm-cov nextest`（`x86_64-pc-windows-msvc` で動く）[65]。数字を目標にはせず、口のクレートの契約テストが実装の行をどれだけ通るかを見る。変異テストは `cargo-mutants` [65] を `mm-ops` と `mm-eval` にだけ月 1 で回す。
- 音とデコードとコマの比較は、数値の許容差をテストの中に書き、見本の側には書かない。

## 契約テスト

口ごとに `mm-testkit` に置き、実装を作る関数を渡して呼ぶ（`mm_testkit::decode::contract(|| MyDecoder::new())`）。中身は口の約束の文書と同じ順に並べる。

| 口 | 契約 |
|---|---|
| `mm-node-api` | 既定値で描ける。端（t = 0、t = duration、範囲の外）で例外なし。`vector` なら 2 倍で描けて縁が崩れない。キーが安定（同じ入力で同じキー）。`deterministic` なら 2 回描いて同じ。`time_varying` が偽なら t を変えても同じ。`is_identity` が真なら入力と同じ。`region_of_definition` の外を描かない。`migrate` が古い見本を通す。`prepare` と `render` が時間切れで戻る。例外を投げたらそのレイヤーだけ透明と印 |
| `mm-render-api` | 見本の絵。プレビューと `mm frame` で同じコマ。キャッシュのキーが「種類、版、評価済みのパラメータ、入力のキー、素材のハッシュ、コマの番号、品質、色の設定、画面上の大きさ」で決まる。文書の一部を変えたら影響する時刻の範囲だけ描き直す |
| `mm-decode-api` | コマの番号と PTS。VFR。頭のずれ。シークの前後で同じコマ。プールの使い回し。壊れたファイルでエラーを返して落ちない |
| `mm-color-api` | 変換の往復（入力 → 作業 → 出力 → 入力）が許容差に入る。Rec.709 の既知の値（白、18% グレー）。OCIO と LUT の実装で同じ結果（許容差つき） |
| `mm-audio-api` | 決定性。無音の入力は無音。音量 0 dB は素通し。ラウドネスの EBU の test set |
| `mm-text-api` | 同じ文字列と字で同じ行組み。空文字。長い 1 語の折り返し。合字と絵文字で落ちない |
| `mm-host-api` | 起動、1 コマの往復、時間切れ、ホストを kill して再起動して続く。共有テクスチャの受け渡しでハンドルが漏れない |
| `mm-cache-api` | 置く → 引ける。大きさの上限で古いものから消える。消しても作り直せる。別の場所に写しても `contentHash` で当たる |
| `mm-ingest-api` | 付属ファイルの JSON Schema。版の移行 |

`mm-testkit` の見本の実装（単色のコマのデコーダー、無音のミキサー、矩形の文字）も、同じ契約を通す。

## 見本の素材

`fixtures/` に置く。生成できるものは生成し、生成の手順を `fixtures/Makefile` に書く。大きいもの（実データ）はリポジトリの外に置き、ハッシュだけを持つ。

| 置き場所 | 中身 | 作り方 |
|---|---|---|
| `fixtures/media/` | 短い動画（5 秒、1080p と 4K、H.264 と HEVC と AV1、CFR と VFR、B フレームあり・なし、bt709 の tv と pc）。各コマに番号と PTS を焼き込む。音はサイン波と無音と、頭をずらしたもの | FFmpeg の lavfi（`testsrc2`、`smptebars`、`sine`）と `drawtext` [63]〔推。コマ番号と PTS の書式は FFmpeg の `drawtext` の文書で確かめる〕。NVENC で作ったものと libx264 で作ったものの両方 |
| `fixtures/media/broken/` | 途中で切れた動画、moov のない MP4、PTS の戻る動画 | 正しい動画を `truncate` で壊す。ファズが見つけた入力を写す |
| `fixtures/doc/` | 版ごとの見本の文書（`v1/`、`v2/` …）。各版の全部の種類と、全部の命令を 1 回ずつ当てた文書 | 版を上げるたびに、その版の見本を凍結する。消さない |
| `fixtures/projects/` | 見本のプロジェクト（縦の 1 本、入れ子のコンポジション、ショートの参照、字幕とチャプター、OFX、`web.cef`）。自動の改善の回が向く先。性能の `mm perf run` の場面もここ | `mm` で作り、JSON を入れる |
| `fixtures/golden/<裏>/` | 見本の絵（320x180 の PNG）。裏は `warp`、`rtx`（D3D12）、`lavapipe` | `cargo xtask golden-update --backend warp`。差を目で見て承認する。RTX の見本は手元で更新する |
| `fixtures/audio/` | EBU の loudness test set（利用条件がある。リポジトリに入れず、取り方を書く）[69]、ミキサーの見本の信号 | ダウンロード。見本の信号は `mm-testkit` が生成 |
| `fixtures/ep03/` | ep03 の文書と、前の書き出しのハッシュ。素材は `D:/dev/DTM_Vlog/ep03/` を指す | 段階 5 で OTIO から写す |

## GPU の要るテストの扱い

- 裏を固定する。CI は D3D12 の WARP（Windows の CPU のラスタライザ。機能レベル 12_1）[62]、Linux は Vulkan の lavapipe（wgpu の CI と同じ）[62]。実機は RTX の D3D12。
- 見本の絵は裏ごとに持つ。裏が違うと浮動小数の丸めとフィルタの実装が違う。
- 許容差は 2 段。(1) 同じ裏: SSIM 0.995 以上かつ最大の差 2/255 以下〔推。段階 2 の最初の見本で決め直す〕。(2) 裏をまたぐ比較（プレビューと `mm frame` の一致を RTX と WARP で）: SSIM 0.98 以上〔推〕。
- NVDEC と NVENC の要るテストは夜間に実機で回す。CI では `ffmpeg` の実装で同じ契約を通す。
- wgpu の `gpu_test!` の型（全部の GPU で回し、裏ごとに「失敗を期待する」を管理する）[62] を `mm-testkit` の `gpu_contract!` に写す。
- 実機の夜間はこの PC で回す。録画中と書き出し中は回さない（[計装](17-instrumentation.md)の「使用中」の印を見る）。
- GPU の文脈を落とした後の復帰（device lost）もテストに入れる。WARP では `DXGI_ERROR_DEVICE_REMOVED` を起こせないので、`mm-render-api` の口で「落ちた」を注入する。

## 性能のテストと回帰のゲート

- 予算は[計装の予算の表](17-instrumentation.md#予算の表)の 1 つ。テストの側に数を書かない。
- 基礎の層（`mm-eval`、`mm-ops`、`mm-ui-layout`）は divan のベンチで、命令の適用と補間の 1 回あたりの時間を測る。
- 通しの層は `mm perf run <場面>`。見本のプロジェクトで、再生 30 秒、スクラブ 100 回、書き出し 10 秒、起動を回し、`metrics.jsonl` を出す。
- 比較は `mm perf compare <前> <後>`。Perfherder の型 [48]（閾値を超えた点を、その後の点でも確かめてから警告にする）に合わせ、閾値は項目ごと（既定 5%〔推〕。Firefox は 2% を既定にしているが、GPU の数は揺れが大きい）、1 回の数で決めず、5 回の中央値で比べる。超えたら落とす。
- 自動の改善の回は、回の前に `mm perf run` を取り、取り込みの前にもう一度取って比べる（[自動の改善](10-review-improve.md#自動の改善)）。回帰なら取り込まない。
- 夜間の数はアプリのデータの置き場所（[計装の置き場所](17-instrumentation.md#置き場所と大きさ)）に貯め、推移を `mm perf history` で出す。Blender の Open Data が共通のシーンで時系列に貯めるのと同じ型 [51]。

## テストの書き方

- 1 テスト 1 主張。名前は「何をしたら何になる」。
- 時刻と乱数と GPU の裏は、テストが決める。`Instant::now()` を直接使わず、`mm-types` の `Clock` を注入する。
- 見本の絵と数値の許容差は、テストの中に理由つきで書く。
- 落ちたテストは、直すか、落ちる理由を `#[ignore = "理由と issue"]` に書く。理由のない `ignore` は CI で落とす。
- WebView の画面のテストは、命令の発行と画面の状態の変化だけを見て、見た目は見本の絵に任せる。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
