# 取り込みと保存

## 取り込み

- 取り込みも命令で文書に素材を足す。素材ごとに中身のハッシュ、プレビュー動画、波形、サムネイル、文字起こし、PTS の索引、色空間のタグを作る。波形と文字起こしが先にでき、プレビュー動画は後から足される。文字起こしと PTS の索引と色空間のタグは文書（`project.mmdb` の `assets`、`transcripts`、`words` の表）に、プレビュー動画と波形とサムネイルは `cache/` に置く。
- 取り込みの処理は、決まった形の付属ファイル（版と JSON Schema を持つ JSON）を書く外の道具として作り、言語を問わない。文字起こし（faster-whisper）は Python のままでよい。付属ファイルは取り込みの命令が読んで DB に入れる。入れた後の付属ファイルは `cache/ingest/` に残し、消してよい。
- 文字起こしの単語の ID は「素材の中身のハッシュ + 開始の時刻」の決定的な値にする。
- 同じ時間に撮った複数の素材（VR の別撮りと画面収録）は、取り込みで組にして頭をそろえる。

## 保存の形

作業中の正本は SQLite の 1 ファイル（`project.mmdb`）。版管理のために、決まった形の JSON を `versions/` に書き出す。

```
<project>/
  project.mmdb        正本。SQLite（WAL。横に project.mmdb-wal と -shm ができる）
  versions/           名前付きの版の JSON の書き出し（mm export）。git に入れる
    project.json      索引と設定（スキーマの版、素材とコンポジションの一覧、レンダーターゲット、colorConfig）
    comps/<id>.json   コンポジションごと（レイヤーを含む）
    assets/<id>.json  素材ごと
    transcripts/<assetId>.json
    review.json       コメント
  backups/            自動のバックアップ（project.<時刻>.mmdb）。消してよい
  media/              プロジェクトの中に写した素材（設定で選ぶ。既定は元の場所を参照）
  .mm/                動作中の印（lock に pid と HTTP のポート）。消してよい
  cache/              消してよい。作り直せるものだけ（下）
    preview/<contentHash>/   プレビュー動画（540p.mp4、要れば 1080p.mp4）
    audio/<contentHash>/     トラックごとの PCM、波形
    thumbs/<contentHash>/    サムネイル
    frames/<compId>/         コマのキャッシュ（短い GOP の H.264）
    overlays/<key>/          文字の絵（web.cef の描いた結果）
    shaders/                 コンパイル済みのシェーダー
    ingest/                  取り込みの付属ファイル（DB に入れた後の控え）
    telemetry/               プロジェクトに固有の計測の数（[計装](17-instrumentation.md#置き場所と大きさ)）
  .gitignore          project.mmdb*、backups/、media/、.mm/、cache/ を外す
```

### なぜ DB を正本にするか

- 文書と命令の記録を 1 つのトランザクションで書ける。複数のファイルに分けると、途中で落ちたとき片方だけ書かれる。SQLite は変えたページだけを書き、電源断でもトランザクションの途中で止まらない [71]。
- 細かい更新に強い。命令 1 つで変わるのはレイヤー 1 行で、ファイル全体を書き直さない。10 KB 前後の塊の読み書きはファイルシステムより速い（公式の測定で約 35%。Windows ではさらに差が開く）[72]。
- 文字起こしの検索（10 万語を言葉で引く）は FTS5 で持てる [76]。
- 競合は保存の形では解けない。サーバーが命令を 1 本の列で当て、`touches` で判定する（[命令の層](05-ops.md)）。書くのは 1 プロセスだけなので、SQLite の「書き手は同時に 1 つ」で足りる。
- 引き換え: git の差分は `versions/` に書き出した JSON でだけ読める。版どうしの比較は、命令の記録から意味の差分を出す `mm diff` を主にする。クラウド同期のフォルダーには置かない（下）。
- 先行事例: FCP の `.fcpbundle` の中のライブラリの DB（15 分ごとに `Backups/` へ写す）、Resolve の disk database、Lightroom の `.lrcat`（SQLite、WAL）[81]。SQLite 公式の「アプリのファイル形式としての SQLite」[71]。

### 表

| 表 | 中身 | 鍵 |
|---|---|---|
| `meta` | プロジェクトの ID、作った日時、文書の版番号（`doc_version`。命令を当てるたびに進む）、取り消しの位置 | `key` |
| `project` | 1 行。設定（レンダーターゲット、`colorConfig`、`bins`）の JSON | |
| `assets` | 素材ごとに 1 行。JSON（原本のパス、色空間、時間軸、PTS の索引）と、検索用に抜いた `content_hash`、`path` | `id` |
| `compositions` | コンポジションごとに 1 行。大きさ、fps、長さ、背景、マーカーの JSON | `id` |
| `layers` | レイヤーごとに 1 行。`comp_id`、重なりの順 `z`、レイヤーの JSON（`source` の動画オブジェクト、時間、変形、効果、音、ロック） | `(comp_id, id)` |
| `ops` | 命令の記録。通し番号、トランザクションの ID、誰が、いつ、命令の名前、引数、逆命令、`touches`、どのコメントへの対応か、当てた後の `doc_version` | `seq` |
| `txns` | トランザクションごとに 1 行。ID、始めと終わりの `seq`、名前（「ドラッグ」「Claude の対応」）、状態（`applied` / `undone`） | `id` |
| `undo_stack` | 取り消しの列。トランザクションの ID の並びと、今の位置（`meta`）。取り消しで位置が戻り、新しい命令で先が捨てられる | `pos` |
| `versions` | 名前付きの版。名前、日時、誰が、そのときの `doc_version` と `ops.seq`、書き出した JSON のハッシュ、git のコミット（あれば） | `id` |
| `comments` | レビューのコメント。時刻か範囲、描き込み、返信、状態（`open` → `fresh` → `done`、`wontfix`）の JSON と、検索用の `state`、`comp_id` | `id` |
| `transcripts` | 素材ごとの文字起こしの元（道具の名前と版、言語、区切り） | `asset_id` |
| `words` | 単語ごとに 1 行。`asset_id`、`word_id`（ハッシュ + 開始の時刻）、始めと終わりの時刻、文字、信頼度 | `(asset_id, word_id)` |
| `words_fts` | `words` の文字を引く FTS5 の仮想表。外部コンテンツ（`content=words`）でトリガーで同期。トークナイザーは `trigram`（日本語は空白で切れない）[76] | |

- 文書の部分は JSON の列で持ち、SQLite の JSON 関数で中を引く（3.38 から組み込み [77]）。検索や結合に使う値（`content_hash`、`state`、`comp_id`）だけ列に抜く。
- 版番号 `doc_version` は 1 つ。古い版を元にした命令は、元にした `doc_version` より後の `ops` の `touches` と比べて当てるか断るかを決める。
- スキーマの版は `PRAGMA user_version`（SQLite 自身は使わず、アプリが自由に使える整数 [74]）。`PRAGMA application_id` に movie-maker の印を入れ、`.mmdb` の見分けに使う。
- 動画オブジェクトの種類ごとの版は、レイヤーの JSON の `source.version` に持つ（[動画オブジェクト](04-nodes.md)の `migrate`）。

### 書き方

| こと | 形 |
|---|---|
| ジャーナル | WAL。読みと書きが互いを待たない。設定は DB に残る [73][74] |
| 同期 | `synchronous=NORMAL`。WAL では一貫性を保ち、電源断で最後の数トランザクションが戻ることだけを許す [74]。版の書き出し、バックアップ、閉じるときは `wal_checkpoint(TRUNCATE)` で WAL を本体に戻す |
| 書き手 | アプリ（か、アプリが動いていないときの `mm`）の 1 プロセスだけ。開くときに `.mm/lock`（pid、HTTP のポート）を置き、`mm` はそれを見てアプリに命令を送る。ほかのプロセスの書き込みは `mm-store` が断る。読むだけの道具（`sqlite3`、DB Browser）は締め出さない |
| トランザクションの単位 | 命令 1 つ。文書の行、`ops`、`txns`、`undo_stack`、`meta.doc_version` を 1 つのトランザクションで書く。ドラッグ、マクロ、Claude の対応 1 回は、複数の命令を 1 つのトランザクションにまとめる（ドラッグ中はメモリの上で当てて画面に出し、離したときに書く）。取り消しもまとまりの単位 |
| 外部キー | `foreign_keys=ON`（rusqlite の bundled の既定 [78]）。レイヤーはコンポジションに従う |
| 予算 | 命令 1 つの書き込みは 20 ms 以下（[計装の予算の表](17-instrumentation.md#予算の表)） |

### バックアップと復旧

- 自動のバックアップ: 編集中は 15 分ごと（FCP と同じ）、閉じるとき、スキーマの移行の前に、`VACUUM INTO 'backups/project.<時刻>.mmdb'` で一貫した写しを作る（`synchronous` が NORMAL 以上なら書いた後に fsync される [75]）。直近 10 個と、日ごとの 1 個を 30 日〔推〕残す。本体が 1 GB を超えるプロジェクトはオンラインバックアップ API で小分けに写す（同じ接続からの書き込みは写しに追いつく [75]）。
- クラッシュからの戻り方: SQLite がトランザクションの途中を WAL から巻き戻す。開くときに `PRAGMA quick_check` を通し、通らなければ `backups/` の新しいものと `versions/` からの `mm import` を選ばせる。書き終えていなかったドラッグ（1 つのトランザクション）は消える。それでよい。
- `project.mmdb` を写すときは `-wal` と `-shm` も一緒に写す。別々に写すと確定した変更を失う [73]。UI の「別名で保存」は `VACUUM INTO` で 1 ファイルにする。
- 命令の記録は切り詰めずに全部残す。どの命令からでも取り消しと意味の差分を出せるようにするため。大きさは下の見積もりのとおりで、`Media` に DB の大きさを出す。

### スキーマの移行

- `user_version` を 1 つずつ上げる純関数の列。DDL の変更（表と列）は `mm-store`、JSON の中の形の変更は `mm-doc` が持ち、同じ番号で対にする。1 版ずつ順に当て、全体を 1 つのトランザクションで囲む。失敗したら戻り、移行の前のバックアップが残る。
- 新しい版で開いた DB は、古いアプリで開けない（`user_version` が大きければ断る）。古い版の DB は、どの版からでも今の版まで上がる。見本は `fixtures/doc/v1/` …（[テスト](11-testing.md#層の一覧)）。
- `versions/` の JSON にもスキーマの版（`project.json` の `schemaVersion`）があり、`mm import` が同じ移行を JSON の側で当ててから DB に入れる。

### 大きさの見積もり

レビューの数（1 回の収録 11 時間、プレビュー動画は 1 時間 2.6 GB）を元にする。

| もの | 見積もり〔推〕 |
|---|---|
| 文書（`project`、`assets`、`compositions`、`layers`） | 本編とショート 10 本で、レイヤー 2000 行、1 行 2 KB → 約 4 MB |
| 文字起こし（`words` + FTS5 の trigram） | 11 時間で約 10 万語。1 語 100 B と trigram の索引で 3 倍 → 約 40 MB |
| 命令の記録（`ops`） | 1 回の編集で 2 万命令、1 行 1 KB（逆命令込み）→ 約 20 MB。全部残す。編集を重ねても数百 MB に収まる見込み〔推〕 |
| コメント | 数百件、数 KB ずつ → 1 MB 以下 |
| 合計 | 1 回で 100 MB 以下。`VACUUM INTO` の写しは 1 秒前後 |
| `cache/` | 11 時間で 30 GB（プレビュー動画）。上限は `cache/` の設定 |

### JSON の書き出し（`mm export`）

- 形は上の木のとおり。`project.json` が索引と設定、`comps/<id>.json` がコンポジション（レイヤーを `z` の順に含む）、`assets/<id>.json`、`transcripts/<assetId>.json`、`review.json`。
- 決定的に書く。キーはスキーマの順、配列は ID か時刻の順、時刻は有理数のまま（`{"num":…,"den":…}`）、浮動小数は書かない、インデントは 2、改行は LF、末尾に改行 1 つ。同じ文書からは同じバイト列になり、`versions` のハッシュと git の差分がそれで決まる。
- ファイルの分け方は git の差分の読みやすさで決める。コンポジションごと、素材ごとに分け、1 行に 1 つの値が来るようにする。文字起こしは単語ごとに 1 行。
- `mm export` は固定の場所（`versions/`）に上書きし、git のコミットが版になる。名前を付けるとき（`mm export --name v3`）は `versions` の表に行を足し、git があれば `git tag` も打つ。
- `mm import <versions/>` は JSON から DB を作り直す。文書、コメント、文字起こしが入る。命令の記録は入らない（JSON には無い）ので `ops` は空から始まり、`versions` に「ここから import」の行が付く。export → import → export が同じバイト列になることをテストで守る（[テスト](11-testing.md#層の一覧)）。
- Claude とモバイル版と durun-dashboard は、`mm` の CLI か MCP で DB を読む。`versions/` の JSON は直接読める。

### `mm diff`

- `mm diff <版 A> <版 B>` は、2 つの版の間の `ops` を読み、意味の差分（「C12 の尻を 0.4 秒詰めた」「レイヤー L3 を外した」）を出す。同じレイヤーへの連続した `setParam` は 1 行に畳む。取り消されたトランザクションは出さない。
- `--json` で機械が読む形、`--files` で `versions/` の JSON のファイルの差分（`git diff` と同じ）。
- 版の名前が無い所は `doc_version` の番号か、`@{2026-11-05 14:00}` の時刻で指す。

### 同期のフォルダーの検出

- OneDrive、Dropbox、Google Drive、iCloud の同期のフォルダーに置くと、同期の道具が DB の鍵を守らずにファイルを書き換え、壊れることがある（Zotero が同じ理由で禁じている [80]）。
- プロジェクトを作るときと開くときに、パスが同期のフォルダーの下か（Windows は `%OneDrive%`、`%USERPROFILE%/Dropbox`、Google Drive の既定の場所、ファイルの属性 `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`〔推。確かめる〕）を見て、当たれば警告して別の場所を勧める。開くのは断らない。それでも置くなら、バックアップの間隔を 5 分に縮め、閉じるたびに `versions/` を書き出す。
- ネットワークのファイルシステム（SMB、NFS）も同じ扱い。WAL は同じホストの中でしか動かない [73]。

### Rust のクレート

`rusqlite` を使う。

| 候補 | 評価 |
|---|---|
| `rusqlite`（0.41.1、SQLite 3.45.3 以上）[78] | 同期の API で、命令の層（1 本の列で順に当てる）と形が合う。`bundled` で SQLite を同梱し、FTS5、JSON1、RTREE が入り、外部キーが既定で ON。`backup`（オンラインバックアップ API）、`hooks`（更新の通知）、`serde_json` の feature がある。採る |
| `sqlx`（0.9.0）[79] | 非同期が前提。コンパイル時の問い合わせの検査は利点だが、ビルドに `DATABASE_URL` が要る。`sqlite` を入れると `forbid(unsafe_code)` が外れる。命令の層は同期で足りるので採らない |
| `diesel`、`sea-orm` | ORM。文書は JSON の列で持つので、型の対応は要らない。採らない |

- 接続はサーバーの中の 1 本（書き手）と、読み取り専用の接続のプール（画面の問い合わせ、`words_fts` の検索）。WAL なので読みは書きを待たない [73]。
- `rusqlite` の `hooks`（`update_hook`）で変わった行を拾い、開いているすべての画面に送る（[命令の層](05-ops.md)の「変わった所を送る」）。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
