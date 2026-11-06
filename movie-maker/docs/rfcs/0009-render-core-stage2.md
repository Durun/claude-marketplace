# RFC 0009: レンダラーのコア（段階 2）: 置き方の決まり、GPU の書き出し、デバイスの消失、次の担当が乗る口

状態: 採用

## 変える理由

- `footage` が GPU のフレーム（d3d12va、cuvid、ffmpeg の YUV のアップロード）を受けるには、素材のままの大きさの画像を
  レンダラーが置けなければならない。段階 0 の「画像は画布と同じ大きさ」の決まりでは、CPU で縮小するしかなかった。
- 書き出しが 1080p で 13 fps（予算 120 fps）。CPU の双線形の拡縮、f32 と f16 の往復、CPU の色変換、同期のリードバックが主な時間。
- d3d12va の 5 フレーム目より後の失敗で D3D12 のデバイスが消えると、レンダラーが描けなくなる（`06-engine.md`）。

## 決めたこと

### 画像の置き方（レンダラーの決まり。インターフェースは変えない）

レンダラーは `region_of_definition` を `FrameCx::region` に渡し、返った画像を次の決まりで置く。

| 画像の大きさ | 置き場所 |
|---|---|
| `FrameCx::size`（画布）と同じ | 画布の全体（段階 0 と同じ） |
| それ以外 | `FrameCx::region` の矩形 |

- ノードの契約（`mm_testkit::node::contract`、画像は `cx.size`）は変えない。契約の呼び方（`region == size.to_rect()`）では 2 つの決まりが一致する。
- `footage` と `image` は `region_of_definition` を「素材を縦横比を保って画布に収めた矩形」にし、GPU のフレームは素材のままの大きさで返す。
  CPU のフレームは契約どおり画布に収めた画像を返す。
- `text` は `region` の分だけの画布を描く。ベクターの倍率は領域の大きさだけに掛かり、上限（一辺 8192）も領域で決める。
- 縮小は合成のシェーダー（`compose.wgsl`）で 1 回だけ、線形の光の上で面積平均（出力の 1 画素が覆う範囲を最大 8x8 点で平均）。

### インターフェースに足したもの（既定の実装つき。壊す変更は無い）

| クレート | 足したもの |
|---|---|
| `mm-decode-api` | `Nv12Frame { size, y, uv }`（Rec.709、tv レンジ、Rec.1886 の符号化値）。`Encoder::push_nv12_frames(&[Nv12Frame])`（既定は `Unsupported`。空の列は対応の有無の問い合わせ） |
| `mm-render-api` | `Renderer::export_range(req, encoder, batch, sink)`（既定は `export_range_rgba`: `render_range` + `download` + `push_frames`）。`mm-decode-api` への依存 |

- wgpu のレンダラーは `export_range` で、描いたフレームを GPU で NV12 にし（`export.rs`）、リードバックを 3 フレーム先行させて
  `push_nv12_frames` に押す。エンコーダーが NV12 を受けなければ既定の経路に戻る。
- `mm-export` はレンダラーの `export_range` を呼ぶだけで、実装を知らない。

### デバイスの消失からの復帰（共通実装 `mm-gpu`）

- `GpuContext` が求め（`GpuRequest`）と消失のフラグを持つ。フラグは wgpu の `set_device_lost_callback`、D3D12 の
  `GetDeviceRemovedReason`、`mark_lost`（先に気づいた側からの通知。試験の差し込み口でもある）で立つ。
- `GpuContext::recreate` が同じ求めで作り直し、共有（`shared`）のコンテキストも差し替える。
- `WgpuRenderer` は毎フレームの最初に `is_lost` を見て、消えていれば合成のパイプライン、タイマー、テクスチャ、開いた素材を作り直す。
  前のフレームの `FrameRef` は無効になる。同じコンテキストを持つ他の実装（glyphon、別プロセスのホスト）は `Engine` 側で持ち替える（未了。下）。
- 試験は `Device::destroy` で実際に消失を起こす（`mm-render/tests/device_loss.rs`。共有のコンテキストを壊すので別の binary）。

### ffmpeg のフレームを YUV のまま GPU に上げる（feature `gpu`）

- `mm-decode-ffmpeg` に feature `gpu`（`mm-decode-hw`、`mm-gpu`、wgpu に依存）。`DecoderFactory::open` に `mm-gpu` のコンテキストが
  来たら、4:2:0 の 8 bit と 10 bit のプレーンをそのままテクスチャに上げ、`mm-decode-hw` の変換のパス（3 プレーンのフラグメント）で
  線形の `Rgba16Float` にして GPU のフレームで返す。扱えない形式は CPU の経路。
- RFC 0007 の「ffmpeg のデコーダーは GPU の依存を引かない」は、feature を切った既定の設定では変わらない。`mm-runtime` の `ffmpeg` で有効にする。

### `WgpuGpu` の取り置きと先行するリードバック

- 解放したテクスチャは大きさごとに、リードバックのバッファはバイト数ごとに取り置く（毎フレームの確保を無くす）。借用したフレームは取り置かない。
- `begin_readback` / `finish_readback` で提出と完了の待ちを分ける。f32 と f16 の変換は `half` の slice の変換。

## 次の担当が乗る口

### ノードの種類（`sequence`、`shader.wgsl`、`color.*`）

- 登録は `mm_nodes_basic::all_kinds`（レジストリ）。画像は上の「置き方」の決まりで返す。
- GPU で描く種類は `FrameCx::gpu.as_any()` で `mm_gpu::WgpuGpu` に降り、`context()` の Device と Queue を使う。返す `FrameRef::Gpu` は
  `WgpuGpu` の番号（`create` か `upload` で作る）。レンダラーが合成の後に `release` する。
- `sequence` のように中身の展開にプロジェクトのデータが要る種類は、`composition` と同じく `mm-render/src/layers.rs` の
  `layer_item` でレンダラーが展開する（`NESTED_KIND` の分岐）。時間の写像は `mm_eval::source_time`。
- `color.*` が `TransformProgram` のテクスチャ（3D LUT）を持つなら、`WgpuGpu` の `create` で作ったテクスチャを使う（`mm-color-api` は変えない）。

### フレームのキャッシュ（`mm-cache-fs`、`Scheduler`）

- キーは `mm_render_api::FrameCacheKey`（`digest` は固定）。レンダラーの入口は `layers.rs` の `kind_image`（レイヤー 1 枚）と
  `encode_comp`（入れ子 1 つ）で、どちらも「画像のテクスチャの番号と置き場所（`LayerImage`）」を返す。キャッシュはこの前に引き、後に置く。
- GPU の上に持つ直近のフレームは `WgpuRenderer::outputs`（`KEPT_RENDERED_FRAMES`）。NVDEC のフレームは借用なので、キャッシュに置くなら
  `FRAME_FORMAT` にコピーする（`GpuHelper::import` が返す番号のテクスチャは `submit` の後に無効）。
- NVMe への書き込みは、書き出しと同じ `export.rs` の経路（GPU で NV12 → 先行するリードバック）を使える。
- `Renderer::invalidate` はいま全部の素材を閉じる。該当する素材だけ閉じるのは、コンテナの索引（`timeline.rs`）と合わせて直す。
- 先読みの順は `mm_render_api::Scheduler`（モックは `NearestFirstScheduler`）。

## 影響する実装

- `mm-render`: `layers.rs`（置き方、領域の倍率）、`compose.wgsl`（面積平均）、`export.rs`、`renderer.rs`（復帰）。
- `mm-nodes-basic`: `footage.rs`（`import`、`region_of_definition`）、`text.rs`（領域の画布）。
- `mm-gpu`: `GpuContext`（消失、`recreate`）、`WgpuGpu`（取り置き、先行するリードバック）。
- `mm-encode-ffmpeg`: NV12 で開き、`push_nv12_frames`。
- `mm-decode-ffmpeg`: feature `gpu`、`FfmpegCpuDecoderFactory`。
- `mm-export`: `export_range` を使う。

## 未了（次の担当へ）

- `Engine` が持つ別プロセスのホストの持ち替え。glyphon の `GlyphonRasterizer` は RFC 0012 で持ち替える。
- `time_varying: false` の文字の毎フレームのシェーピング、開くたびの全パケットの走査、`invalidate` が全部の素材を閉じること（段階 0 の注意のまま）。
