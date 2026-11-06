# 計装

後の最適化と継続的改善のために、アプリと CLI とホストの動きを数で残す。常時は軽く数え、詳しく見たいときだけ全部を残す。数は手元にだけ置き、外に送らない。予算を超えた処理とクラッシュした回数は改善要望に起票し、自動の改善の回は前後の数を比べて回帰を拒否する。

```mermaid
flowchart LR
  subgraph SRC[計測の元]
    direction TB
    RS[Rust の全クレート<br>tracing のスパンとイベント]
    GPU[wgpu のタイムスタンプ<br>パスごとの GPU の時間]
    WV[WebView の画面<br>Performance API]
    HOST[別プロセスのホスト<br>CEF / OFX / 音]
  end
  COL[mm-trace<br>購読者。時計をそろえ、負担を見張る]
  RS --> COL
  GPU --> COL
  WV -- IPC（まとめて送る） --> COL
  HOST -- パイプ --> COL
  COL --> LIGHT[常時: 数える<br>metrics.jsonl<br>区間ごとの分布と回数]
  COL --> HEAVY[詳しく: 全部残す<br>trace.json（Chrome の形）<br>Tracy へ生で送る]
  COL --> USE[使い方の記録<br>usage.jsonl<br>操作・パネル・取り消し・エラー]
  LIGHT --> GATE[予算と比べる<br>超えたら起票]
  GATE --> FB[Feedback<br>改善要望（同じものは 1 件）]
  LIGHT --> CMP[改善の回の前後の比較<br>mm perf compare]
  HEAVY --> UI[Perfetto UI / Tracy<br>人と Claude が見る]
  LIGHT & HEAVY & USE --> CLI[mm trace / mm perf<br>Claude が CLI と MCP で読む]
```

## 何を測るか

| 項目 | 測り方 | スパンかカウンタ |
|---|---|---|
| 描画の 1 フレーム（CPU） | `mm-render` の `render_frame` のスパン。中にレイヤーごとの評価、合成、リードバックのスパン | スパン |
| 描画の 1 フレーム（GPU） | wgpu のタイムスタンプのクエリ。`Features::TIMESTAMP_QUERY` で compute と render のパスの先頭と末尾に `timestamp_writes` を書き、`Queue::get_timestamp_period()`（1 tick のナノ秒）で換算する [44]。パスの中の点は `TIMESTAMP_QUERY_INSIDE_PASSES`（native のみ）[44] | スパン（GPU のトラック） |
| デコード | `Decoder::next_frame` と `seek` のスパン。シークは「頼んでから正しいフレームが出るまで」。実装（`d3d12va`、`cuvid`、`ffmpeg`）と素材の形（コーデック、解像度、GOP）をフィールドに | スパン |
| キャッシュ | 引いた回数とヒットした回数（フレーム、プレビュー動画、文字の画像、音の PCM の種類ごと）。大きさ | カウンタ |
| 操作の適用 | `mm-ops` の `apply` のスパン（検証、適用、逆操作の生成、`touches`、記録の書き込み）。トランザクションは全体で 1 つ | スパン |
| 操作から画面まで | 入力のイベントが届いた時刻から、その結果のフレームがサーフェスに出た時刻まで。wgpu のサーフェスは Rust の中で閉じる。WebView の画面は、`performance.now()` の時刻を操作に添えて送り、Rust が受けた時刻と、変更の通知が画面に戻って描かれた時刻（`requestAnimationFrame` の後）まで | スパン（流れとしてつなぐ） |
| 再生 | 落としたフレーム（音の時計に対して遅れたフレーム）の数、サーフェスに出す間隔のばらつき | カウンタ |
| 書き出し | 1 フレームあたりの時間（描画、NV12 への変換、NVENC）、区間ごとの fps、全体の fps、並行の数 | スパンとカウンタ |
| メモリ | RSS、ヒープ（アロケータの統計）、VRAM（wgpu の `Device` の報告と D3D12 の `QueryVideoMemoryInfo`〔推〕）、GPU の資源の数（テクスチャ、バッファ）、開いているハンドル。5 秒おき | カウンタ |
| 別プロセスのホスト | 1 フレームの往復の時間、時間切れの回数、クラッシュした回数、再起動にかかった時間、共有テクスチャのハンドルの数 | スパンとカウンタ |
| 取り込み | 素材ごとの段階（ハッシュ、波形、文字起こし、プレビュー動画）の時間と、実時間に対する倍率 | スパン |
| 起動 | 起動からアプリが入力を受け付けるまで、プロジェクトを開いてから最初のフレームが出るまで | スパン |
| 保存 | 操作のトランザクションのコミット 1 回、WAL のチェックポイント、バックアップ（`VACUUM INTO`）、`mm export` | スパン |
| エラー | panic、`Result` の `Err` のうち利用者に見せたもの、画面の例外、ホストのクラッシュ | イベント |
| 自動の改善の回 | 回の長さ、ターン数、検査の結果、取り込んだか | イベント |

スパンには、どの素材か（`contentHash` の先頭 8 桁）、どのコンポジションか、どの品質か（プレビュー動画か原本か）、どの実装か、をフィールドで付ける。利用者の文字列（文字の内容、ファイル名）は付けない。

## 予算の表

目標は「この PC（RTX 5090、Ryzen 9 9950X）で、1080p60 の本編を編集するとき」の値。実測の数（[レビューの実測](reviews/2026-11-05.md)の表）を手がかりにし、ほかは〔推〕。超えたら性能のテストが失敗する（[テスト](11-testing.md#性能のテストと回帰のゲート)）。

段階 0 の試作（2026-11-06）で測れた行は「段階 0 の結果」に実測を置き、測った条件を添えた。測った条件が目標の条件と違う行は、参考の数だけ書いて〔推〕を残す。段階 2 の `mm perf` の `playback` と `scrub` で残りを測り直す。

- 結果の出どころ: ウィンドウ、CEF、NVDEC は [spikes/](spikes/) の各設計書、起動と書き出しは `mm perf run startup` と `export`（PR #15）。
- 測った機材は全て RTX 5090（ドライバー 616.92）、Windows 11、wgpu 30.0.1。

| 項目 | 目標 | 段階 0 の結果 | 根拠 |
|---|---|---|---|
| 描画の 1 フレーム（プレビュー、1080p、レイヤー 10 枚、文字 3 枚） | CPU 4 ms 以下、GPU 6 ms 以下。合わせて 60 fps の 16.7 ms の中に、デコードと表示を含めて収める | **達成**。`mm perf run playback`（`fixtures/projects/perf-preview`: 1080p60、solid 10 枚〔回転と拡大つき〕と text 3 枚、フレームのキャッシュなし、CPU へのリードバックを含む）で、`render.frame` のスパンが p50 1.1 ms、GPU が p50 0.2 ms、リードバックを含む 1 フレームの全体が p50 8.7 ms、p99 12.0 ms。前は `render.frame` が p50 98.6 ms で、ほぼ全部が solid の画布の大きさの f32 の画像の生成と f16 への変換とアップロード（1080p で 1 枚約 8 ms）だった。solid をヘルパーの `fill`（GPU のクリア）で塗り、リードバックの f16 から f32 への変換を写像した範囲から直接行うようにした（RFC 0011）。RTX 5090、release | 〔推〕 → 実測 |
| 描画の 1 フレーム（書き出し、同じシーン） | 10 ms 以下（100 fps） | 参考: 同上の GPU 12 µs。CPU 側の内訳（リードバック、色変換、エンコード）は段ごとの計測がまだ無く未測定。全体は下の書き出しの行 | 〔推〕 |
| デコード（NVDEC、4K60 の同時の本数） | 8 本以上（fps に直すと 480 fps 以上。2026-11-06、ユーザー） | 達成（9 本分）。デコードだけで d3d12va 556〜588 fps、cuvid 561〜594 fps。wgpu に渡しても d3d12va のコピーなし 542〜580、cuvid の 1 コピー 560〜599。CPU 経由は 247〜265。H.264 と HEVC（x265、NVENC）、60 Mbps、GOP 120、B 2 枚、500 フレーム。[nvdec](spikes/nvdec.md#4k-の-fps) | 前の実測: 4K で 700〜800 fps |
| デコード（NVDEC、1 本、1080p） | 1,500 fps 以上〔推〕 | 達成。d3d12va 1,806〜1,857 fps（コピーなし）、cuvid 1,983〜2,004 fps（1 コピー）。H.264、20 Mbps、同じ条件。[nvdec](spikes/nvdec.md#4k-の-fps) | 実測（段階 0） |
| デコード（NVDEC、4 本の合計） | 1,400 fps 以上 | 未測定（同じ `ID3D12Device` に複数のデコーダーを載せる試験が残っている） | 前の実測 |
| シーク（プレビュー動画 540p、GOP 3） | 中央値 15 ms、99% 40 ms 以下 | 未測定 | 〔推。GOP 3 ならデコードは 3 フレーム以内〕 |
| シーク（原本 1080p60、GOP 2 秒、B あり） | 中央値 60 ms、99% 150 ms 以下 | 参考: 4K、GOP 120、B 2 枚、d3d12va、40 か所で、中央値 114 ms（H.264）と 109 ms（HEVC x265）、p95 195 と 201 ms、最大 220 と 213 ms。ラップの費用は 4 µs で、残りはキーフレームから目的のフレームまでのデコード。1080p は未測定 | 〔推。最大 120 フレームのデコード〕 |
| スクラブ（縮小したフレームがサーフェスに出るまで） | 50 ms 以下。正確なフレーム 150 ms 以下 | **達成**。`mm perf run scrub`（上と同じ見本、ランダムな 100 か所を 5 回、フレームのキャッシュなし）で、縮小したフレーム（半分の大きさ、`Quality::Preview`）が p50 2.1 ms、p99 3.8 ms。正確なフレーム（1080p、`Quality::Full`）が p50 5.9 ms、p99 8.3 ms。いずれもサーフェスに出す前の、リードバックまでの時間。素材のデコードのシークは含まない（見本に素材を使わない）。前の p99 の揺れ（縮小 92 ms、正確 437 ms、`render.frame` の最大 1.6 秒）は、solid の CPU の画像が 1 フレームで約 500 MB（1080p）の一時の確保をしていたことによる〔推。回ごとの p50 も 29〜62 ms と揺れ、GPU の時間は揺れていなかった〕。確保が無くなり、回ごとの値も揃った | 〔推〕 → 実測 |
| 操作から画面まで（wgpu のサーフェス） | 1 フレーム（16.7 ms）以下 | 参考: `SendInput` から Rust がキーを受けるまで、中央値 3.1 ms、最大 7.6 ms。サーフェスは 60 秒の連続で 59.97 fps、サーフェスを取れなかった数 0。キーからサーフェスに出るまでは未測定。[window](spikes/window.md) | 〔推〕 |
| 操作から画面まで（WebView → 操作 → 通知 → 画面） | 50 ms 以下 | 参考: 区間だけ。WebView からキーが Rust に届くまで中央値 5.0 ms、最大 10.0 ms。Rust の通知が WebView の次の rAF に届くまで（`Channel`、60 Hz）中央値 9.5 ms、99% 11.6 ms。60 秒の連続で中央値 9.6 ms、99% 16.8 ms。操作の適用を含む全体は未測定。[window](spikes/window.md) | 〔推。IPC 2 往復〕 |
| 操作の適用（単発） | 中央値 0.5 ms、99% 5 ms 以下 | 未測定（`mm-ops` は段階 1） | 〔推〕 |
| 操作の適用（トランザクション、ドラッグ 1 回分） | 20 ms 以下 | 達成。`mm perf run cpu`（レイヤー 2000 個。複製を含む `SetParam` 1 つ）が release で p50 1.8 ms。`validate` は 0.77 ms。CI の性能の回帰のゲートが前後で比べる | 実測 |
| 再生で落とすフレーム | 1 分あたり 0 フレーム（キャッシュあり）、1% 以下（キャッシュなし） | **達成**。`mm perf run playback`（5 秒を 3 回）で、60 fps の間隔（16.7 ms）を超えたフレームがキャッシュなしで 0%（`playback.dropped_pct`）、キャッシュありで 0%（`playback.cached_dropped_pct`）。前はキャッシュなしで 100%（1 フレーム 174 ms）。音の時計に対する遅れではなく、描画の時間との比べ。参考: サーフェスだけの 60 秒で 59.97 fps、フレームの間隔は 99% 18.1 ms、最大 25.7 ms | 〔推〕 → 実測 |
| フレームのキャッシュのヒット率（同じ区間の 2 回目の再生） | 95% 以上 | 1080p の 1 分の素材で 60 フレームの区間の 2 回目は 120/120（100%）。4K は 1 GiB に 16 枚しか入らず 61/120（51%）で、縮小して覚える対応が要る（RFC 0010） | 〔推〕 |
| 書き出し（1080p60、NVENC、1 プロセス） | 実時間の 2 倍以上（120 fps） | **超過**。13.2 fps（1080p）、26 fps（720p）。GPU の 1 フレームは 12 µs。条件: `mm perf run export`、`fixtures/projects/stage0`、libx264 と CPU へのリードバックで、NVENC は使っていない。PR #15。PR #23 の後は 12.55 fps（1080p、libx264）。主な時間は 1080p の描画、f32 のリードバック、CPU の色変換で、x264 のスレッドを増やしても変わらない（1080p のエンコード単体は 81 fps）。RFC 0009 の後は **達成**: 154 fps（1080p、libx264、release。同じ条件で前は 13.2 fps）。ffmpeg のフレームを YUV のまま GPU に上げ、縮小を合成で 1 回にし、GPU で NV12 にしてリードバックを 3 枚先行させた。1 フレームは描画 2.7 ms、x264 1.8 ms。段階 2 の再測定（`perf/baseline/rtx5090/export.json`、5 回の中央値）は 145 fps、最初の 1 回は 118 fps | 〔推。前の案のブラウザの中の VideoEncoder が 299 fps だった〕 |
| 書き出し（並行 4） | 実時間の 5 倍以上 | 未測定 | 〔推〕 |
| プレビュー動画の生成 | 実時間の 10 倍以上 | 未測定 | 前の実測: 約 11 倍 |
| 取り込み（波形と文字起こしが先に出るまで） | 1 時間の素材で 5 分以内〔推〕 | 未測定 | 文字起こしは faster-whisper の速さ次第 |
| メモリ（RSS、本編を開いて 1 時間） | 4 GB 以下。増え続けない（最初と最後の 1 時間の中央値の差 10% 以下） | 未測定 | 〔推〕 |
| VRAM（プレビュー） | 8 GB 以下 | 未測定 | 〔推。RTX 5090 は 32 GB だが OBS と DAW と同居する〕 |
| CEF の 1 フレーム（1080p、共有テクスチャのコピー） | 3 ms 以下 | 達成。コピー（`host_cef.import`）が p50 0.60 ms、p95 0.79 ms、p99 3.04 ms、最大 14.0 ms。ページの描画を含む 1 フレームの全体は p50 4.84 ms、p95 6.11 ms、約 190 fps。300 フレーム、固定の 1920x1080 のページ、cef 156.3.0、`windowless_frame_rate` 240、CEF と wgpu を同じ GPU に揃えた条件。[cef](spikes/cef.md#実測) | 実測（段階 0） |
| OFX の 1 フレーム（別プロセス、共有テクスチャ） | 往復 5 ms 以下 | 未測定（段階 7）。CEF の別プロセスへの受け渡しも未実装で、同じ課題が先に出る | 〔推〕 |
| ホストがクラッシュする回数 | 1 セッションあたり 0。クラッシュしたら再起動 2 秒以内 | 未測定（CEF の再起動は試していない） | 〔推〕 |
| 起動 | 入力を受け付けるまで 2 秒以内、プロジェクトを開いて最初のフレームまで 1 秒以内 | 達成。入力を受け付けるまで 464〜499 ms、最初のフレームまで 13〜19 ms（段階 2 の再測定: 545 ms と 14.8 ms）。条件: `mm perf run startup` の子プロセス（ウィンドウも WebView も作らない CLI の形）、`fixtures/projects/stage0`。ウィンドウつきのアプリは未測定。PR #15 | 実測（段階 0） |
| 保存（操作のトランザクションのコミット 1 回） | 20 ms 以下。バックアップ（`VACUUM INTO`）2 秒以下 | コミット 1 回は達成。`mm perf run cpu`（レイヤー 2000 個のプロジェクトのデータに `SetParam` を 30 回）で p50 3.9 ms。バックアップは未測定 | 実測 |
| 計装の負担（常時） | CPU の 1% 以下、1 フレームあたり 50 µs 以下 | 未測定 | 下の「負担の上限」 |

### 超えた行の見込み

| 行 | 超えた理由 | 満たす見込みの段階 |
|---|---|---|
| 書き出し（1080p60、NVENC、1 プロセス） | GPU は 12 µs で、時間は CPU 側にある。f32 RGBA のリードバックとコピー、色変換、libx264 が画素数に比例して遅くなる（1080p 13.2 fps、720p 26 fps）。NVENC ではなく libx264 で測った | 段階 4（NVENC と GPU 上の NV12 変換）。そこで測り直す。満たす見込みは段階 2（YUV の GPU の受け渡し、リードバックの先行）と段階 4（NVENC）。段ごとの計測は `Engine` から実装の一式を取り出せる関数が要る（PR #15） |

## 仕組み

### スパンの決まり

- 全クレートで `tracing` を使う [39]。`log` は使わない。スパンは `#[instrument(skip_all, fields(...))]` か `tracing::span!` で、名前は `<クレート>.<動詞>`（`render.frame`、`decode.seek`、`ops.apply`）。
- レベル: 1 フレームに 1 回以下のものは `INFO`、レイヤーごとやパスごとは `DEBUG`、それより細かいもの（シェーダーのディスパッチ 1 回）は `TRACE`。常時は `INFO` まで、詳しくは `DEBUG` か `TRACE`。
- 購読者が興味を持たないスパンは構築されない [39]。`TRACE` のスパンを多く置いても、常時の負担にはならない。release のビルドの静的な上限（`release_max_level_*`）は付けず、実行時に切り替える [39]。
- カウンタは `mm-trace` の `counter!`（名前と値）と `gauge!`。実体は `tracing` の `event!` にフィールドで載せ、`mm-trace` の `Layer` が集計する（OpenTelemetry の `MetricsLayer` が特定の名前の event からメトリクスを出すのと同じ型 [46]）。
- API クレート（`mm-*-api`）はスパンを置かない。実装のクレートと組み立て（`mm-runtime`）が置く。インターフェースの呼び出しの境目のスパンは `mm-runtime` のラッパーで付ける。
- GPU の時間は `wgpu-profiler` のスコープで取る（QuerySet とバッファを中で管理し、入れ子にできる。Chrome の形と Tracy に出せる）[45]。スコープの名前は CPU のスパンと同じ名前にし、同じトラックの横に並べる。
- 別プロセスのホストも `tracing` を持ち、スパンをパイプで `mm-trace` に送る（ホストの `pid` を付ける）。ホストがクラッシュしたら、最後に送れたスパンまでが残る。

### 出力先と切り替え

| 段 | 残すもの | 形 | 負担 |
|---|---|---|---|
| 常時（既定） | `INFO` のスパンの、名前ごとの回数と分布（中央値、95%、99%、最大。HDR のヒストグラム〔推。`hdrhistogram` クレート〕）、カウンタ、イベント。1 分ごとに 1 行 | `metrics.jsonl`（1 行 1 分。JSON Schema を `mm-trace/schema/` に） | 1% 以下 |
| 詳しく（手で入れる、予算を超えたら自動で 30 秒） | 全部のスパンとイベント、GPU の時間、WebView の計測 | Chrome の trace event の JSON（`tracing-chrome` の `Layer` [40]。`ph: "X"` の complete event、`ts` と `dur` はマイクロ秒、B/E は入れ子に [42]）。Perfetto UI で開ける [40][43] | 数%〔推〕 |
| 生で見る（開発中） | 同上をリアルタイムに | `tracing-tracy` の `Layer` で Tracy に送る（`ondemand` で、Tracy がつながるまで集めない。`only-localhost` で外に出さない）[41]。Tracy はスレッドをまたぐスパンと順序の入れ違うスパンを扱えないので、非同期のスパンは Chrome の形で見る [41] | Tracy のゾーン 1 つは数十 ns〔推。公式の現行の数は未確認〕 |
| ETW（Windows で OS の中まで見る） | 同上を ETW に | `tracing-etw` の `Layer` [54]。Windows Performance Analyzer で OS のスケジューラや DPC と並べて見る | 詳しくと同じ |

- 切り替えは `mm-trace` の 1 つの状態（段階 1 で足す。`Light`、`Detailed`、`Tracy`、`Etw`）。`Program` の右上のマーク、`mm trace start/stop`、環境変数 `MM_TRACE`、設定ファイルで変える。`Layer` は `Option` で積み、実行時に有効無効を変える [39]。
- 詳しくの自動の 30 秒は、予算を超えたスパンが出たときに始め、その間に 1 回しか始めない（1 時間あたり上限 6 回）。
- 形は Chrome の trace event に合わせる。Perfetto の Trace Processor が SQL で読め [43]、Claude が `mm trace query` で同じ SQL を打てる。Perfetto のプロトバッファ（`tracing-perfetto` [52]）は Chrome の形で足りなくなったら考える。

### 時計をそろえる

- Rust の時刻は `Instant`（Windows では `QueryPerformanceCounter`）[53]。トレースの `ts` は、アプリの起動の `Instant` からの経過を使い、起動の壁時計（`SystemTime`）を 1 回だけメタデータに書く。
- WebView の `performance.now()` は `timeOrigin` からの相対で、単調 [53]。Rust と同じ軸にそろえるため、起動時と 10 分ごとに「Rust が IPC で `ping` を送り、画面が `performance.now()` を返し、Rust が往復の時間を測る」を 5 回行い、往復の半分を引いた差を補正値にする。補正の揺れが 1 ms を超えたら、その画面の計測にフラグを付ける〔推。`timeOrigin` と QPC の原点の関係は確かめられなかったので、原点を頼らず往復で測る〕。
- 画面の計測は `performance.mark` と `performance.measure` で取り、1 秒に 1 回まとめて IPC で送る（1 件ずつ送らない）。送るのは名前と時刻と長さだけ。
- 別プロセスのホストは同じ `QueryPerformanceCounter` の軸なので、`pid` を付けてそのまま送る。
- GPU の時刻は `get_timestamp_period` で ns に換算し、そのパスを提出した CPU の時刻に先頭をそろえる（GPU の時計と CPU の時計の原点は合わないので、提出の時刻を 0 とした相対で置く）〔推〕。

### 負担の上限

- 常時の段は、CPU の 1% 以下、1 フレームあたり 50 µs 以下。`mm-trace` が自分の時間（`Layer` の中で過ごした時間）を測って `metrics.jsonl` に書き、超えたら `INFO` のスパンの集計を間引く（10 回に 1 回だけ集計）。
- 詳しくの段は上限を置かず、30 秒で止める。
- 計装の自己テスト（[テスト](11-testing.md)）で、計装ありとなしで同じベンチを回して差が上限に入ることを確かめる。
- 1 フレームの中で確保するものは置かない（スパンのフィールドは `Copy` の値だけ。文字列は事前に interning）。

### 使い方の記録

- 使いやすさの改善の材料として、どの操作が使われたか、どのパネルがどれだけ開かれていたか、取り消しとやり直しの回数（どの操作の直後に取り消されたか）、ショートカットとメニューのどちらで呼んだか、エラーと「拒否された操作」の回数を、`usage.jsonl` に残す。
- 残すのは操作の名前とパネルの名前と回数と時刻だけ。引数の内容、ファイル名、文字の内容、コメントの文は残さない。
- 外に送らない。durun-dashboard にも送らず、Claude が `mm perf usage` で読むだけ。
- 「取り消しの多い操作」と「拒否された操作」の上位は、月 1 で Feedback に起票する（下）。

## 置き場所と大きさ

| もの | 置き場所 | 大きさの上限 | 消してよいか |
|---|---|---|---|
| `metrics.jsonl`、`usage.jsonl` | アプリのデータの置き場所（`%LOCALAPPDATA%/movie-maker/telemetry/<日付>/`）。プロジェクトをまたいで比べるので `cache/` には置かない | 1 日 10 MB〔推〕、90 日で古い日から消す | 消してよい。消すと推移が見られなくなるだけ |
| 詳しくの trace | 同じ所の `traces/<時刻>.json` | 1 本 200 MB まで、合計 2 GB で古いものから消す | 消してよい |
| 性能のテストの履歴（`mm perf run` の結果） | 同じ所の `perf/<時刻>.json`。git のコミットと GPU のドライバのバージョンを添える | 1 年 | 消してよい。消すと比較の相手がなくなる |
| 自動の改善の回の前後の数 | 回の worktree の `.mm/perf/`。取り込んだら上の履歴へコピーする | 回ごと | 回と一緒に消す |
| プロジェクトに固有の数（キャッシュの大きさ、素材ごとのデコードの速さ） | プロジェクトの `cache/telemetry/` | `cache/` の上限の中 | `cache/` と一緒に消す |

- プロジェクトのデータ（`project.mmdb`、`versions/` の JSON）には計測の数を書かない。
- git に入れない。

## 継続的改善とのつなぎ

- 予算を超えたスパンとクラッシュした回数は、durun-dashboard の `errors.ts` の型 [70] で起票する。同じものは 1 件にまとめ（キーは「場所 + スパンの名前 + メッセージから数字と ID を伏せたもの」）、未対応は「遅い処理」と「エラー」でそれぞれ 3 件まで、同じものの起票は 2 回まで、1 時間あたりの上限。要望の文面には、回数、最長、最長だったときの負荷（CPU の load と GPU の使用率）、詳しくの trace のパス、`mm trace query` で見るための SQL の例を添える。
- `ErrorRecord` の形を `mm-improve` に取り入れる（`source` は `app | cli | webview | host-cef | host-ofx | host-audio`、`slowestMs`、`slowestLoad`、`count`、`firstAt`、`lastAt`、`filed`、`item`）。
- 自動の改善の回は、回の前に `mm perf run all` を取り、取り込みの前に同じシナリオを取って `mm perf compare` で比べる。どれかが閾値を超えて遅くなっていたら取り込まず、回の返事に「遅くなった項目と数」を書く（[自動の改善](10-review-improve.md#自動の改善)）。
- 回の直後に起きたエラーと遅い処理は、その回の疑いとして結びつける（durun-dashboard と同じ）。
- 人が見るときは Perfetto UI か Tracy。Claude は CLI と MCP で読む（下）。

## Claude が読む

| 操作 | 内容 |
|---|---|
| `mm perf run <シナリオ> [--repeat 5]` | フィクスチャのプロジェクトでシナリオ（`playback`、`scrub`、`export`、`startup`、`all`）を回し、`metrics.jsonl` と結果の JSON を書く |
| `mm perf compare <前> <後> [--threshold 5%]` | 2 つの結果を項目ごとに比べ、閾値を超えたものを出す。終了コードで失敗させる |
| `mm perf history [--item render.frame] [--days 30]` | 履歴の推移（中央値と 99%）。コミットとドライバのバージョンつき |
| `mm perf budget` | 予算の表と今の数を並べ、超えている項目を出す |
| `mm perf usage [--days 30]` | 使い方の記録の集計（操作、パネル、取り消し、拒否された操作） |
| `mm perf soak --hours N` | 耐久（[テスト](11-testing.md)） |
| `mm trace start/stop [--level detailed]` | 動いているアプリの詳しくの段を切り替える（HTTP で頼む） |
| `mm trace query <trace.json> "<SQL>"` | Perfetto の Trace Processor（Python の `perfetto` パッケージ）で SQL を打つ [43]。`slice`、`thread`、`counter` の表 |
| `mm trace summary <trace.json>` | 名前ごとの回数と分布、いちばん長いスパン上位 20、落としたフレーム、を文章で |
| `mm trace open <trace.json>` | Perfetto UI をブラウザで開く |

MCP も同じ名前のツールを持つ（`perf_budget`、`trace_summary`、`trace_query`）。`mm-serve` の HTTP には `/telemetry/metrics`（直近 1 分）と `/telemetry/trace/start` を置く。

## 先行事例から採るもの

| 事例 | 採るもの |
|---|---|
| `tracing` の `Layer` [39] | スパンを全クレートの決まりにし、出力先を `Layer` で差し替える。興味のないスパンは作られない |
| Chrome の trace event と Perfetto [40][42][43] | 詳しくの形。SQL で読める。`tracing-chrome` で書く |
| Tracy [41] と Zed [49] | 生で見る段。Zed は `#[instrument(skip_all)]` と環境変数で Tracy に送る。同じ型 |
| wgpu-profiler [45] と wgpu のタイムスタンプ [44] | GPU の時間を CPU のスパンと同じ名前で並べる |
| Unreal Insights [47] | チャンネル（`cpu,gpu,frame,…`）で軽い常時と詳しい段を切り替える。`.utrace` の自己記述 |
| Firefox の Perfherder [48] | 閾値を超えた点を後の点でも確かめてから警告にする。項目ごとの閾値 |
| Blender Open Data [51] | 共通のシーンで測り、時系列に貯める |
| Rerun [50] | `profile_function!` の型のマクロ。数フレーム分をメモリに取る `ProfileCapture`（詳しくの自動の 30 秒に相当） |
| OpenTelemetry [46] | event からメトリクスを出す `Layer` の型、単位と名前の決まり（秒、階層の名前）。外に送らないので exporter は使わない |
| durun-dashboard の `errors.ts` [70] | 同じものを 1 件にまとめ、未対応の上限、起票の上限 |

新しく設計する所: WebView と Rust の時計の往復での補正、予算を超えたら 30 秒だけ詳しくの段に入る自動の切り替え、自動の改善の回の前後の比較を取り込みのゲートにすること、使い方の記録を手元だけに残すこと。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
