# 命令の層

文書を変えるのは命令だけ。実装は `crates/mm-ops`（基礎の層。口のクレートではない）。

## 命令

- 命令は `mm_ops::Op` の 1 つの枝で、`mm-ops/src/ops/` に 1 つ 1 ファイル。引数の型（serde + JSON Schema）、検証、適用、逆命令、`touches` を持つ。JSON では `op` の欄に名前（camelCase）、残りの欄に引数（`{"op":"ripple","cut":"k1","edge":"out","by":{"num":-2,"den":5}}`）。知らない欄は断る。
- 汎用の命令: `addLayer`、`removeLayer`、`moveLayer`、`setParam`、`removeParam`、`setKeyframe`、`removeKeyframe`、`setEase`、`parentLayer`、`attachLayer`、`lockLayer`、`precompose`、`unprecompose`、`addComposition`、`removeComposition`、`addSequence`、`forkSequence`、`removeSequence`、`addRenderTarget`、`removeRenderTarget`、`addMarker`、`removeMarker`。`remove*` と `unprecompose` は `add*` と `precompose` の逆命令で、人も呼べる。
- 編集の命令（磁気の並び）: `insert`、`overwrite`、`extract`、`split`、`join`、`ripple`、`roll`、`slip`、`slide`、`disable`。`by` と `at` は並びの時間軸の長さで、素材の時間には `speed` を掛けて写す。`lift`（抜いた所に隙間を残す）は隙間の動画オブジェクトの種類を決めてから足す。選別（`markIn`・`markOut`、`addSelection`、`rate`、`tag`、`addTopic`）と文字起こし（`deleteWords`、`fixTranscript`）は段階 3。
- パラメータは `ParamRef`（`layer` か `cut` の ID と、`.` 区切りの道筋）で指す。道筋は `opacity`、`transform.position|scale|rotation|anchor`、`source.<名前>`、`effects.<ID>.<名前>`、`masks.<ID>.path|feather|opacity`、`audio.gain_db|pan`。
- 当てた後は必ず `Project::validate` を通す。通らなければ命令は失敗で、文書は変わらない。
- 逆命令が元の文書をバイトで戻せるよう、命令は形を絞る。`precompose` は重なりの上で続いた範囲だけ、`unprecompose` は `precompose` が作った形だけ、`join` は `split` が作った続きだけ、`addMarker` は同じ時刻が無いときだけ受ける。
- 入口は 4 つで、どれも同じ命令を呼ぶ。wgpu の面（Rust から直接）、WebView の画面（Tauri の IPC）、Claude の CLI（`mm op <project> ripple --cut k1 --edge out --by -0.4`。動いていなければ `project.mmdb` を直接開く。別のプロセス（アプリ）が書き手のときは、HTTP で転送する形を `mm-serve`（段階 3）で足すまで、「別のプロセスが書いている」と断って読み取りだけにする）、Claude の MCP（命令ごとのツール。引数の型はスキーマから作る）。
- 全部の命令の JSON Schema は `mm_ops::ops_schema()` が出し、`crates/mm-ops/schema/ops.json` に凍結する（テストが最新かを確かめる。`MM_UPDATE_SCHEMA=1 cargo test -p mm-ops --test schema` で書き直す）。`mm op --schema`、MCP、`ui/sdk` の型の生成の元。
- CLI の引数は、このスキーマの欄の名前から作る（`--cut c12 --edge out --by -0.4`）。時刻の欄は `-0.4` と `3/2` を受け、パラメータの指し方は `layer:<ID>:<道筋>` と略せ、複雑な欄は `--json '{...}'` で渡す。元にした版は `--base <版>`、複数の命令は `--txn <file.json>`（`Transaction` の JSON）で 1 つのまとまりにする。CLI の入口は `Entry::Cli` で、ロックの付いた所に触る命令（ロックを外す取り消しも）は断る。
- 命令を当てて書くのは `mm_store::Store::apply_txn`（取り消しとやり直しは `undo_last`、`redo_last`）。古い版を元にした命令の判定は、保存した記録の `touches` で行う（別のプロセスが書いた命令も、取り消しとやり直しの記録も含む）。`mm-store` は `mm-ops` の `OpRecord` と `TxnRecord` の形をそのまま表に書く。

## `touches`

- 命令が読む・書く所の ID の集合（文字列）。書く所だけでなく、参照先（親とその祖先の鎖、接続先、入れ子のコンポジションと、そこから入れ子でたどれる先の全部）、参照元（レンダーターゲット、接続したレイヤー）、並びの隣のカットも含める。入れ子や親の循環を作る命令は、先の中身を読んでいるため。
- 守る性質: `touches` が重ならない 2 つの命令は、1 つずつ別に当てると、順を入れ替えても各命令の成否と最後の文書が同じになる。性質テスト（`crates/mm-ops/tests/props.rs`）が、同じ文書に当たる命令の全部の組で守る。
- 同時書き込みの判定と `mm diff` がこれで指す。

## トランザクション

- 複数の命令を 1 つのまとまり（`Transaction { name, base_version, ops }`）として当てる。ドラッグの連続、マクロ、Claude の対応 1 回。途中で失敗したら全部戻す（`apply_transaction`）。
- 文書と版番号を持つのは、メモリの上では `Editor`、保存では `mm_store::Store`。どちらも同じ数え方で、命令 1 つで版が 1 進み、まとまりの記録 `TxnRecord { id, kind, name, base_version, version, ops }` を残す（`id` は 1 からの通し番号）。`ops` の各要素は `OpRecord { op, inverse, touches }`（命令と逆命令は JSON の値、`touches` は ID の列）。
- 取り消しもまとまりの単位。逆命令を逆の順に当て、その記録（`op` は逆命令、`touches` は当てたときのもの）を `kind = undo { of }` のまとまりとして残す。やり直しは元の命令を当て直し `kind = redo { of }`。版はどちらも命令の数だけ進み、元のまとまりの行は書き換えない。取り消した後に新しい命令を当てると、やり直しの先は捨てる。
- 文書の変更と記録は SQLite の 1 つのトランザクションで書く（[保存の形](09-ingest-storage.md#書き方)）。保存の層は `TxnRecord` に誰が・いつ・どのコメントへの対応かを足して `txns` の 1 行と `ops` の複数行に写し、`version` を `meta.doc_version` に置く。

## 同時書き込み

- 1 本の列で順に当て、文書の版番号を進め、変わった所を開いているすべての画面に送る。
- 古い版を元にした命令（`base_version` が今の版より前）は、その後に当てた命令の `touches` と重ならなければ当て、重なれば `Conflict` で断る（呼ぶ側が今の文書で作り直す）。今の版より先を元にしていれば `FutureBase`。CRDT は使わない。

## ロック

- `Layer.lock` の付いたレイヤー（とその効果・マスク）に `touches` が触る命令は、Claude の入口（`Entry::Cli`、`Entry::Mcp`）からは `Locked` で断る。ロックを外すのも断る。入口の種類は `OpContext` で渡す。

## 記録と差分

- `project.mmdb` の `txns`（誰が、いつ、どのコメントへの対応として、どの種類で）と `ops`（何を、逆命令、`touches`）の表。版の比較は `mm diff` がこの記録から意味の差分を出し、取り消しと元のまとまりは対で畳む（[保存の形](09-ingest-storage.md#mm-diff)）。
- マクロ: 命令の列を作る手順。vlog の編集の好みは、マクロとテンプレートの組（パック）として持つ。
