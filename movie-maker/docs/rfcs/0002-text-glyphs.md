# RFC 0002: シェーピングの結果にグリフを持たせ、揃えをスタイルに置く

状態: 採用

## 変える理由

`ShapedText` がグリフの ID も文字列も持たないため、レンダラーはシェーピング時の文字列を実装のレジストリから引いて行組みをやり直していた。
シェーパーとレンダラーを同じ `TextEngine` から取る制約が生まれ、`mm-runtime` の組み立てもそれに縛られていた。
揃え（align）がインターフェースに無く、実装の `TextAlign` / `shape_aligned` で受けていた。

## 新しいシグネチャ

```rust
pub enum TextAlign { Left, Center, Right, Justified }
pub struct TextStyle { /* 既存 */ pub align: TextAlign }
pub struct FaceRef { pub post_script_name: String, pub weight: u16 }
pub struct GlyphBox {
    pub rect: Rect,
    pub byte_offset: usize,
    pub byte_len: usize,
    pub face: u32,          // ShapedText::faces の索引
    pub glyph_id: u16,
    pub origin: [f32; 2],   // 1 倍でのグリフの原点（ベースライン上）
}
pub struct ShapedText { pub lines, pub bounds, pub faces: Vec<FaceRef> }
```

- フェイスは PostScript 名と太さで指すので、`ShapedText` は JSON を往復しても描ける（キャッシュに置ける）。
- 契約に「グリフの参照が `faces` と文字列の中を指す」「`Center` / `Right` で短い行は右から始まる」「JSON を往復した結果も描ける」を足す。

## 影響する実装

- `mm-text-glyphon`: レジストリと `shape_aligned` を消す。描画はフェイスを名前で引き、`CacheKey` から swash のグリフのビットマップを取る。`CosmicShaper::new` / `GlyphonRasterizer::new` で別々に作れる。
- `mm-testkit`: モックのシェーピングが揃えを持ち、グリフのフィールドを埋める。
- `mm-nodes-basic` とモックの `text`: パラメータ `align`（`left` / `center` / `right` / `justified`）。

## 移行の手順

1. インターフェースにフィールドと `TextAlign` を足す。
2. 実装と契約テストを直す。
3. `TextEngine` はフォントの読み込みを共有する便宜として残す（制約ではない）。
