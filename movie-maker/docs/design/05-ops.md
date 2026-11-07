# 操作の層

プロジェクトのデータを変えるのは操作だけ。実装は `crates/mm-ops`（基礎の層。API クレートではない）。

## 操作

- 操作は `mm_ops::Op` の 1 つのバリアントで、`mm-ops/src/ops/` に 1 つ 1 ファイル。引数の型（serde + JSON Schema）、検証、適用、逆操作、`touches` を持つ。JSON では `op` のフィールドに名前（camelCase）、残りのフィールドに引数（`{"op":"ripple","cut":"k1","edge":"out","by":{"num":-2,"den":5}}`）。知らないフィールドは拒否する。
- 汎用の操作: `addAsset`、`removeAsset`、`addLayer`、`removeLayer`、`addMask`（`index` で列の位置を指せる。`removeMask` の逆操作が元の位置に戻す）、`removeMask`、`moveLayer`、`setParam`、`removeParam`、`setKeyframe`、`removeKeyframe`、`moveKeyframe`（時刻と中身を 1 つの操作で置き直す。Graph のドラッグ）、`setEase`、`parentLayer`、`attachLayer`、`lockLayer`、`precompose`、`unprecompose`、`addComposition`、`removeComposition`、`addSequence`、`forkSequence`、`removeSequence`、`overrideCut`、`resetCut`、`setReframe`、`setCaption`、`setLayerAudio`、`setMixer`、`setTransition`（カットの頭のトランジションを丸ごと置き換える。`transition: null` で外す。逆操作は前の値を置く同じ操作）、`addRenderTarget`（`index` で並びの位置を指せる。`removeRenderTarget` の逆操作が元の位置に戻す）、`removeRenderTarget`、`addMarker`、`removeMarker`。`remove*` と `unprecompose` は `add*` と `precompose` の逆操作で、人も呼べる。
- 編集の操作（マグネティックシーケンス）: `insert`、`overwrite`、`extract`、`split`、`join`、`ripple`、`roll`、`slip`、`slide`、`disable`、`lift`、`insertCuts`、`extractCuts`。`by` と `at` はシーケンスの時間軸の長さで、素材の時間には `speed` を掛けて変換する。`lift` はカットのノードを隙間（[データの形](03-data-model.md#素材の整理と文字起こし)の `Node::gap`）に替え、長さと後ろの位置を変えない。逆操作は元のカットへの `overwrite`。`insertCuts` と `extractCuts` は続いたカットの列（貼り付けと範囲の削除）で、互いの逆操作。
- レイヤーの時間の操作: `trimLayer`（Timeline の帯の端のつまみ）。`by` はコンポジションの時間軸。イン点は `start` と `in` を一緒に動かして画を止めたまま帯の左端を動かし、アウト点は `out` だけ。タイムリマップのあるレイヤーのイン点は拒否する。撮影素材のレイヤーは、動かした後の `in` が 0 未満、`out` が素材の長さ超になる動かし方を拒否する。
- sequence の長さへの追従: `fitLayer { layer }`。sequence を置いたレイヤーのアウト点を sequence の長さ（有効なカットの和）に合わせ、コンポジションの長さは「これまでレイヤーの終わりと同じだった」か「新しい終わりが超える」ときだけ全レイヤーの終わりの最大に合わせる（自分で決めた長いコンポジションは縮めない）。逆操作は同じ操作に `out_point` と `duration` を明示した形。`Store::apply_txn` が、sequence に触る操作の列に対して、レイヤーの長さが操作の前の sequence の長さと同じ（追従している）レイヤーの分を自動で末尾に足すので、CLI、アプリ、MCP のどこから `insertTopics` や `insert` を出しても同じトランザクションで追従し、取り消し 1 回で戻る（自分で縮めたレイヤーには足さない。列に `fitLayer` が既にあれば足さない）。
- 選別の操作: `markIn`・`markOut`（素材のイン点・アウト点。`at: null` で外す）、`addSelection`・`removeSelection`、`addTopic`・`removeTopic`、`rate`（`target` は選別か話題の ID）、`tag`（`remove: true` で外す。付いているタグを付ける・付いていないタグを外すのは拒否）。どれも素材の ID を `touches` に入れる（素材を外す操作と重ねるため）。`rate` と `tag` は `target` だけ。
- 文字起こしの操作: `deleteWords`（単語の並びを消す。`restore: true` で戻す。既にその状態の単語は拒否）、`fixTranscript`（単語 1 つの文字。`text: null` で元に戻す）。本文の表は変えず、`transcript_edits` を変える。`touches` は素材と単語の ID。`deleteWords` は文字起こしの表示と検索だけを変え、sequence のカットは変えない（Story の Hide）。
- 消して詰める操作: `cutWords { asset, sequence, words, ranges }`（Story の Delete。例 `{"op":"cutWords","asset":"a1","sequence":"s1","words":["w_…_7_2","w_…_4_1"],"ranges":[{"in":{"num":7,"den":2},"out":{"num":9,"den":2}}]}`）。単語を消し、素材の時刻の範囲 `ranges` を、`sequence` のうちその素材の撮影素材のノードを持つカットから抜いて後ろを詰める。カットの素材の時刻は素材の時刻と同じなので、カットの `[in, out)` と範囲の重なりで決める。
  - カットを切るのは `sequence` だけで、同じ素材を使うほかの sequence（本編から `forkSequence` で分けたショートなど）は変えない。削除のフラグは素材に付くので、文字起こしの表示と検索からは全体で外れる。Story は、いま開いているコンポジションの sequence のレイヤーが置く sequence（`story_overview` の `open_sequence`）を、無ければ並べ先の sequence を送る。
  - 範囲がカットの中ほどなら割って間を抜く（後ろの半分は `<元の ID>-<番号>` の空いている ID）。端に掛かれば端を詰め、カットを覆えばカットを抜く。複数のカットにまたがる範囲は、それぞれのカットに同じ規則を適用する。どのカットにも掛からない範囲は単語を消すだけ。
  - 重なるか接する範囲は 1 つにまとめる。隣り合う単語を 1 つの範囲にするのは呼び出し側で、Story は選んだ単語の最初から最後までを `story_range` の規則で 1 つにする（単語の間の無音も抜く）。
  - 既に消した単語は通し、単語もカットも変わらなければ拒否する。逆操作は同じ操作に、変えた単語、`restore: true`、`sequence` のカットの置き換えの列 `splices: [{index, count, cuts}]` を明示した形。1 回の取り消しで単語とカットが戻る。
  - `touches` は素材、単語、`sequence`、置き換えるカットとその前後、接続したレイヤー。sequence に触るので、`fitLayer` の追従が同じトランザクションに付く。
- 話題から並びを作る操作: `insertTopics { sequence, index?, topics, cuts }`。話題の素材の範囲を切り出したカット（撮影素材のノード、等速）を、話題の順に 1 つの操作で差し込む（`index` を省くと末尾）。カットの ID は呼び出し側が話題と同じ数だけ決める（やり直しで同じ ID に戻るため）。逆操作は `extractCuts` で、1 回の取り消しで全部抜ける。マクロ（操作の列）にしないのは、取り消しと `mm diff` で 1 つの編集として扱うため。
- 本編とショートの操作（[データの形](03-data-model.md#入れ子と本編とショートの関係)）:
  - `forkSequence { sequence, new_sequence, relation, cuts, range? }`。`range: {first, last}` で続いたカットだけを分ける（省くと全部）。`relation: "reference"` は参照のショート、`"fork"` は複製のショート。どちらも各カットの `origin_base` に元の値を残す。逆操作は `removeSequence`。
  - `overrideCut { cut, in?, out? }`。参照のショートのカットの in/out の上書きを置く。値のある所は上書きし（`Cut.overrides` のフラグが立つ）、省いた所は上書きを外して元のカットに従う。逆操作は前の上書きを置く `overrideCut`。`resetCut { cut }` は全部外す（逆操作は `overrideCut`）。例 `{"op":"overrideCut","cut":"r1","in":{"num":7,"den":2}}`。
  - 参照のショートのカットの、元に従う所（上書きしていない in/out、素材、速さ、有効）を直接変える操作（`ripple`、`split`、`disable` など）は拒否する。上書きした in/out はショートの中で `ripple` などで動かせる。
  - 操作を 1 つ適用するたびに、適用した後・`validate` の前に `Project::sync_references` が参照のショートのカットへ元の値を反映する。逆操作で元が戻れば、反映した値も戻る。反映すると上書きした側を元が越えて `in < out` を満たさなくなる本編の操作（上書きした in 以下へ本編の out をリップルするなど）は、上書きを外さずに断り、エラーに本編のカット、ショートのカット、上書きした所を出す。元のカットが消えたカットは最後の値のまま残し、`mm shorts` が「消えた」と出す。
  - `setReframe { layer, cut, transform }`。sequence のレイヤーで、カットの画像に掛ける変形（`transform: null` で外す）。置くときも外すときも、カットがそのレイヤーの sequence に在ることを確かめる（消えたカットの項目を外すと、逆操作が置き直せないため）。`setCaption { layer, caption }`。字幕の置き場所（`null` で既定）。`delivery.burn_in_captions` が真のレンダーターゲットとプレビューが、字幕を絵に描くときにこの置き場所を使う（既定は下部中央。`position` は帯の下端の中央、`width` は折り返しの幅）。どちらも逆操作は前の値を置く同じ操作。例 `{"op":"setReframe","layer":"vert-seq","cut":"r0","transform":{"position":{"value":{"type":"vec2","value":[-30.0,0.0]}},"scale":{"value":{"type":"number","value":3.3}},"rotation":{"value":{"type":"number","value":0.0}},"anchor":null}}`。
  - `touches` は、参照のショートのカットと元のカットの片方に触れば両方（ショートのショートもたどる）と、参照のショートのそれより後ろのカットに接続したレイヤー。
- パラメータは `ParamRef`（`layer` か `cut` の ID と、`.` 区切りのパラメータのパス）で指す。パスは `opacity`、`transform.position|scale|rotation|anchor`、`crop.left|top|right|bottom`（無い辺は `setParam` で作り、`removeParam` で外す）、`source.<名前>`、`effects.<ID>.<名前>`、`masks.<ID>.path|feather|opacity|rect.position|rect.size|rect.radius`、`audio.gain_db|pan`、`reframe.<カットの ID>.position|scale|rotation|anchor`、`caption.position|width`。
- 適用した後は必ず `Project::validate` を通す。通らなければ操作は失敗で、プロジェクトのデータは変わらない。
- 逆操作が元のプロジェクトのデータをバイトで戻せるよう、操作は形を絞る。`precompose` は重なりの上で続いた範囲だけ、`unprecompose` は `precompose` が作った形だけ、`join` は `split` が作った続きだけ、`addMarker` は同じ時刻が無いときだけ受ける。
- 操作経路は 4 つで、どれも同じ操作を呼ぶ。wgpu のサーフェス（Rust から直接）、WebView の画面（Tauri の IPC）、Claude の CLI（`mm op <project> ripple --cut k1 --edge out --by -0.4`。動いていなければ `project.mmdb` を直接開く。別のプロセス（アプリ）が書き手で HTTP サーバーがあれば、[HTTP サーバー](#http-サーバー)で転送する。HTTP サーバーが無ければ「別のプロセスが書いている」と拒否して読み取りだけにする）、Claude の MCP（操作ごとのツール。引数の型はスキーマから作る）。
- 全部の操作の JSON Schema は `mm_ops::ops_schema()` が出し、`crates/mm-ops/schema/ops.json` に凍結する（テストが最新かを確かめる。`MM_UPDATE_SCHEMA=1 cargo test -p mm-ops --test it` で書き直す）。`mm op --schema`、MCP、`ui/sdk` の型の生成の元。
- CLI の引数は、このスキーマのフィールドの名前から作る（`--cut c12 --edge out --by -0.4`）。時刻のフィールドは `-0.4` と `3/2` を受け（省ける時刻は `null` も）、列のフィールドは JSON の配列（`--topics '["t1","t2"]'`）、パラメータの指し方は `layer:<ID>:<パス>` と略せ、複雑なフィールドは `--json '{...}'` で渡す。元にしたバージョンは `--base <バージョン>`、複数の操作は `--txn <file.json>`（`Transaction` の JSON）で 1 つのトランザクションにする。CLI の操作経路は `Entry::Cli` で、ロックの付いた所に触る操作（ロックを外す取り消しも）は拒否する。
- 操作を適用して書くのは `mm_store::Store::apply_txn`（取り消しとやり直しは `undo_last`、`redo_last`）。古いバージョンを元にした操作の判定は、保存した記録の `touches` で行う（別のプロセスが書いた操作も、取り消しとやり直しの記録も含む）。`mm-store` は `mm-ops` の `OpRecord` と `TxnRecord` の形をそのまま表に書く。

## `touches`

- 操作が読む・書く所の ID の集合（文字列）。書く所だけでなく、参照先（親とその祖先の鎖、接続先、入れ子のコンポジションと、そこから入れ子でたどれる先の全部）、参照元（レンダーターゲット、接続したレイヤー）、ノードが参照する素材（撮影素材、LUT）、シーケンスの隣のカットも含める。入れ子や親の循環を作る操作は、先の内容を読んでいるため。
- 守る性質: `touches` が重ならない 2 つの操作は、1 つずつ別に適用すると、順を入れ替えても各操作の成否と最後のプロジェクトのデータが同じになる。性質テスト（`crates/mm-ops/tests/props.rs`）が、同じプロジェクトのデータに適用する操作の全部の組で守る。
- 同時書き込みの判定と `mm diff` がこれで指す。

## トランザクション

- 複数の操作を 1 つのトランザクション（`Transaction { name, base_version, ops }`）として適用する。ドラッグの連続、マクロ、Claude の対応 1 回。途中で失敗したら全部戻す（`apply_transaction`）。
- プロジェクトのデータとバージョン番号を持つのは、メモリの上では `Editor`、保存では `mm_store::Store`。どちらも同じ数え方で、操作 1 つでバージョンが 1 進み、トランザクションの記録 `TxnRecord { id, kind, name, base_version, version, ops }` を残す（`id` は 1 からの通し番号）。`ops` の各要素は `OpRecord { op, inverse, touches }`（操作と逆操作は JSON の値、`touches` は ID の列）。
- 取り消しもトランザクションの単位。逆操作を逆の順に適用し、その記録（`op` は逆操作、`touches` は適用したときのもの）を `kind = undo { of }` のトランザクションとして残す。やり直しは元の操作を再適用し `kind = redo { of }`。バージョンはどちらも操作の数だけ進み、元のトランザクションの行は書き換えない。取り消した後に新しい操作を適用すると、やり直しの先は破棄する。
- アプリの Ctrl+Z（取り消し）と Ctrl+Y（やり直し）は、発行元を問わず直前のトランザクションを戻す（人の操作だけを選んで戻さない。戻す順を変えると、後の操作が前提にした状態が崩れるため）。Claude や CLI の操作を戻したときは、編集の文言に書き手とトランザクションの名前（`mcp の操作を取り消した: <名前>`）を出す。戻すものが無いとき、戻せなかったときも文言を出す。
- アプリのキーとドラッグの編集は、計画から適用までを 1 本のキューで順に行い、計画に使ったプロジェクトのデータのバージョンを `base_version` に付ける。間に CLI などが同じ所を触っていれば `Conflict` で断り、古い編集点に適用しない。
- プロジェクトのデータの変更と記録は SQLite の 1 つのトランザクションで書く（[保存の形](09-ingest-storage.md#書き方)）。保存の層は `TxnRecord` に誰が・いつ・どのコメントへの対応かを足して `txns` の 1 行と `ops` の複数行に書き込み、`version` を `meta.doc_version` に置く。

## 提案

Claude の対応 1 回を、適用せずに保留して人が決める単位（[RFC 0078](../rfcs/0078-review-proposals.md)）。

- 提案は `Transaction` と同じ形（名前、`base_version`、操作）で、`project.mmdb` の `proposals` の表に置く。受け付けるとき今のデータのコピーに適用して検証し（ロック、`validate`、衝突）、触る所を記録する。保留中はプロジェクトのデータもバージョンも変えない。
- 承認は、提案の名前・発行元・対応するコメントのまま `base_version` を元にした普通のトランザクションとして適用し（`Store::approve_proposal`）、取り消しとやり直しがそのまま効く。操作の番号を選べば一部だけ承認する。後の編集と触る所が重なれば `Conflict` で断り、Claude が今のデータで作り直す。
- 却下はデータを変えず、保留に戻せる（`reopen`）。決めるのは人の操作経路だけで、MCP の経路からは拒否する。
- プレビュー: 提案を今のデータのコピーに適用したフレームを、Program（アプリ）と `GET /frame?proposal=<id>` で見せる。書かない。

## 同時書き込み

- 1 本の列で順に適用し、プロジェクトのデータのバージョン番号を進め、変わった所を開いているすべての画面に送る。
- 古いバージョンを元にした操作（`base_version` が今のバージョンより前）は、その後に適用した操作の `touches` と重ならなければ適用し、重なれば `Conflict` で拒否する（呼ぶ側が今のプロジェクトのデータで作り直す）。今のバージョンより先を元にしていれば `FutureBase`。CRDT は使わない。

## HTTP サーバー

実装は `crates/mm-serve`。アプリが書き手として `project.mmdb` を開き、`mm_serve::Core`（`Store` を `Mutex` で直列に扱う）を作り、`Server` で公開する。IPC の `apply_op` も同じ `Core::apply`（`Store::apply_txn`）を通るので、どの経路で書いても `project.mmdb` に残り、変わった所（`touched`）が `Core` の通知から再生スレッド（`invalidate_touched` と描き直し）、WebView（Tauri のイベント `mm:doc-changed`）、`GET /events` の購読者に届く。

- 待つ場所は 127.0.0.1 の空きポートだけ。ポートとトークンの置き場所の識別子（`token_id`）は `.mm/lock.info` に書く。起動ごとのトークン（32 バイトの乱数の 16 進）はプロジェクトの外の持ち主専用のフォルダーに置く（Windows は `%LOCALAPPDATA%movie-maker	okens<識別子>`、フォルダーごと現在のユーザーだけの ACL。Unix は `$XDG_RUNTIME_DIR` か `~/.local/state` の同じ構成で 0700/0600）。同期のフォルダーや別アカウントの読み取りに出ない。書き手が終了するとポートとトークンが消え、クラッシュで残っても、次に書き手になったプロセスが消す。クライアントは書き手のロックが生きていることを確かめてから、残った情報を信じる。
- 全部の要求で `Host`（`127.0.0.1:<port>`、`localhost:<port>`）を確かめる（DNS の再束縛を止める）。`Origin` があれば同じ待ち受けのオリジンだけを通す（別のページからの要求を止める）。トークンは `Authorization: Bearer <トークン>`。順に 403、403、401。CORS のヘッダーは返さない。
- HTTP から名乗れる操作経路は `cli`（既定）と `mcp` だけで、ロックの付いた所に触る操作は拒否する。`webview` と `surface` は名乗れない。`actor.kind` は操作経路で決まり、名乗れるのは `actor.name` だけ（`kind` を渡すと 400）。

| パス | 中身 |
|---|---|
| `POST /ops` | `{"op": {...}, "name": "...", "base_version": 3}` か `{"txn": {"name": "...", "base_version": 3, "ops": [...]}}`。任意で `entry`（`cli` か `mcp`）、`actor`（`{"name"}`）、`responds_to`。応答は `{"version","txn_id","kind","name","actor","touched":[...]}` |
| `POST /undo`、`POST /redo` | 本体は空か `{"entry"}`。応答は `{"change": {...}}`、無ければ `{"change": null}` |
| `GET /project` | `{"version","undo_depth","project": {...}}`（素材のパスはフォルダーからの相対のまま） |
| `POST /transcripts` | `{"asset","content_hash","producer","language","words":[{"segment","start","end","text"}]}`。素材の文字起こしを DB の表に入れる（前の本文は置き換える）。応答は `{"words": 件数}`。文字起こしは操作ではなく表なので `/ops` に載らない。`mm ingest` がアプリの動作中に使う |
| `GET /frame?t=3/2&comp=main&width=640` | 1 フレームの PNG（`image/png`）。`t` は秒（`1.5`、`3/2`）。専用のスレッドの `Engine` で描くので再生を止めない。`proposal=<id>`（と `ops=0,2`）で提案を適用したフレーム |
| `GET /events` | SSE。最初に `hello`（`{"version"}`）、書くたびに `change`（`POST /ops` の応答と同じ形）、取りこぼしたら `resync`（`GET /project` で読み直す）。提案の追加と決定で `proposal`（`{"id","state"}`） |
| `POST /proposals`、`GET /proposals`、`GET /proposals/{id}` | 提案を出す（本体は `POST /ops` と同じ）、一覧（`{"proposals":[...]}`、新しい順）、1 つ。形は `ui/sdk` の `Proposal`（だれが、操作、触る範囲 `scope`、後の編集と重なる `conflict`、`applied`、`undoable`） |
| `POST /proposals/{id}/approve`、`/reject`、`/reopen` | 承認（任意で `{"ops":[0,2]}`。応答は `{"proposal","change"}`）、却下、却下を保留に戻す。`entry: "mcp"` は 403（`forbidden`） |
| `POST /mcp` | MCP（Streamable HTTP、ステートレス、JSON 応答）。JSON-RPC の `initialize`、`ping`、`tools/list`、`tools/call`。通知は 202、バッチは拒否 |

失敗は `{"error": {"code","message", ...}}`。`conflict`（409、`touches` を添える）、`future_base`（409）、`locked`（403）、`invalid`（422、検証の失敗）、`bad_request`（400）、`unavailable`（503、閉じた後や描画が無いとき）。

MCP のツールは、操作ごとに 1 つ（名前は操作名。`inputSchema` は `ops_schema()` の各要素から `op` を除き、任意の `label`、`base_version`、`responds_to` を足して作る）と、`transaction`（操作の列を 1 つの取り消しの単位にする。コメントへの対応 1 回分）、`propose`（引数は `transaction` と同じで、適用せずに提案として保留する）、`undo`、`redo`、`project`、`proposals`（提案の一覧）、`frame`（PNG を画像で返す。`proposal` と `ops` で提案を適用したフレーム）。提案の承認・却下のツールは置かない。呼び出し元は常に `Entry::Mcp`、`actor` は `mcp`/`claude`。`responds_to` は記録の `txns` に残る。失敗は `isError` で、HTTP の失敗と同じ `{"error": ...}` を text に入れる。

CLI は `Store::open` で書き手になれなければ、`.mm/` のポートとトークンで `mm op`、`mm undo`、`mm redo` を転送する（応答の `touched` を出す）。`mm frame <project> <t> -o x.png` も、動いているアプリがあればそこで描いてもらい、無ければ自分で描く。

## ロック

- `Layer.lock` の付いたレイヤー（とその効果・マスク）に `touches` が触る操作は、Claude の操作経路（`Entry::Cli`、`Entry::Mcp`）からは `Locked` で拒否する。ロックを外すのも拒否する。操作経路の種類は `OpContext` で渡す。

## 記録と差分

- `project.mmdb` の `txns`（誰が、いつ、どのコメントへの対応として、どのトランザクションの種類で）と `ops`（何を、逆操作、`touches`）の表。バージョンの比較は `mm diff` がこの記録から意味の差分を出し、取り消しと元のトランザクションは対にして集約する（[保存の形](09-ingest-storage.md#mm-diff)）。
- マクロ: 操作の列を作る手順。vlog の編集の好みは、マクロとテンプレートの組（パック）として持つ。
