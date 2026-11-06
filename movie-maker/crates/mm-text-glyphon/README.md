# mm-text-glyphon

文字の整形（cosmic-text）と、2 通りの描画（`cpu` の swash、`glyphon` の wgpu）。

## glyphon は試験用

`glyphon` のレンダラーは、`rasterize` のたびに出力のテクスチャとリードバックのバッファを作り、
`map_blocking` で GPU の完了を待って CPU の `RgbaImage` に読み戻す。GPU のフレームをそのまま返す口
（レンダラーが文字のテクスチャを直接受け取る形）ができるまでは、速さの出る経路ではない。
1080p の字幕は `cpu` のほうが速い（`RuntimeConfig::real()` は `cpu` を選ぶ）。
`glyphon` は、GPU の文脈を共有する経路と、CPU のレンダラーとの絵の一致（`tests/gpu.rs`）を確かめるために持つ。

## グリフの ID

glyphon の `CustomGlyph::id` は `u16`。グリフの絵は `glyph_ids.rs` の表で ID を振り、
1 回の絵で使い切ったときは表と atlas を作り直して振り直す。1 回の絵だけで 65536 グリフを超えるときは
`Unsupported` で断る。
