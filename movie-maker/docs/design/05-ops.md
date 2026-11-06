# 操作の層

プロジェクトのデータを変えるのは操作だけ。実装は `crates/mm-ops`（基礎の層。API クレートではない）。

## 操作

- 操作は `mm_ops::Op` の 1 つのバリアントで、`mm-ops/src/ops/` に 1 つ 1 ファイル。引数の型（serde + JSON Schema）、検証、適用、逆操作、`touches` を持つ。JSON では `op` のフィールドに名前（camelCase）、残りのフィールドに引数（`{"op":"ripple","cut":"k1","edge":"out","by":{"num":-2,"den":5}}`）。知らないフィールドは拒否する。
- 汎用の操作: `addAsset`、`removeAsset`、`addLayer`、`removeLayer`、`moveLayer`、`setParam`、`removeParam`、`setKeyframe`、`removeKeyframe`、`setEase`、`parentLayer`、`attachLayer`、`lockLayer`、`precompose`、`unprecompose`、`addComposition`、`removeComposition`、`addSequence`、`forkSequence`、`removeSequence`、`addRenderTarget`、`removeRenderTarget`、`addMarker`、`removeMarker`。`remove*` と `unprecompose` は `add*` と `precompose` の逆操作で、人も呼べる。
- 編集の操作（マグネティックシーケンス）: `insert`、`overwrite`、`extract`、`split`、`join`、`ripple`、`roll`、`slip`、`slide`、`disable`、`lift`、`insertCuts`、`extractCuts`。`by` と `at` はシーケンスの時間軸の長さで、素材の時間には `speed` を掛けて変換する。`lift` はカットのノードを隙間（[データの形](03-data-model.md#素材の整理と文字起こし)の `Node::gap`）に替え、長さと後ろの位置を変えない。逆操作は元のカットへの `overwrite`。`insertCuts` と `extractCuts` は続いたカットの列（貼り付けと範囲の削除）で、互いの逆操作。
- レイヤーの時間の操作: `trimLayer`（Timeline の帯の端のつまみ）。`by` はコンポジションの時間軸。イン点は `start` と `in` を一緒に動かして画を止めたまま帯の左端を動かし、アウト点は `out` だけ。タイムリマップのあるレイヤーのイン点は拒否する。
- 選別の操作: `markIn`・`markOut`（素材の印。`at: null` で外す）、`addSelection`・`removeSelection`、`addTopic`・`removeTopic`、`rate`（`target` は選別か話題の ID）、`tag`（`remove: true` で外す。付いているタグを付ける・付いていないタグを外すのは拒否）。どれも素材の ID を `touches` に入れる（素材を外す操作と重ねるため）。`rate` と `tag` は `target` だけ。
- 文字起こしの操作: `deleteWords`（単語の列を消す。`restore: true` で戻す。既にその状態の単語は拒否）、`fixTranscript`（単語 1 つの文字。`text: null` で元に戻す）。本文の表は変えず、`transcript_edits` を変える。`touches` は素材と単語の ID。
- 話題から並びを作る操作: `insertTopics { sequence, index?, topics, cuts }`。話題の素材の範囲を切り出したカット（撮影素材のノード、等速）を、話題の順に 1 つの操作で差し込む（`index` を省くと末尾）。カットの ID は呼び出し側が話題と同じ数だけ決める（やり直しで同じ ID に戻るため）。逆操作は `extractCuts` で、1 回の取り消しで全部抜ける。マクロ（操作の列）にしないのは、取り消しと `mm diff` で 1 つの編集として扱うため。
- パラメータは `ParamRef`（`layer` か `cut` の ID と、`.` 区切りのパラメータのパス）で指す。パスは `opacity`、`transform.position|scale|rotation|anchor`、`source.<名前>`、`effects.<ID>.<名前>`、`masks.<ID>.path|feather|opacity`、`audio.gain_db|pan`。
- 適用した後は必ず `Project::validate` を通す。通らなければ操作は失敗で、プロジェクトのデータは変わらない。
- 逆操作が元のプロジェクトのデータをバイトで戻せるよう、操作は形を絞る。`precompose` は重なりの上で続いた範囲だけ、`unprecompose` は `precompose` が作った形だけ、`join` は `split` が作った続きだけ、`addMarker` は同じ時刻が無いときだけ受ける。
- 操作経路は 4 つで、どれも同じ操作を呼ぶ。wgpu のサーフェス（Rust から直接）、WebView の画面（Tauri の IPC）、Claude の CLI（`mm op <project> ripple --cut k1 --edge out --by -0.4`。動いていなければ `project.mmdb` を直接開く。別のプロセス（アプリ）が書き手のときは、HTTP で転送する形を `mm-serve`（段階 3）で足すまで、「別のプロセスが書いている」と拒否して読み取りだけにする）、Claude の MCP（操作ごとのツール。引数の型はスキーマから作る）。
- 全部の操作の JSON Schema は `mm_ops::ops_schema()` が出し、`crates/mm-ops/schema/ops.json` に凍結する（テストが最新かを確かめる。`MM_UPDATE_SCHEMA=1 cargo test -p mm-ops --test schema` で書き直す）。`mm op --schema`、MCP、`ui/sdk` の型の生成の元。
- CLI の引数は、このスキーマのフィールドの名前から作る（`--cut c12 --edge out --by -0.4`）。時刻のフィールドは `-0.4` と `3/2` を受け（省ける時刻は `null` も）、列のフィールドは JSON の配列（`--topics '["t1","t2"]'`）、パラメータの指し方は `layer:<ID>:<パス>` と略せ、複雑なフィールドは `--json '{...}'` で渡す。元にしたバージョンは `--base <バージョン>`、複数の操作は `--txn <file.json>`（`Transaction` の JSON）で 1 つのトランザクションにする。CLI の操作経路は `Entry::Cli` で、ロックの付いた所に触る操作（ロックを外す取り消しも）は拒否する。
- 操作を適用して書くのは `mm_store::Store::apply_txn`（取り消しとやり直しは `undo_last`、`redo_last`）。古いバージョンを元にした操作の判定は、保存した記録の `touches` で行う（別のプロセスが書いた操作も、取り消しとやり直しの記録も含む）。`mm-store` は `mm-ops` の `OpRecord` と `TxnRecord` の形をそのまま表に書く。

## `touches`

- 操作が読む・書く所の ID の集合（文字列）。書く所だけでなく、参照先（親とその祖先の鎖、接続先、入れ子のコンポジションと、そこから入れ子でたどれる先の全部）、参照元（レンダーターゲット、接続したレイヤー）、シーケンスの隣のカットも含める。入れ子や親の循環を作る操作は、先の内容を読んでいるため。
- 守る性質: `touches` が重ならない 2 つの操作は、1 つずつ別に適用すると、順を入れ替えても各操作の成否と最後のプロジェクトのデータが同じになる。性質テスト（`crates/mm-ops/tests/props.rs`）が、同じプロジェクトのデータに適用する操作の全部の組で守る。
- 同時書き込みの判定と `mm diff` がこれで指す。

## トランザクション

- 複数の操作を 1 つのトランザクション（`Transaction { name, base_version, ops }`）として適用する。ドラッグの連続、マクロ、Claude の対応 1 回。途中で失敗したら全部戻す（`apply_transaction`）。
- プロジェクトのデータとバージョン番号を持つのは、メモリの上では `Editor`、保存では `mm_store::Store`。どちらも同じ数え方で、操作 1 つでバージョンが 1 進み、トランザクションの記録 `TxnRecord { id, kind, name, base_version, version, ops }` を残す（`id` は 1 からの通し番号）。`ops` の各要素は `OpRecord { op, inverse, touches }`（操作と逆操作は JSON の値、`touches` は ID の列）。
- 取り消しもトランザクションの単位。逆操作を逆の順に適用し、その記録（`op` は逆操作、`touches` は適用したときのもの）を `kind = undo { of }` のトランザクションとして残す。やり直しは元の操作を再適用し `kind = redo { of }`。バージョンはどちらも操作の数だけ進み、元のトランザクションの行は書き換えない。取り消した後に新しい操作を適用すると、やり直しの先は破棄する。
- プロジェクトのデータの変更と記録は SQLite の 1 つのトランザクションで書く（[保存の形](09-ingest-storage.md#書き方)）。保存の層は `TxnRecord` に誰が・いつ・どのコメントへの対応かを足して `txns` の 1 行と `ops` の複数行に書き込み、`version` を `meta.doc_version` に置く。

## 同時書き込み

- 1 本の列で順に適用し、プロジェクトのデータのバージョン番号を進め、変わった所を開いているすべての画面に送る。
- 古いバージョンを元にした操作（`base_version` が今のバージョンより前）は、その後に適用した操作の `touches` と重ならなければ適用し、重なれば `Conflict` で拒否する（呼ぶ側が今のプロジェクトのデータで作り直す）。今のバージョンより先を元にしていれば `FutureBase`。CRDT は使わない。

## ロック

- `Layer.lock` の付いたレイヤー（とその効果・マスク）に `touches` が触る操作は、Claude の操作経路（`Entry::Cli`、`Entry::Mcp`）からは `Locked` で拒否する。ロックを外すのも拒否する。操作経路の種類は `OpContext` で渡す。

## 記録と差分

- `project.mmdb` の `txns`（誰が、いつ、どのコメントへの対応として、どのトランザクションの種類で）と `ops`（何を、逆操作、`touches`）の表。バージョンの比較は `mm diff` がこの記録から意味の差分を出し、取り消しと元のトランザクションは対にして集約する（[保存の形](09-ingest-storage.md#mm-diff)）。
- マクロ: 操作の列を作る手順。vlog の編集の好みは、マクロとテンプレートの組（パック）として持つ。
