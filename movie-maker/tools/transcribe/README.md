# transcribe

素材の音声を [faster-whisper](https://github.com/SYSTRAN/faster-whisper) で文字起こしし、取り込みの付属ファイル（`kind = "transcript"`）を書く外部のツール。
形は `crates/mm-ingest-api/schema/sidecar.schema.json`。書き先は `<project>/cache/ingest/<コンテンツハッシュ>.transcript.json` で、次の `mm ingest` が読んで検証する。

## 入れ方

Python 3.10 以降。仮想環境を作って入れる（Windows、Git Bash）。

```sh
cd movie-maker/tools/transcribe
py -3 -m venv .venv
.venv/Scripts/python.exe -I -m pip install -r requirements.txt
```

- GPU（CUDA 12）で動かすには cuBLAS と cuDNN 9 が要る。`requirements.txt` の `nvidia-*-cu12` が pip で入れ、スクリプトが DLL の場所を足す。CPU だけなら `--device cpu --compute-type int8`。
- 初回はモデル（`large-v3`、約 3 GB）を Hugging Face から取る。置き場所は `HF_HOME` で変えられる。
- 手元の計測（RTX 5090、Windows 11、pip のキャッシュあり）: `pip install` 17 秒。

## 動かし方

```sh
# プロジェクトの cache/ に書く（コンテンツハッシュは BLAKE3 を計算する）
.venv/Scripts/python.exe -I transcribe.py <素材> --cache <project>/cache --language ja
# 書き先を指す
.venv/Scripts/python.exe -I transcribe.py <素材> --out out.json --content-hash <hex>
```

- 動画でも音声でもよい。`--language` を省くと言語を自動で判定する。
- 書く前に凍結したスキーマで検査し、合わなければ書かずに失敗する。
- 単語ごとの時刻（`words`）も書く。単語の ID は取り込む側が「コンテンツハッシュ + 開始の時刻」から決める。
- 時間（RTX 5090、`large-v3`、`float16`）: 7 秒の日本語の音声が、モデルの読み込みを含めて約 5 秒。

## 試験

```sh
.venv/Scripts/python.exe -I -m unittest discover                    # モデルなし。スキーマと変換
MM_TRANSCRIBE_E2E=1 .venv/Scripts/python.exe -I -m unittest discover # 日本語のフィクスチャの音声で通しで確かめる
```

通しの試験は `fixtures/ingest/generate.sh` が作る `fixtures/ingest/out/speech-ja.wav`（Windows の音声合成）を使う。
`fixtures/ingest/transcript-ja.json` はその出力を git に入れたもので、Rust 側（`crates/mm-ingest/tests/it/sidecar_fixture.rs`）が読めることを確かめている。

## 別のツールを書くとき

どんな言語でも、`sidecar.schema.json` に合う JSON を `cache/ingest/<ハッシュ>.<種類>.json` に書けばよい。
`content_hash` は素材のバイト列の BLAKE3（小文字の 16 進 64 文字）で、`mm_ingest::hash_file` と同じ値にする。
種類は `transcript`、`waveform`、`preview_media`、`scenes`。
