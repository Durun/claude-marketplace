# RFC 0015: エンコード（段階 4）

状態: 採用

## 変える理由

- 段階 2 の書き出しは libx264 だけで、NVENC、ProRes、DNxHR、2 パスを書けない。
- NVENC は FFmpeg の `h264_nvenc` / `hevc_nvenc` で書ける。`mm-encode-nvenc` を `mm-encode-ffmpeg` に依存させると依存の規則 3（実装どうしは依存しない）に反し、MP4 と音の詰め込みを複製すると 2 か所に同じ実装が残る。
- 2 パスは映像を 2 回通す。`Encoder` は 1 回しか押せない形だった。

先行事例は RFC 0007 の `mm-decode-ffmpeg-core`（FFmpeg を使う実装の共通部分を名前のあるクレートにして、規則 3 の例外に並べる）と、RFC 0009 の `push_nv12_frames`（`Encoder` に既定の実装つきのメソッドを足す）。同じ形を使う。

## 決めたこと

### 共通実装 `mm-encode-ffmpeg-core`

`mm-encode-ffmpeg`、`mm-encode-nvenc` が使う、FFmpeg でエンコーダーを動かして MP4 と MOV に書く部分。依存はインターフェースと基礎、FFmpeg（feature `ffmpeg`）だけで、実装には依存しない。`mm-gpu`、`mm-decode-ffmpeg-core`、`mm-decode-hw` と並べて規則 3 の例外に書く。

- `CodecSpec`: エンコーダーの名前、コンテナ（MP4、MOV）、エンコーダーが受ける画素形式（NV12、8 ビット 4:2:2、10 ビット 4:2:2、10 ビット 4:4:4）、音のコーデック（AAC、24 ビット PCM）、ビットレートの決め方（品質、1 画素あたりのビット数、プロファイル）、オプション、パスの数、`codec_tag`。FFmpeg なしでもビルドできる。
- `FfmpegEncoder`: `CodecSpec` を名前から引く関数を受け取り、`begin` の設定で指定を決めて書く `Encoder`。

実装のクレートは名前から `CodecSpec` を決めるだけにする。

| クレート | 扱う名前 |
|---|---|
| `mm-encode-ffmpeg` | `h264`（`libx264`）、`h264_2pass`、`prores_422hq`（`prores`、`prores_ks`）、`prores_4444`、`dnxhr_lb`、`dnxhr_sq`、`dnxhr_hq`（`dnxhd`）、`dnxhr_hqx`、`dnxhr_444` |
| `mm-encode-nvenc` | `h264_nvenc`、`hevc_nvenc` |

### `Encoder` に足すメソッド（足すだけ）

```rust
fn passes(&self) -> u32 { 1 }
fn begin_pass(&mut self, pass: u32) -> Result<()> { Ok(()) }
```

2 パスのエンコーダーは `passes()` が 2 を返す。呼ぶ側は `begin_pass(1)` で映像の全フレームを押し、`begin_pass(2)` で同じフレームをもう一度押し、音は 2 回目の後に押す。1 パスの実装は何も変えなくてよく、`begin_pass` を呼ばなくてもよい。`mm-export` は同じ区間を 2 回描く（映像を溜めない。1080p で 1 分が 11 GB になるため）。

### `RuntimeConfig` と組み立て

- `Backend::Nvenc` を足す（エンコードだけ）。
- エンコードの `Backend::Auto` を足す。`AutoEncoderFactory` が `begin` のコーデックで、前から順に（nvenc、ffmpeg）その名前を扱う実装を選ぶ。`h264_nvenc` が頼まれたのに NVENC が開けない（NVIDIA の GPU かドライバーが無い）ときは、同じ H.264 の `h264`（libx264）で書く。`hevc_nvenc` に代わりは無いので、エラーにする。`RuntimeConfig::real()` のエンコードは `Auto` になる。コーデック `h264` の動きは変わらない。

### 並行の書き出し（`mm-export`）

`ExportOptions` に `parallel`（並行に描くレンダラーの数、0 と 1 は並行にしない）と `codec`（レンダーターゲットのコーデックの上書き）を足す。

- 区間を短いチャンク（最大 30 フレーム。大きなフレームでは溜める量が 2 GiB に収まるよう短くする）に分け、`parallel` 本のワーカーがそれぞれ別の `Engine`（同じ設定で組む。GPU のコンテキストは共有、フレームのキャッシュは 256 MiB まで）で描く。
- 受け取りの側（呼び出したスレッド）がチャンクの番号順に 1 つずつ受け取り、バッチでエンコーダーに押す。ワーカーは受け取り済みの数 + `parallel + 1` 未満のチャンクだけを取る（溜まる量の上限）。
- 音は区間の全体を 1 回で混ぜる。チャンクの境で音は切れない。
- 失敗と取り消しは、最初のエラーで全ワーカーを止める。一時ファイルの扱いは 1 本の書き出しと同じ。

## 影響

- API クレートの変更は `Encoder` の 2 つのメソッド（既定の実装つき）だけ。
- 破壊的な変更は無い。`EncodeSettings` と `EncoderFactory` は変わらない。
- `mm-encode-ffmpeg` の公開から `H264_NAMES` が消え、`SUPPORTED_NAMES` と `spec_of` が増える。`FfmpegEncoder` は `mm-encode-ffmpeg-core` に移る。
- `mm render` に `--codec` と `--parallel`、`mm perf run export` に `--codec`、`--parallel`、`--matrix` が増える。
