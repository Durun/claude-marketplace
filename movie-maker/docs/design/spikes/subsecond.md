# 試作: subsecond で Rust の関数を差し替える

[ロードマップ](../15-roadmap.md)の段階 0 の難所「Rust の関数の差し替え」の結果。出典の番号は [references.md](../references.md) の [8]。

## 結論

- **Windows 11 で動く。** 素の実行ファイルと最小の Tauri 2 のアプリの両方で、動かしたまま関数を差し替えられた。文書の状態（メモリの中の `Project`、呼び出しの通し番号）は差し替えをまたいで残り、Tauri の窓も開いたまま。
- **ただし、この大きさでは速さの利益は小さい。** 保存から反映まで素の実行ファイルで 0.7 秒、Tauri で 1.5 秒。比べる `cargo build` の増分と再起動は 1.1 秒と 2.8 秒。得るものは速さより、状態と窓を落とさないこと。
- **Tauri は Windows でそのままは動かない。** `dx` の Windows のリソースと `tauri-build` のリソースが二重になる。省くための手当てが要る（下の「詰まった所」）。
- 段階 0 では採用してよい。ただし本番の Tauri のアプリに入れる前に、実物の大きさで測り直す。

## 環境

| 項目 | 値 |
|---|---|
| OS | Windows 11 Home 10.0.26200 |
| rustc / cargo | 1.95.0 |
| `dx`（dioxus-cli） | 0.7.10（57d6794）。`cargo install dioxus-cli --locked` |
| subsecond / dioxus-devtools | 0.7.10 |
| Tauri / tauri-build | 2.12.5 / 2.7.4（パッチ済み） |
| 実行 | `dx serve --hot-patch --platform desktop --interactive false --open false` |

## 作ったもの

`movie-maker/spikes/subsecond/`。本体のワークスペースの外。

| 場所 | 中身 |
|---|---|
| `ops-sample/` | `mm-doc` の `Project` を受けて変える関数（`mm-ops` の操作の見本）。別クレートの関数が差し替わるかを見る |
| `plain/` | 素の実行ファイル。200ms ごとに先端のクレートの関数と `ops-sample` の関数を `subsecond::call` で呼び、印と epoch ms を 1 行出す |
| `tauri-app/` | 最小の Tauri 2。WebView が 200ms ごとに Tauri のコマンドを呼び、コマンドが `subsecond::call` の中で `ops-sample` を呼んで同じ形の 1 行を出す |
| `scripts/` | `measure.sh`（印を書き換えた時刻から、新しい印が出るまで）、`repeat.sh`（繰り返し）、`baseline.sh`（`cargo build` の増分と再起動） |

測り方は[計装](../17-instrumentation.md)の考え方に合わせ、時計を 1 つにそろえる。保存の代わりに `sed` で印を書き換えた時刻 T0 と、アプリ自身が出した TICK の epoch ms の T1 の差を取る（ファイル監視の待ち、再ビルド、リンク、パッチの読み込み、次の呼び出しまで全部入る）。

## 実測

単位は ms。

### 保存から反映まで（subsecond）

| 形 | 差し替える対象 | 回数 | 中央値 | 範囲 |
|---|---|---|---|---|
| 素の実行ファイル | 先端のクレートの関数 | 7 | 672 | 437〜979 |
| 素の実行ファイル | 別クレート（`ops-sample`）の関数 | 6 | 853 | 726〜967。ほかに 1 回 4,911（`dx` の patch が 2.9 秒かかった） |
| Tauri のコマンド | 先端のクレートの関数 | 8 | 1,476 | 1,427〜1,707。最初の 5 回は 1,431〜5,015 |

`dx` が出す `Hot-patching ... took` は素の実行ファイルで 330〜480、Tauri で 1,270〜1,330。残りはファイル監視と反映。

### 逃げ道（`cargo build` の増分と再起動）

| 形 | 回数 | build | 再起動（起動から最初の出力） | 合計の中央値 |
|---|---|---|---|---|
| 素の実行ファイル（先端） | 5 | 539〜1,674 | 39〜232 | 1,077 |
| 素の実行ファイル（別クレート） | 4 | 774〜1,208 | 43〜108 | 1,114 |
| Tauri（先端） | 3 | 1,953〜2,473 | 687〜797 | 2,801 |

別クレートの最初の 1 回は 13,078（`dx` の後で依存が作り直された）。Tauri の再起動には WebView2 の起動を含む。

### 参考: 最初のビルド

`dx` の最初のビルド（fat binary）は、素の実行ファイルで約 36 秒（依存を含む）、Tauri で約 14 秒（依存は作成済み）。毎回ではなく起動時の 1 回。

## 分かったこと

- 差し替えは `subsecond::call` の中でだけ効く。`call` の外から呼ぶ関数は古いまま。操作の入口と Tauri のコマンドの本体を `call` で包む。
- 差し替えが効くクレートは、`dx` で起動したワークスペースのメンバーだけ。ワークスペースの外のパスの依存（`tauri-app` から見た `ops-sample`）を書き換えても `dx` は動かない。`mm-ops` を別クレートにするなら、アプリと同じワークスペースに置く。
- 同じワークスペースに、起動していない別のアプリ（`plain` と `tauri-app`）が入っていると、共通のクレートを書き換えたときに `dx` が `Missing rustc args for replay: 'spike_tauri'` で失敗する。アプリごとにワークスペースを分けるか、動かすアプリだけを入れる。
- 状態は残る。Tauri の管理する状態（`Doc`）の通し番号は差し替えの間も続いた。構造体の形を変える変更は試していない（subsecond の文書では未対応）。

## 詰まった所

1. **ポインタ 1 本分のクロージャは差し替えを素通りする。** Tauri のコマンドで `subsecond::call(|| op_body(&doc))` と書くと、閉じ込めたのが参照 1 つ（8 バイト）で、`subsecond` がそれを関数ポインタとみなしてジャンプ表を引けず、古いまま動く。`dx` は成功と出し、エラーも出ない。別スレッドの対照（何も捕まえないクロージャ）は差し替わっていたので気づいた。参照に通し番号を足して 2 本分にしたら効いた。素の実行ファイルは参照 2 つを捕まえていたので当たらなかった。
2. **Windows のリソースが二重になる。** Tauri のビルドは `resource.lib`（アイコン、バージョン、comctl32 v6 のマニフェスト）を作り、`dx` も自前の `resource.lib` をリンクするので、`rust-lld: error: duplicate resource` で fat binary が作れない。試作では `tauri-build` を `[patch.crates-io]` で写して、環境変数 `MM_SKIP_WINRES` のときだけ `res.compile()` を省き、マニフェストを `/MANIFEST:EMBED` と `/MANIFESTDEPENDENCY` でリンカーに埋めた。マニフェストを省くと起動時に `0xc0000139`（エントリポイントなし）で落ちる。起動は `MM_SKIP_WINRES=1 dx serve ...`。本番に入れるなら `tauri-build` 側に切り替えを出すか、`dx` の `write_winres` を止める上流への報告が要る。
3. `dx` は Dioxus でないクレートでも `--platform desktop` を付ければ起動する。`dioxus_devtools::connect_subsecond()` を `main` の最初に呼ぶとパッチを受ける。
4. WebView2 を起動したまま `dx` を落とすと、ユーザーデータのフォルダが使用中のまま残り（`0x800700AA`）、次の起動で窓が作れない。`dx.exe` の子まで閉じる（`taskkill /F /T`）。

## 逃げ道の時間

差し替えを使えない場合の逃げ道（Rust は増分のビルドと再起動、文書は自動保存から復元）は、この大きさで素の実行ファイルが約 1.1 秒、Tauri が約 2.8 秒。差し替えとの差は 0.4〜1.3 秒で、速さより状態の保持が利点。本番の大きさではどちらも伸びる（リンクの時間と、起動後に文書とデコーダー、GPU を作り直す時間）。自動保存からの復元の時間は文書の保存が出来てから測る。

## 次にやること

- 実物の大きさ（`mm-ops` と `mm-doc`、wgpu を含むアプリ）で測り直す。リンクの時間が伸びるので、`cargo build` の増分との差が開くかを見る。
- `mm-ops` が別クレートで差し替わるか、Tauri 側のワークスペースにまとめて確かめる（素の実行ファイルでは確認済み）。
- 構造体の形を変えたときの挙動と、文書の復元の流れを確かめる。
- 再現: `spikes/subsecond/` で `dx serve --hot-patch --platform desktop -p spike-plain`、`tauri-app/` で `MM_SKIP_WINRES=1 dx serve --hot-patch --platform desktop`。測定は `scripts/repeat.sh <ログ> <ソース> <印の接頭辞> <回数> <開始の版>`。
