# RFC 0008: 取り込みの付属ファイル（`mm-ingest-api`）

状態: 採用

## 変える理由

段階 2 の取り込み（`mm-ingest`、`tools/transcribe`）で、外部のツールが書く付属ファイルを実際に作った。
外部のツールは Rust の型を見られないので、型と同じ内容の JSON Schema が要る。単語ごとの時刻（`words` の表）とシーンの区切りの置き場も足りなかった。

先行事例は `mm-ops` の `schema/ops.json`（型から導出して凍結し、テストで最新かを確かめる）。同じ形にする。

## 形を変える（0.1.0 → 0.2.0）

- `TranscriptSegment` に `words: Vec<TranscriptWord>` を足す（`serde(default)` で、無ければ空。空なら書かない）。構造体のリテラルで作っているコードは `words` が要る（`mm-testkit` のフィクスチャを直した）。
- `SidecarBody` に `Scenes(Scenes)`（`kind = "scenes"`）を足す。バリアントを足すので、網羅した `match` は直す。
- 全部の型に `schemars::JsonSchema` を導出する（`mm-types` の feature `schema` を使う）。

どれも JSON の形としては後方互換（古い JSON は `words` 無しで読め、新しい種類は古い読み手が知らない `kind` として拒否する）で、Rust の型としては破壊的なので、0.x の決まりで minor を上げる。`SIDECAR_SCHEMA_VERSION` は 1 のまま（JSON の読み手にとっての破壊は無い）。

## 足すだけ

- `SCHEMA_PATH`、`sidecar_schema_json()`: 凍結したスキーマ（`schema/sidecar.schema.json`）の置き場所と、型から導出した文字列。テストが最新かを確かめ、`MM_UPDATE_SCHEMA=1 cargo test -p mm-ingest-api --test schema` で書き直す。
- `Scenes`、`SceneCut`、`TranscriptWord`。

## 外部のツールが書く形

- 置き場所: `<project>/cache/ingest/<コンテンツハッシュ>.<種類>.json`（種類は `transcript`、`waveform`、`preview_media`、`scenes`）。
- コンテンツハッシュ: 素材のバイト列の BLAKE3（小文字の 16 進 64 文字）。SHA-256 より桁違いに速く、mmap と複数コアで大きな素材を読める。16 進のままキャッシュのフォルダー名に使うので、`:` などパスに使えない文字を入れない。
- 時刻: `{"num": 整数, "den": 正の整数}` の有理数（秒）。
- 言語を問わない。`tools/transcribe`（Python）が最初の例。

## 同時に足したもの（この RFC の範囲外のクレート）

- `mm-ops` に `addAsset` と `removeAsset` を足した（取り込みが素材をプロジェクトに足す操作。`schema/ops.json` を更新）。素材を足す操作はそれまで無かった。
