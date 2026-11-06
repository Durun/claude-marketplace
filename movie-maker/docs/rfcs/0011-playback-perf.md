# RFC 0011: 再生の性能（段階 2）: 単色を GPU で塗る

状態: 採用

## 変える理由

- `mm perf run playback`（1080p60、solid 10 枚と text 3 枚）で `render.frame` が p50 98.6 ms（予算 4 ms）。ほぼ全部が
  solid の画布の大きさの f32 の画像の生成、f16 への変換、アップロードで、1080p で 1 枚約 8 ms かかる。
- 1 フレームで約 500 MB の一時の確保をするため、スクラブの p99 も揺れる（縮小 92 ms、正確 437 ms）。

## 先行事例と新規の設計

- 先行事例: 単色の層は GPU のクリアで塗るのが通例（wgpu の `LoadOp::Clear`、D3D12 の `ClearRenderTargetView`）。
  CPU の画像を介さない。
- 新規: ノードの種類は wgpu を知らなくても塗れるよう、`GpuHelper` に塗りの操作を足す。

## インターフェースに足したもの（既定の実装つき。壊す変更は無い）

| クレート | 足したもの |
|---|---|
| `mm-node-api` | `GpuHelper::fill(size, color)`。1 色（線形の premultiplied）で塗ったテクスチャ。既定は `upload(&RgbaImage::filled(..))` |

共通実装（`mm-gpu`）に足したもの:

- `WgpuGpu::fill`: クリアのパスで塗る。`allocate` は透明の `fill`。
- `WgpuGpu::is_borrowed(id)`: デコーダーから借用した番号か。レンダラーは借用でない GPU のテクスチャをコピーせずにフレームのキャッシュに覚える。
- `WgpuGpu::finish_readback_rows`: 余白を外した行を順に渡す。`finish_frame_readback` はマップした範囲から直接 f32 に変換し、
  中間のバイト列を作らない（1080p のリードバックが約 10 ms から約 5 ms）。

## 影響する実装

- `mm-nodes-basic`: `solid` が `cx.gpu.fill` を返す。
- `mm-render`: `layers.rs` の `upload_rendered` が持ち主を `is_borrowed` で決める。
- `apps/mm`: `perf run playback` はフレームのキャッシュなしで測り（予算の表の「キャッシュなし」）、続けてキャッシュありで
  ドロップしたフレームを測る（`playback.cached_*`）。`perf run scrub` はキャッシュなしで測る。
