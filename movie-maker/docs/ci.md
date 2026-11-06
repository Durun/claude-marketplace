# CI

`.github/workflows/movie-maker.yml`。`movie-maker/**` か workflow 自体を変えた PR、main への push、手動の起動（`workflow_dispatch`）、夜間（`schedule`、毎日 03:00 JST）で走る。権限は `contents: read` だけで、秘密の値は使わない（例外は `perf-record` の `issues: write`）。Rust のバージョンは `rust-toolchain.toml` に従う。夜間は `perf-record` だけが走る。

| ジョブ | 環境 | いつ | 内容 |
|---|---|---|---|
| `check` | windows-latest | PR、main、手動 | `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。外の SDK（FFmpeg、OCIO、CEF）無しの既定の設定。GPU は無いので GPU のテストはスキップされる（`MM_REQUIRE_GPU` は付けない） |
| `ffmpeg` | windows-latest | PR、main、手動 | BtbN の FFmpeg 9.0 の shared ビルドを展開し、feature `ffmpeg` などを入れた clippy とテスト、`mm render fixtures/projects/stage0/project.json` の書き出し。LLVM は runner のものを使い、無ければ choco で入れる。手順の元は [mm-decode-ffmpeg の README](../crates/mm-decode-ffmpeg/README.md) |
| `semver` | ubuntu-latest | PR のみ | `cargo-semver-checks` で `crates/mm-*-api` の全部を PR の基点（`origin/<base>`）と比べる |
| `ui` | ubuntu-latest | `movie-maker/ui/**` を変えた PR、main、手動 | Node 24 と pnpm 12（pnpm のキャッシュ）。`pnpm install --frozen-lockfile` のあと `build`、`typecheck`、`lint`、`test`。PR で `ui/` に差が無ければ、ジョブは走るが各手順を飛ばす |
| `perf-gate` | windows-latest | PR のみ | 性能の回帰のゲート。基点と PR の `mm` を作り、`scripts/perf-gate.sh` で `mm perf run cpu` を交互に回して比べる。下の「性能の回帰のゲート」 |
| `perf-record` | windows-latest | main、夜間 | `mm perf run cpu` を 10 回測り、結果を artifact（90 日）に残し、予算を超えた項目を issue にする。`issues: write` を持つのはこのジョブだけ |

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

先行事例は Firefox の Perfherder（閾値を超えた点を確かめてから警告にする）と、同じ runner で基点と変更を交互に回して比べる Bencher などの相対比較。runner ごとに速さが違うので、保存した数との絶対値の比較はせず、PR のたびに基点（`origin/<base>`）の `mm` も作って同じ runner で比べる。新しく設計した所は、小さな絶対値の差を除く `--min-delta` と、回帰が出たら回数を倍にして束ね直す確認。

### CI で回す場面と回さない場面

| 場面 | 場所 | 項目 |
|---|---|---|
| `cpu` | CI（`perf-gate`、`perf-record`） | `ops.apply_ms`（レイヤー 2000 個で操作 1 つ）、`ops.validate_ms`、`store.commit_ms`（保存のコミット 1 回） |
| `playback`、`scrub` | 手元の GPU | 描画の 1 フレーム、落とすフレームの割合、スクラブの p50 と p99 |
| `export`、`startup` | 手元の GPU と FFmpeg | 書き出しの fps、起動 |

GitHub Actions の runner に GPU は無く、FFmpeg の素材も無い。CI は `cpu` を前後で比べ、GPU の場面は手元で回してベースラインと比べる。divan のベンチは使わず、`mm perf run cpu` に一本化した。divan の出力は機械で読める形でなく、通しの場面と同じ結果の形（`mm perf compare` が読む）にそろえるため。

### しきい値と回数

| 設定 | 値 | 理由 |
|---|---|---|
| 閾値（`--threshold`） | 30% | 同じ runner で交互に回しても、共有 runner は隣のジョブの負荷で揺れる〔推。CI の実測で見直す〕。手元の同じ `mm` どうしの比較は 1% 未満。設計書の既定 5% は手元の GPU 用 |
| 絶対値の下限（`--min-delta`） | 0.05 ms | 0.02 ms が 0.03 ms になるような差を回帰にしない |
| 1 回の測定（`--repeat`） | 5 | 中央値を取る |
| ラウンド | 基点と PR を 3 回ずつ交互 | 全部を束ねた中央値で比べる。外れ値の 1 回に引きずられない |
| 確認 | 回帰が出たらさらに 3 ラウンドを足し、束ね直して再判定 | 1 度の揺れで落とさない |

所要時間は、キャッシュが冷たい最初の実行で `perf-gate` が 7 分（基点の mm のビルド 5 分 12 秒、PR の mm 26 秒、測定は 6 秒）。測定そのものは数十秒で、残りはビルド。runner の値は手元の約 2 倍（`ops.apply_ms` 3.7 ms、`store.commit_ms` 8.5 ms）で、予算には収まる。

環境変数 `PERF_THRESHOLD`、`PERF_MIN_DELTA`、`PERF_ROUNDS`、`PERF_REPEAT` で `scripts/perf-gate.sh` の値を変えられる。基点に `perf run cpu` がまだ無い PR（このシナリオを足す PR 自身）では、比較を飛ばして PR の側だけ測る。

### ベースラインの置き場所と更新

- CI の比較の相手は、基点の `mm` を同じ runner で測った数で、保存しない。更新の手間がない。
- main と夜間の `cpu` の結果は artifact `perf-<sha>`（90 日）に残る。推移を見たいときは落として `mm perf compare` に渡す。
- 手元の GPU のベースラインは `perf/baseline/rtx5090/<場面>.json`（git に入れる）。意図して速さが変わった PR で、次を回して差し替える。

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

`playback` は実時間の速さで回すので、1 回に `--seconds` 秒かかる（既定 10）。

### 予算の自動の起票

`perf-record`（main への push と夜間のみ。PR では走らない）が `scripts/perf-file-issues.sh` を呼ぶ。

- `mm perf budget <結果> --json` が、予算を超えた項目を出す。
- 題は `perf: <項目名> が予算を超えた`。同じ題の開いた issue があればコメントを足し、無ければ作る（`gh issue create`）。
- 本文は予算の表の行、測った値、予算、コミット、実行の URL。
- 秘密の値は使わず、`GITHUB_TOKEN` の `issues: write` を `perf-record` にだけ付ける。
- CI の runner は手元の PC より遅いので、起票が出たらまず手元の `mm perf run cpu` で再現するか確かめる。
