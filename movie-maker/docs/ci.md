# CI

`.github/workflows/movie-maker.yml`。`movie-maker/**` か workflow 自体を変えた PR、main への push、手動の起動（`workflow_dispatch`）、夜間（`schedule`、毎日 03:00 JST）で走る。権限は `contents: read` だけで、秘密の値は使わない（例外は `perf-record` の `issues: write`）。Rust のバージョンは `rust-toolchain.toml` に従う。夜間は `perf-record` だけが走る。

| ジョブ | 環境 | いつ | 内容 |
|---|---|---|---|
| `check` | windows-latest | PR、main、手動 | `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（main と手動は `--include-ignored` を付け、通しの試験も回す）。外の SDK（FFmpeg、OCIO、CEF）無しの既定の設定。GPU は無いので GPU のテストはスキップされる（`MM_REQUIRE_GPU` は付けない）。`fixtures/golden/warp/` があれば、WARP で期待画像のテストを必須で回す手順が続く |
| `ffmpeg` | windows-latest | PR、main、手動 | BtbN の FFmpeg 9.0 の shared ビルドを展開し、feature `ffmpeg` などを入れた clippy とテスト（feature で中身が変わるクレートと `mm-ingest` だけ。main と手動は通しの試験も回す）、`mm render fixtures/projects/stage0/project.json` の書き出し。LLVM は runner のものを使い、無ければ choco で入れる。手順の元は [mm-decode-ffmpeg の README](../crates/mm-decode-ffmpeg/README.md) |
| `semver` | ubuntu-latest | PR のみ | `cargo-semver-checks` で `crates/mm-*-api` の全部を PR の基点（`origin/<base>`）と比べる |
| `conventions` | ubuntu-latest | PR、main、手動 | `scripts/check-conventions.sh`。ルートの `Cargo.toml` の `mm-*` が名前の順にまとまっているか。PR では、足した RFC のファイルの名前が PR の番号で始まるか（[並行開発の境界](design/14-dev-structure.md#並行開発の境界)） |
| `ui` | ubuntu-latest | `movie-maker/ui/**` か `crates/mm-ops/schema/**` を変えた PR、main、手動 | Node 24 と pnpm 12（pnpm のキャッシュ）。`pnpm install --frozen-lockfile` のあと `gen:check`（コミット済みの `generated/ops.ts` が `ops.json` と合うか）、`build`、`typecheck`、`lint`、`test`。PR で `ui/` に差が無ければ、ジョブは走るが各手順をスキップする |
| `perf-gate` | windows-latest | PR のみ（docs、ui、テストだけの PR は測らず成功で終わる） | 性能の回帰のゲート。基点と PR の `mm` を作り、`scripts/perf-gate.sh` で `mm perf run cpu` と、WARP の `playback`・`scrub` を交互に回して比べる。下の「性能の回帰のゲート」 |
| `perf-record` | windows-latest | main、夜間 | `mm perf run cpu` を 10 回測り、結果を artifact（90 日）に残し、予算を超えた項目を issue にする。`issues: write` を持つのはこのジョブだけ |

## PR と main の違い

PR は速く返すことを優先し、1 本が 30 秒を超える通しの試験を `#[ignore = "通しの試験。main への push と手動実行で走らせる"]` で外す。main への push と手動の起動は `-- --include-ignored` で全部を回す。測定用の `#[ignore]`（`scale` の 2 本。release で手動）は `--skip` で除く。

| 項目 | PR | main の push、手動 |
|---|---|---|
| 通しの試験（`stage3_e2e`、`stage4_e2e`、`captions_loudness`、`caption_burn_in`、`render` の 2 レンダーターゲット、`mm` の kill の試験、`mm-store` の kill の試験 2 本） | 回さない | 回す（`check` と `ffmpeg`） |
| `ffmpeg` ジョブのテスト | feature で中身が変わるクレートと `mm-ingest` だけ。ほかは `check` が回す | 同じ |
| `perf-gate` | 性能に関わるファイルを変えたときだけ | なし（`perf-record` が測る） |

手元で全部を回すときは `cargo test --workspace -- --include-ignored --skip 操作_10_万件の大きさ --skip 一万語の素材`。

## API クレートのバージョンの検査

公開しないクレート（`publish = false`、バージョン 0.1.0）でも `--baseline-rev` で動くことを確かめた。公開メソッドを消すと「次のバージョン（0.x では minor）を上げろ」で失敗する。
API クレートのインターフェースを壊す PR は、バージョンを上げるか、壊さない形にする。`workspace.package.version` は全クレートで共有なので、バージョンの上げ方はルートの `Cargo.toml` の担当（統合）に合わせる。

## 手元で回す

`movie-maker/` で実行する。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
sh scripts/check-ffmpeg.sh clippy   # FFmpeg あり（FFMPEG_DIR などは README のとおり）
sh scripts/check-ffmpeg.sh render
cargo install cargo-semver-checks --locked
cargo semver-checks $(for d in crates/mm-*-api; do printf -- '-p %s ' "$(basename "$d")"; done) --baseline-rev origin/main
```

Windows でワークツリーのパスが長いと、semver-checks の基点のビルドが `LNK1104` で失敗する。`CARGO_TARGET_DIR=C:/t/sc` のように短い場所を指す。

## 性能の回帰のゲート

先行事例は Firefox の Perfherder（閾値を超えた点を確かめてから警告にする）と、同じ runner で基点と変更を交互に回して比べる Bencher などの相対比較。runner ごとに速さが違うので、保存した数との絶対値の比較はせず、PR のたびに基点（`origin/<base>`）の `mm` も作って同じ runner で比べる。新しく設計した所は、小さな絶対値の差を除く `--min-delta` と、回帰が出たら回数を倍にして集約し直す確認。

### CI で回すシナリオと回さないシナリオ

| シナリオ | 場所 | 項目 |
|---|---|---|
| `cpu` | CI（`perf-gate`、`perf-record`） | `ops.apply_ms`（レイヤー 2000 個で操作 1 つ）、`ops.validate_ms`、`store.commit_ms`（保存のコミット 1 回） |
| `playback`、`scrub`（WARP） | CI（`perf-gate`） | 描画の 1 フレーム、スクラブの p50 と p99。予算とは比べず、基点との相対だけ見る |
| `playback`、`scrub`（実機） | 手元の GPU | 同じ項目を 1080p で。予算とベースラインと比べる |
| `export`、`startup` | 手元の GPU と FFmpeg | 書き出しの fps、起動 |

GitHub Actions の runner に実機の GPU は無く、FFmpeg の素材も無い。CI は `cpu` と、WARP（D3D12 のソフトウェアの GPU。`Microsoft Basic Render Driver`）で回す `playback`・`scrub` を前後で比べ、実機の GPU のシナリオは手元で回してベースラインと予算と比べる。WARP の速さは実機と別物なので、予算とは比べない。目的は、描画の経路（レンダラー、キャッシュ、リードバック）が遅くなった PR を GPU の無い CI でも見つけること。WARP のシナリオは `mm perf run playback|scrub --adapter "Microsoft Basic Render Driver" --size 480x270` で、遅い GPU でも回数を稼げるよう小さい画像で短く回す（`playback` は 1 秒、`scrub` は 20 回シーク、`--repeat 3`）。divan のベンチは使わず、`mm perf run cpu` に一本化した。divan の出力は機械で読める形でなく、通しのシナリオと同じ結果の形（`mm perf compare` が読む）にそろえるため。

### しきい値と回数

| 設定 | 値 | 理由 |
|---|---|---|
| 閾値（`--threshold`） | 30% | 同じ runner で交互に回しても、共有 runner は隣のジョブの負荷で揺れる〔推。CI の実測で見直す〕。手元の同じ `mm` どうしの比較は 1% 未満。設計書の既定 5% は手元の GPU 用 |
| 絶対値の下限（`--min-delta`） | 0.05 ms | 0.02 ms が 0.03 ms になるような差を回帰にしない |
| 1 回の測定（`--repeat`） | 5 | 中央値を取る |
| ラウンド | 基点と PR を 3 回ずつ交互 | 全部を集約した中央値で比べる。外れ値の 1 回に引きずられない |
| 確認 | 回帰が出たらさらに 3 ラウンドを足し、集約し直して再判定 | 1 度の揺れで落とさない |

WARP のシナリオは別の閾値で比べる。

| 設定 | 値 | 理由 |
|---|---|---|
| 閾値（`PERF_WARP_THRESHOLD`） | 40% | 同じ `mm` どうしを windows-latest で 5 回（1 回は 3 ラウンドを集約した比較）比べた揺れは、p50 と平均の項目で ±3.5% 以内、p99 の項目で最大 +18.4%（`scrub.full_p99_ms`）、-17.0%、+16.7%（`playback.frame_p99_ms`）。p99 は 1 回の繰り返しの最大に近く揺れる。最大の揺れの 2 倍強に置き、描画が 1.4 倍以上遅くなる変更を拾う |
| 絶対値の下限（`PERF_WARP_MIN_DELTA`） | 2 ms | WARP のフレームは 20〜70 ms なので、2 ms 未満の差は無視 |
| 1 回の測定 | `--repeat 3` | `playback` は 1 回に 2 秒（キャッシュなしとありを 1 秒ずつ）、`scrub` は 20 回シーク |

`playback.dropped_pct` と `playback.cached_dropped_pct` は WARP では常に 100%（60 fps に追いつかない）で、比較では動かない。回帰を見るのは `*_frame_ms`、`*_p99_ms`、`playback.cpu_ms`、`playback.gpu_ms`、`scrub.*`。

WARP の揺れの実測は、基点に `--adapter` がまだ無いこの変更の PR で、PR の `mm` どうしを比べて取った（基点を足した後は本来の基点比べになる）。

所要時間は、キャッシュが冷たい最初の実行で `perf-gate` が 7 分（基点の mm のビルド 5 分 12 秒、PR の mm 26 秒、測定は 6 秒）。測定そのものは数十秒で、残りはビルド。runner の値は手元の約 2 倍（`ops.apply_ms` 3.7 ms、`store.commit_ms` 8.5 ms）で、予算には収まる。

WARP のシナリオを足した後の所要時間は、2026-11 の実行で、PR の mm のビルドが 4 分 22 秒（冷たいキャッシュ）、基点のビルドが 53 秒、比較が約 3.6 分（cpu と WARP の 3 ラウンド）。回帰が出て確認のラウンドを足すとさらに約 3.6 分。ジョブの上限は 45 分。

環境変数 `PERF_THRESHOLD`、`PERF_MIN_DELTA`、`PERF_ROUNDS`、`PERF_REPEAT`、`PERF_WARP_*`（`THRESHOLD`、`MIN_DELTA`、`REPEAT`、`ADAPTER`、`SIZE`、`SECONDS`、`SEEKS`）、`PERF_WARP=0`（WARP のシナリオを回さない）で `scripts/perf-gate.sh` の値を変えられる。基点に `perf run cpu` や `--adapter` がまだ無い PR（そのシナリオや引数を足す PR 自身）では、比較をスキップして PR の側だけ測る。

### WARP の期待画像

`check` ジョブは `fixtures/golden/warp/` があるとき、`MM_GOLDEN_BACKEND=warp MM_REQUIRE_GOLDEN=1 MM_REQUIRE_GPU=1` で `cargo test -p mm-render --test golden` をもう一度回す。runner のアダプターが WARP でなければ（`MM_GOLDEN_BACKEND` の食い違いで）失敗し、WARP の期待画像が欠けていても失敗する。WARP の期待画像が無いうちはこの手順をスキップする。

### ベースラインの置き場所と更新

- CI の比較の相手は、基点の `mm` を同じ runner で測った数で、保存しない。更新の手間がない。
- main と夜間の `cpu` の結果は artifact `perf-<sha>`（90 日）に残る。推移を見たいときは落として `mm perf compare` に渡す。
- 手元の GPU のベースラインは `perf/baseline/rtx5090/<シナリオ>.json`（git に入れる）。意図して速さが変わった PR で、次を回して差し替える。

```sh
cargo build --release -p mm
target/release/mm perf run playback --seconds 5 --repeat 3 --baseline --out-dir perf/baseline/rtx5090
target/release/mm perf run scrub --baseline --out-dir perf/baseline/rtx5090
# export と startup は --features ffmpeg の mm を、PATH に FFmpeg の bin を通して回す
```

手元で回帰を見るとき:

```sh
target/release/mm perf run scrub --out-dir .mm/perf
target/release/mm perf compare perf/baseline/rtx5090/scrub.json .mm/perf --threshold 10%
target/release/mm perf budget .mm/perf
```

`playback` は実時間の速さで回し、フレームのキャッシュなしとありで 1 回ずつ再生するので、1 回に `--seconds` の 2 倍の秒数かかる（既定 10）。予算の表の「キャッシュなし」の行は `playback.*`、「キャッシュあり」の行は `playback.cached_*` に出る。`scrub` は繰り返しで同じ位置に戻るので、キャッシュなしで測る。

### 予算の自動の起票

`perf-record`（main への push と夜間のみ。PR では走らない）が `scripts/perf-file-issues.sh` を呼ぶ。

- `mm perf budget <結果> --json` が、予算を超えた項目を出す。
- 題は `perf: <項目名> が予算を超えた`。同じ題の開いた issue があればコメントを足し、無ければ作る（`gh issue create`）。
- 本文は予算の表の行、測った値、予算、コミット、実行の URL。
- 秘密の値は使わず、`GITHUB_TOKEN` の `issues: write` を `perf-record` にだけ付ける。
- CI の runner は手元の PC より遅いので、起票が出たらまず手元の `mm perf run cpu` で再現するか確かめる。
