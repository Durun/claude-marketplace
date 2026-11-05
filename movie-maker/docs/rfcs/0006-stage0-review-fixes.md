# RFC 0006: 段階 0 のレビュー（基礎・口・組み立て）の修正

状態: 採用

## 変える理由

段階 0 のレビューで、基礎の型と口の約束に、実際に panic や取りこぼしを起こす穴が見つかった。
口の形を変えるもの（下の「形を変える」）はこの RFC で決める。足すだけ・doc だけのものは「足すだけ・doc だけ」に一覧する。

## 形を変える

### ホストのプロトコル（`mm-host-api`）

`HostResponse::Frame(SharedFrame)` は外も中も JSON のタグが `type` で、`{"type":"frame","type":"cpu",...}` になり読めなかった。

- `SharedFrame` のタグを `kind` にする（`{"type":"frame","kind":"cpu",...}`）。Rust の型の形は変わらない。
- `PROTOCOL_VERSION` を 1 から 2 に上げる。
- 契約（`mm_testkit::host::contract`）に、応答の JSON の往復を足す。
- 影響する実装: 見本のホストだけ（`mm-host-cef` は段階 0 でこの型を使っていない）。

### VFR の区間（`mm-decode-api`）

VFR の `decode_range` が最後の 1 コマを落とし（PTS が `[0, 1/2, 3/4]` で区間 `[0, 1)` のとき 3 コマのところ 2 コマ）、最初のコマより前の `t` で CFR は -1、VFR は 0 を返していた。

- `MediaInfo::frame_range(range)` を足す。CFR は `range.frame_indices(fps)`、VFR は「`end` より前の PTS の数 − `start` より前の PTS の数」。
- `Decoder::decode_range` の既定の実装は `frame_range` の番号を返す。区間の頭が PTS の途中でシーク先が頭より前のコマになるときは、そのコマを捨てる。
- `MediaInfo::frame_index_at` は、最初のコマより前で VFR も -1 を返す（CFR と同じ）。
- 契約に VFR を足す（`mm_testkit::decode_vfr::vfr_contract`。見本は `SampleVfrDecoder`）。CFR の契約も `frame_range` との一致を見る。
- 影響する実装: VFR の PTS の索引を持つデコーダー（`mm-decode-ffmpeg`）は、`decode_range` を上書きしていないので既定の実装で直る。`frame_index_at` を seek に使う実装は、-1 を受けて「コマなし」として扱う。

### 時刻の演算（`mm-types`、基礎）

分母を `u32 * u32` で掛けていたため、90kHz の PTS と 48kHz の音を足すとあふれた。`Rational` の読み込みが検査を通らず、分母 0 の fps が読めた。

- `RationalTime` の `+` `-` をやめ、`checked_add` / `checked_sub` を足す。最小公倍数の上で計算し、分母が `u32`、分子が `i64` に収まらなければ `Err(InvalidArgument)`。
- `scale` / `div_by` / `from_frames` / `TimeRange::duration` / `Layer::to_source_time` / `Composition::range` は `Result` を返す。
- `Rational` は分子・分母とも 1 以上を型で保ち、読み込みも `Rational::new` を通る（約分も）。`Rational::integer(0)` は定数ならコンパイルエラー。実行時の値は `Rational::new` で作る。
- `TimeRange` の読み込みも `TimeRange::new` を通る。
- 文書は `mm_doc::load` の最後に `Project::validate`（長さが負でない、レイヤーの範囲が作れる）を通す。
- `RationalTime` の `Add` / `Sub` を使っていた呼び出し側は全部、`checked_*` に直した（`mm-doc`、`mm-render`、`mm-testkit`、`mm-export`、`mm-render-api`）。

## 足すだけ・doc だけ

- `mm-render-api`: `render_frame` が返す `FrameRef::Gpu` の寿命（直近 `KEPT_RENDERED_FRAMES` 回の返り値を有効に保つ）と、`render_range` の sink に渡すコマの寿命（sink の呼び出しの中だけ）を doc に書く。`KEPT_RENDERED_FRAMES = 8` を足し、`mm-render` の保持数がこれを使う。契約に、寿命の内側のコマが読み戻せることを足す。
- `mm-render-api`: `FrameCacheKey::digest` を FNV-1a にする。値が Rust の版をまたいで変わらず、ディスクのキャッシュの名前に使える。固定値と全欄の検査を足す。digest の値は変わるが、段階 0 には永続のキャッシュが無い。
- `mm-types`: `GpuAdapterId::matches` はベンダー・デバイス・名前だけで照合する（LUID は起動のたびに変わるため、保存した設定の照合に使えない）。同じ機種の複数枚から 1 枚を選ぶ `is_same_instance` を足す。
- `mm-gpu`: `GpuContext::shared` は、作ってあるものが求めに合わなければ `Err`。`read_back` は形式と大きさが合わなければ `Err(Unsupported)`。
- `mm-node-api`: `audio_frames_in` はサンプルレート 0 で 0 を返す（panic しない）。
- `mm-cache-api`: `ContentHash::short` は文字の途中で切らない。
- `mm-trace`: `MetricsHandle::drain` は取り出しと消去を 1 回のロックで行う。使われていない `TraceLevel` を消す。
- `mm-types`: 使われていない `Clock` / `ManualClock` / `SystemClock` を消す。時計の差し替えが要る所（命令と保存）は段階 1 で入れる。
- `mm-testkit`: 契約を強くする。
  - 見本の描画器が `Layer.transform` を掛ける。契約に、移動・拡大・失敗したレイヤー（`Err` のレイヤーだけ透明）を足す。
  - `NodeFixture.inputs` を足し、`is_identity` の問い合わせと描画に同じ入力を使う。`ConstInputs` は入力ごとの絵を持つ。
  - ベクターの契約は、絵の外接矩形の縁が 2 倍の位置に来ることを見る。
  - ミキサーの契約は、短い入力の中身の位置と後ろの無音、長い入力の切り方を見る。
- `mm-runtime`: `GpuContext` を `mm-gpu` から直接使う。GPU の文脈を作る要否は `RuntimeConfig::needs_gpu`（描画器とデコーダーの選択）で決める。
- `mm-export`: エンコーダーの失敗でそこで止め、最初のエラーを返す。映像のコマ数が半端な fps（29.97 で 1 秒など）では、音も映像の長さに揃える。
- `apps/mm`: `mm-render` への依存をやめる（GPU の名前は `Engine::gpu_adapter()`）。既定のプロジェクトは作業フォルダー基準。読み込みの I/O エラーは種類を保つ。起動の測定の子に時間切れを付ける。

## 移行の手順

1. 基礎（`mm-types`、`mm-doc`）を直し、呼び出し側を全部コンパイルが通るまで直す。
2. 口（`mm-host-api`、`mm-decode-api`、`mm-render-api`）と契約（`mm-testkit`）を同じ PR で更新する。
3. 実装のクレートは、口と基礎の変更に合わせるのに要る所だけ変える。
