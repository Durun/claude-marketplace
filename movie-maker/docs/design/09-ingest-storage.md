# 取り込みと保存

## 取り込み

- 取り込みも命令で文書に素材を足す。素材ごとに中身のハッシュ、プレビュー動画、波形、サムネイル、文字起こし、PTS の索引、色空間のタグを作る。波形と文字起こしが先にでき、プレビュー動画は後から足される。文字起こしと PTS の索引と色空間のタグは文書（`assets/<id>.json`、`transcripts/`）に、プレビュー動画と波形とサムネイルは `cache/` に置く。
- 取り込みの処理は、決まった形の付属ファイル（版と JSON Schema を持つ JSON）を書く外の道具として作り、言語を問わない。文字起こし（faster-whisper）は Python のままでよい。
- 文字起こしの単語の ID は「素材の中身のハッシュ + 開始の時刻」の決定的な値にする。
- 同じ時間に撮った複数の素材（VR の別撮りと画面収録）は、取り込みで組にして頭をそろえる。

## 保存の形

```
<project>/
  project.json        索引と設定（スキーマの版、素材とコンポジションの一覧、レンダーターゲット、colorConfig）
  comps/<id>.json     コンポジションごと
  assets/<id>.json    素材ごと
  transcripts/<assetId>.json
  review.jsonl        コメント
  ops.jsonl           命令の記録（逆命令つき）。スナップショットごとにローテーション
  snapshots/<n>/      文書のスナップショット
  .index/             SQLite（rusqlite、FTS5）。派生物なので消してよい
  cache/              消してよい。作り直せるものだけ（下）
    preview/<contentHash>/   プレビュー動画（540p.mp4、要れば 1080p.mp4）
    audio/<contentHash>/     トラックごとの PCM、波形
    thumbs/<contentHash>/    サムネイル
    frames/<compId>/         コマのキャッシュ（短い GOP の H.264）
    overlays/<key>/          文字の絵（web.cef の描いた結果）
    shaders/                 コンパイル済みのシェーダー
```

- 正は文書。`ops.jsonl` は記録で、取り消し・履歴・復旧に使う。変わったファイルだけ書き、書き込みはアトミック（一時ファイル → fsync → rename）。
- 復旧は「最後のスナップショット + その後の `ops.jsonl` の再生」。
- git には文書と `review.jsonl` を入れ、`ops.jsonl`、`snapshots/`、`.index/`、`cache/` は外す（プロジェクトを作るときに `.gitignore` を書く）。
- スキーマの移行は 1 版ずつの純関数。動画オブジェクトの種類も `migrate` を持つ。文書の JSON の形は言語に依存しないので、モバイル版や durun-dashboard からも読める。

