# 取り込みと保存

## 取り込み

- 取り込みも操作でプロジェクトのデータに素材を足す。素材ごとにコンテンツハッシュ、プレビュー動画、波形、サムネイル、文字起こし、PTS の索引、色空間のタグを作る。波形と文字起こしが先にでき、プレビュー動画は後から足される。PTS の索引と色空間のタグはプロジェクトのデータ（`assets` の表）に、文字起こしの本文は `project.mmdb` の `transcripts`・`words` の表（プロジェクトのデータの外。操作の記録に載せない）に、プレビュー動画と波形とサムネイルは `cache/` に置く。文字起こしへの人の修正はプロジェクトのデータの `transcript_edits`（[データの形](03-data-model.md#素材の整理と文字起こし)）。
- 取り込みの処理は、決まった形の付属ファイル（バージョンと JSON Schema を持つ JSON）を書く外部のツールとして作り、言語を問わない。文字起こし（faster-whisper）は Python のままでよい。付属ファイルは取り込みの操作が読んで DB に入れる。入れた後の付属ファイルは `cache/ingest/` に残し、消してよい。
- `mm ingest <dir> <素材>...`（`mm-ingest`）が素材ごとにコンテンツハッシュ（BLAKE3）を取り、プレビュー動画（高さ 540、NVENC か x264）、波形、サムネイルを並行に作って `cache/` に置き、`addAsset` をまとめて 1 つのトランザクションで適用する。出力はハッシュのフォルダーにあれば作らずにスキップし、同じ素材の 2 回目は何も足さない。ffmpeg と ffprobe は CLI を呼ぶ（`MM_FFMPEG`、`MM_FFPROBE` で場所を変えられる）。外部のツールの付属ファイルは [RFC 0008](../rfcs/0008-ingest-sidecar.md)。
- 文字起こしの単語の ID は「素材のコンテンツハッシュ + 開始の時刻」の決定的な値にする（`mm_store::word_id`）。
- `mm ingest` は `cache/ingest/<hash>.transcript.json` があれば、素材を足すトランザクションの後に本文を `words` に入れ直す（`Store::put_transcript`。同じ素材の 2 回目も入れ直し、単語の ID は変わらない）。単語の時刻の無い区間は区間を 1 つの単語にする。
- `mm search <dir> <語>` は語から素材と時刻を引く（`Store::search_words`。`--json` で 1 行に 1 件、時刻は有理数）。単語の境界をまたぐ語（「編」「集」の「編集」）も引け、消した単語は外し、直した文字で引く。語の長さは 32 文字まで。
- 同じ時間に撮った複数の素材（VR の別撮りと画面収録）は、取り込みで組にして先頭をそろえる。

## 保存の形

作業中の正本は SQLite の 1 ファイル（`project.mmdb`）。バージョン管理のために、決まった形の JSON を `versions/` に書き出す。

```
<project>/
  project.mmdb        正本。SQLite（WAL。横に project.mmdb-wal と -shm ができる）
  versions/           名前付きバージョンの JSON の書き出し（mm export）。git に入れる
    project.json      索引と設定（スキーマのバージョン、素材とコンポジションの一覧、レンダーターゲット、colorConfig）
    comps/<id>.json   コンポジションごと（レイヤーを含む）
    assets/<id>.json  素材ごと
    sequences/<id>.json  sequence ごと
    transcripts/<assetId>.json   文字起こしの本文（まだ書かない。取り込みの付属ファイルから作り直せる）
    bins.json         素材の整理（イン点・アウト点、選別、話題）と文字起こしの修正
    review.json       コメント
  backups/            自動のバックアップ（project.<時刻>.mmdb）。消してよい
  media/              プロジェクトの中にコピーした素材（設定で選ぶ。既定は元の場所を参照）
  .mm/                動作中を示すファイル（lock は書き手の OS のファイルロック、lock.info に pid と HTTP のポート）。消してよい
  cache/              消してよい。作り直せるものだけ（下）
    preview/<contentHash>/   プレビュー動画（540p.mp4、要れば 1080p.mp4）
    audio/<contentHash>/     トラックごとの PCM、波形
    thumbs/<contentHash>/    サムネイル
    frames/<compId>/         フレームのキャッシュ（短い GOP の H.264）
    overlays/<key>/          文字の画像（web.cef の描いた結果）
    shaders/                 コンパイル済みのシェーダー
    ingest/                  取り込みの付属ファイル（DB に入れた後の控え）
    telemetry/               プロジェクトに固有の計測の数（[計装](17-instrumentation.md#置き場所と大きさ)）
  .gitignore          project.mmdb*、backups/、media/、.mm/、cache/ を外す
```

### なぜ DB を正本にするか

- プロジェクトのデータと操作の記録を 1 つのトランザクションで書ける。複数のファイルに分けると、途中でクラッシュしたとき片方だけ書かれる。SQLite は変えたページだけを書き、電源断でもトランザクションの途中で止まらない [71]。
- 細かい更新に強い。操作 1 つで変わるのはレイヤー 1 行で、ファイル全体を書き直さない。10 KB 前後の塊の読み書きはファイルシステムより速い（公式の測定で約 35%。Windows ではさらに差が開く）[72]。
- 文字起こしの検索（10 万語を言葉で引く）は FTS5 で持てる [76]。
- 競合は保存の形では解けない。サーバーが操作を 1 本の列で適用し、`touches` で判定する（[操作の層](05-ops.md)）。書くのは 1 プロセスだけなので、SQLite の「書き手は同時に 1 つ」で足りる。
- 引き換え: git の差分は `versions/` に書き出した JSON でだけ読める。バージョンどうしの比較は、操作の記録から意味の差分を出す `mm diff` を主にする。クラウド同期のフォルダーには置かない（下）。
- 先行事例: FCP の `.fcpbundle` の中のライブラリの DB（15 分ごとに `Backups/` へコピーする）、Resolve の disk database、Lightroom の `.lrcat`（SQLite、WAL）[81]。SQLite 公式の「アプリのファイル形式としての SQLite」[71]。

### 表

| 表 | 内容 | キー |
|---|---|---|
| `meta` | プロジェクトの ID、作った日時、プロジェクトのデータのバージョン番号（`doc_version`。操作を適用するたびに進み、取り消しとやり直しも操作の数だけ進める）、取り消しの位置（`undo_pos`） | `key` |
| `project` | 1 行。プロジェクトのデータのスキーマのバージョン（`schema_version`）の JSON。設定（`colorConfig`、`bins`）はプロジェクトのデータに入った段で足す | |
| `sequences` | sequence ごとに 1 行。JSON | `id` |
| `render_targets` | レンダーターゲットごとに 1 行。JSON | `id` |
| `assets` | 素材ごとに 1 行。JSON（原本のパス、色空間、時間軸、PTS の索引）と、検索用に抜いた `content_hash`、`path` | `id` |
| `compositions` | コンポジションごとに 1 行。大きさ、fps、長さ、背景、マーカーの JSON | `id` |
| `layers` | レイヤーごとに 1 行。`comp_id`、重なりの順 `z`、レイヤーの JSON（`source` のノード、時間、変形、効果、音、ロック） | `(comp_id, id)` |
| `ops` | 操作の記録。通し番号、トランザクションの ID、操作の名前、引数、逆操作、`touches`、適用した後の `doc_version`（操作ごとに 1 進む）。取り消しの行は逆操作を適用した記録、やり直しの行は元の操作を再適用した記録 | `seq` |
| `txns` | トランザクション（`mm-ops` の `TxnRecord`）ごとに 1 行。ID（1 からの通し番号）、トランザクションの種類（`normal` / `undo` / `redo`）と元のトランザクション（`of_txn`）、名前（「ドラッグ」「Claude の対応」）、誰が、いつ、どのコメントへの対応か、適用する前と後の `doc_version`、始めと終わりの `seq`。適用した行は後から書き換えない。`mm diff` は `doc_version` が範囲に入る行を読み、取り消しと元のトランザクションを対にして集約する | `id` |
| `undo_stack` | 取り消しの列。通常のトランザクションの ID の並びと、今の位置（`meta.undo_pos`）。取り消しで位置が戻り、やり直しで進み、新しい操作で先が破棄される | `pos` |
| `versions` | 名前付きバージョン。名前、日時、誰が、そのときの `doc_version` と `ops.seq`、書き出した JSON のハッシュ、git のコミット（あれば） | `id` |
| `comments` | レビューのコメント。時刻か範囲、描き込み、返信、状態（`open` → `fresh` → `done`、`wontfix`）の JSON と、検索用の `state`、`comp_id` | `id` |
| `proposals` | Claude の編集の提案（保留中のトランザクション）。名前、操作経路、発行元、対応するコメント、`base_version`、操作の JSON、`touches`、状態（`pending` / `approved` / `rejected`）、決めた者と時刻、承認で適用した `txn_id`、承認した操作の番号（[操作の「提案」](05-ops.md#提案)） | `id` |
| `search` | 段階 1 の検索。FTS5 の仮想表（`text`、`kind`、`owner`）。レイヤーの ID とノードの種類と文字のパラメータ、マーカーの名前、コンポジションの ID、素材のパスを、プロジェクトのデータの行を書くたびに同じトランザクションで入れ替える。トークナイザーは `trigram`（3 文字未満の語は部分一致で探す） | |
| `marks`、`selections`、`topics`、`word_edits` | プロジェクトのデータの `bins` と `transcript_edits` を要素ごとに 1 行（`(id, json)`）。`marks` の ID は素材の ID、`word_edits` は `<素材>/<単語>`。話題の見出しと選別・話題のタグは `search` にも入れる | `id` |
| `transcripts` | 素材ごとの文字起こしの元（コンテンツハッシュ、ツールの名前とバージョン、言語）。素材を外しても消さない（取り消しで戻るため） | `asset_id` |
| `words` | 単語ごとに 1 行。`asset_id`、`word_id`（ハッシュ + 開始の時刻）、素材の中の順 `ord`、区間の番号、始めと終わりの時刻（有理数の分子と分母）、取り込んだ文字、修正（`fixed`、`deleted`。`transcript_edits` を操作の書き込みと同じトランザクションで反映する）、`run`（表示する文字に後ろの単語の文字を 32 文字まで続けたもの。消した単語は空） | `id`（整数）、`(asset_id, word_id)` |
| `words_fts` | `words.run` を引く FTS5 の仮想表。外部コンテンツ（`content=words`）でトリガーで同期。トークナイザーは `trigram case_sensitive 1`（日本語は空白で切れない）[76]。3 文字以上は `MATCH`、未満は `instr` で引き、語の始まりがその単語の文字の中にある行だけを採る | |

- プロジェクトのデータは JSON の列で持ち、SQLite の JSON 関数で中を引く（3.38 から組み込み [77]）。検索や結合に使う値（`content_hash`、`state`、`comp_id`）だけ列に抜く。
- バージョン番号 `doc_version` は 1 つ。古いバージョンを元にした操作は、元にした `doc_version` より後の `ops`（取り消しとやり直しの行も）の `touches` と比べて適用するか拒否するかを決める。
- スキーマのバージョンは `PRAGMA user_version`（SQLite 自身は使わず、アプリが自由に使える整数 [74]）。`PRAGMA application_id` に movie-maker の識別値を入れ、`.mmdb` の見分けに使う。
- ノードの種類ごとのバージョンは、レイヤーの JSON の `source.version` に持つ（[ノード](04-nodes.md)の `migrate`）。

### 書き方

| こと | 形 |
|---|---|
| ジャーナル | WAL。読みと書きが互いを待たない。設定は DB に残る [73][74] |
| 同期 | `synchronous=NORMAL`。WAL では一貫性を保ち、電源断で最後の数トランザクションが戻ることだけを許す [74]。バージョンの書き出し、バックアップ、閉じるときは `wal_checkpoint(TRUNCATE)` で WAL を本体に戻す |
| 書き手 | アプリ（か、アプリが動いていないときの `mm`）の 1 プロセスだけ。開くときに `.mm/lock` に OS のファイルロックをかけ（クラッシュしたプロセスのロックは OS が解く）、pid と HTTP のポートを `.mm/lock.info` に書く。`mm` は `lock.info` を見てアプリに操作を送る。ロックを取れなかったプロセスは読み取りだけで開き、書き込みは `mm-store` が拒否する。読むだけのツール（`sqlite3`、DB Browser）は締め出さない |
| トランザクションの単位 | 操作 1 つ。プロジェクトのデータの行、`ops`、`txns`、`undo_stack`、`meta.doc_version` を 1 つのトランザクションで書く。ドラッグ、マクロ、Claude の対応 1 回は、複数の操作を 1 つのトランザクションにまとめる（ドラッグ中はメモリの上で適用して画面に出し、離したときに書く）。取り消しとやり直しもトランザクションの単位で、同じ形の行として書く |
| 外部キー | `foreign_keys=ON`（rusqlite の bundled の既定 [78]）。レイヤーはコンポジションに従う |
| 予算 | 操作 1 つの書き込みは 20 ms 以下（[計装の予算の表](17-instrumentation.md#予算の表)） |

### バックアップと復旧

- 自動のバックアップ: 編集中は 15 分ごと（FCP と同じ）、アプリを閉じるとき、スキーマの移行の前に、`VACUUM INTO 'backups/project.<UTC の時刻 YYYYMMDDThhmmssmmmZ>.mmdb'` で一貫したコピーを作る（`synchronous` が NORMAL 以上なら書いた後に fsync される [75]）。`.tmp` に書いてから名前を変え、途中でクラッシュしても壊れたコピーが最新として残らない。直近 10 個と、日ごとの 1 個を 30 日〔推〕残す。1 操作ごとに開き直す `mm` は閉じるたびにはバックアップせず、前回のバックアップ（`backups/` の新しいファイル）から間隔が過ぎたときだけバックアップする。本体が 1 GB を超えるプロジェクトはオンラインバックアップ API で小分けにコピーする（同じ接続からの書き込みはコピーに追いつく [75]）。
- クラッシュからの戻り方: SQLite がトランザクションの途中を WAL から巻き戻す。開くときに `PRAGMA quick_check` を通し、通らなければ `backups/` の新しいものと `versions/` からの `mm import --overwrite` を選ばせる。`mm import --overwrite` は壊れた DB のコピーを作れなければ、生のファイル（`-wal`、`-shm` も）を `backups/project.<時刻>.broken.mmdb` に移してから作り直す。書き終えていなかったドラッグ（1 つのトランザクション）は消える。それでよい。
- `project.mmdb` をコピーするときは `-wal` と `-shm` も一緒にコピーする。別々にコピーすると確定した変更を失う [73]。UI の「別名で保存」は `VACUUM INTO` で 1 ファイルにする。
- 操作の記録は切り詰めずに全部残す。どの操作からでも取り消しと意味の差分を出せるようにするため。大きさは下の見積もりのとおりで、`Media` に DB の大きさを出す。

### スキーマの移行

- `user_version` を 1 つずつ上げる純関数の列。DDL の変更（表と列）は `mm-store`、JSON の中の形の変更は `mm-doc` が持ち、同じ番号で対にする。1 バージョンずつ順に適用し、全体を 1 つのトランザクションで囲む。失敗したら戻り、移行の前のバックアップが残る。
- 新しいバージョンで開いた DB は、古いアプリで開けない（`user_version` が大きければ拒否する）。古いバージョンの DB は、どのバージョンからでも今のバージョンまで上がる。フィクスチャは `fixtures/doc/v1/` …（[テスト](11-testing.md#層の一覧)）。
- `versions/` の JSON にもスキーマのバージョン（`project.json` の `schemaVersion`）があり、`mm import` が同じ移行を JSON の側で適用してから DB に入れる。

### 大きさの見積もり

レビューの数（1 回の収録 11 時間、プレビュー動画は 1 時間 2.6 GB）を元にする。

| もの | 見積もり〔推〕 |
|---|---|
| プロジェクトのデータ（`project`、`assets`、`compositions`、`layers`） | 本編とショート 10 本で、レイヤー 2000 行、1 行 2 KB → 約 4 MB |
| 文字起こし（`words` + FTS5 の trigram） | 11 時間で約 10 万語。1 語 100 B と trigram の索引で 3 倍 → 約 40 MB |
| 操作の記録（`ops`） | 1 回の編集で 2 万操作、1 行 1 KB（逆操作込み）→ 約 20 MB。全部残す。編集を重ねても数百 MB に収まる見込み〔推〕 |
| コメント | 数百件、数 KB ずつ → 1 MB 以下 |
| 合計 | 1 回で 100 MB 以下。`VACUUM INTO` のコピーは 1 秒前後 |
| `cache/` | 11 時間で 30 GB（プレビュー動画）。上限は `cache/` の設定 |

### JSON の書き出し（`mm export`）

- 形は上の木のとおり。`project.json` が索引と設定、`comps/<id>.json` がコンポジション（レイヤーを `z` の順に含む）、`assets/<id>.json`、`sequences/<id>.json`、`bins.json`、`review.json`（`transcripts/<assetId>.json` は後で足す）。ファイル名の ID は英数字と `-_.` 以外を `%XX` にする。
- 決定的に書く。キーは辞書順（入れ子も。構造体の順にすると、キーが変わるバージョンで差分が散る）、配列は ID か時刻の順、時刻は有理数のまま（`{"num":…,"den":…}`）、時刻に浮動小数は使わない（パラメータの数は最短で往復する表記）、インデントは 2、改行は LF、末尾に改行 1 つ。同じプロジェクトのデータからは同じバイト列になり、`versions` のハッシュと git の差分がそれで決まる。
- ファイルの分け方は git の差分の読みやすさで決める。コンポジションごと、素材ごとに分け、1 行に 1 つの値が来るようにする。文字起こしは単語ごとに 1 行。
- `mm export` は固定の場所（`versions/`）に上書きし、git のコミットがバージョンになる。名前を付けるとき（`mm export --name v3`）は `versions` の表に行を足し、git があれば `git tag` も打つ。
- `mm import <dir> <versions/>` は JSON から DB を作り直す（`<dir>` に `project.mmdb` がある場合は `--overwrite` で、コピーを `backups/` に残して置き換える）。1 つにまとまった `project.json`（段階 0 のフィクスチャの形）も渡せ、素材の相対パスは絶対パスにして入れる。プロジェクトのデータ、コメント、文字起こしが入る。操作の記録は入らない（JSON には無い）ので `ops` は空から始まり、`versions` に「ここから import」の行が付く。export → import → export が同じバイト列になることをテストで守る（[テスト](11-testing.md#層の一覧)）。
- Claude とモバイル版と durun-dashboard は、`mm` の CLI か MCP で DB を読む。`versions/` の JSON は直接読める。

### `mm diff`

- `mm diff <dir> <バージョン A> <バージョン B>`（バージョンは番号、`head`、名前付きバージョンの名前）は、2 つのバージョンの間の `txns` と `ops` を読み、意味の差分（「C12 のアウト点を 0.4 秒詰めた」「レイヤー L3 を外した」）を出す。同じレイヤーへの連続した `setParam` は 1 行に集約する。範囲の中で取り消された（やり直されていない）トランザクションは、その取り消しと対で消す。範囲の前に適用したトランザクションの取り消しだけが範囲に入るときは、逆操作として出る。
- 段階 1 の実装は文だけ出す。`--json` で機械が読む形、`--files` で `versions/` の JSON のファイルの差分（`git diff` と同じ）は後で足す。
- バージョンの名前が無い所は `doc_version` の番号で指す。`@{2026-11-05 14:00}` の時刻の指定は `Store::doc_version_at` があり、CLI へは後で足す。
- `mm log <dir>` は取り消しとやり直しのトランザクションも、適用したバージョンと種類（適用 / 取消 / 再適用）を添えて出す。`mm export --name` の `git tag` は CLI がまだ打たない。
- `mm log`、`mm diff`、`mm export`（名前を付けないとき）、`mm render` は書き手のロックを取らず、スキーマの移行もしないで読む（`Store::open_read_only`）。アプリが編集している最中でも邪魔をしない。

### 同期のフォルダーの検出

- OneDrive、Dropbox、Google Drive、iCloud の同期のフォルダーに置くと、同期のツールが DB のロックを守らずにファイルを書き換え、壊れることがある（Zotero が同じ理由で禁じている [80]）。
- プロジェクトを作るときと開くときに、パスが同期のフォルダーの下か（Windows は `%OneDrive%`、`%USERPROFILE%/Dropbox`、Google Drive の既定の場所）か、UNC のパス（ネットワーク）かを、パスの部品の名前で見て、該当すれば警告して別の場所を勧める。開くのは拒否しない。それでも置くなら、バックアップの間隔を 5 分に縮め、閉じるたびに `versions/` を書き出す。
- ネットワークのファイルシステム（SMB、NFS）も同じ扱い。WAL は同じホストの中でしか動かない [73]。

### Rust のクレート

`rusqlite` を使う。

| 候補 | 評価 |
|---|---|
| `rusqlite`（0.41.1、SQLite 3.45.3 以上）[78] | 同期の API で、操作の層（1 本の列で順に適用する）と形が合う。`bundled` で SQLite を同梱し、FTS5、JSON1、RTREE が入り、外部キーが既定で ON。`backup`（オンラインバックアップ API）、`hooks`（更新の通知）、`serde_json` の feature がある。採る |
| `sqlx`（0.9.0）[79] | 非同期が前提。コンパイル時の問い合わせの検査は利点だが、ビルドに `DATABASE_URL` が要る。`sqlite` を入れると `forbid(unsafe_code)` が外れる。操作の層は同期で足りるので採らない |
| `diesel`、`sea-orm` | ORM。プロジェクトのデータは JSON の列で持つので、型の対応は要らない。採らない |

- 接続はサーバーの中の 1 本（書き手）と、読み取り専用の接続のプール（画面の問い合わせ、`words_fts` の検索）。WAL なので読みは書きを待たない [73]。
- `rusqlite` の `hooks`（`update_hook`）で変わった行を拾い、開いているすべての画面に送る（[操作の層](05-ops.md)の「変わった所を送る」）。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
