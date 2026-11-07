# mm-text-cef

文字の描画の実装（`text: cef`）。シェーパー（cosmic-text）が組んだ行を、別プロセスの CEF に CSS で描かせる。
縁取り、書体、ルビを CSS で付け、行組みはネイティブの描画と同じにする。

## 分け方

| 段 | 担当 | 中身 |
|---|---|---|
| 行組み | `mm-text-glyphon` の `CosmicShaper` | 折り返しの位置、行の位置、`bounds`。ルビの注記を外して親文字だけを組む。`LineBreak::Phrase` なら文節の中で折らない。縁とふりがなの分だけ `bounds` を広げる |
| 描画 | `mm-text-cef` の `CefRasterizer` | 行ごとの文字を `ShapedText::text` から取り、`assets/text.html` に渡す。行の中は折り返さない |

`text: cef` を選ぶと、`mm-runtime` がこの 2 つを組む。描画が行組みをやり直さないので、文字の幅と行の数がプレートの大きさ（パックの `text` 手順）と合う。

## 先行事例と新規の設計

| 先行事例 | 採ったもの |
|---|---|
| BudouX（Chromium の `word-break: auto-phrase` の元。DTM_Vlog の `tools/linebreak.py` も同じ考え） | 文節の切れ目を `budoux` クレートで求め、文節の中の文字の間に WORD JOINER を挟んで cosmic-text に折らせない |
| 青空文庫の注記（`｜親文字《ふりがな》`、漢字の並びの直後の `《ふりがな》`。小説投稿サイトも同じ書き方） | ルビは文字列に注記で書く。シェーパーとここは同じ `mm_text_api::parse_ruby` で読む。描けない実装は親文字だけを描く |
| CSS の縁取りの定番（`-webkit-text-stroke` を `paint-order: stroke fill` で） | 縁を太さの 2 倍で描き、字の下に回して外側だけを見せる |

新規に設計したもの: 行組みと描画を別の実装に分けたこと。CEF のホストのプロトコルは画像しか返さないので、CEF で組んだ行の位置をアプリへ戻せない。行組みを cosmic-text に任せれば、`bounds` とプレートの大きさを CEF を起こさずに決められる。

## 速さ

528x198（64 px、2 行、ルビと縁つき）で 1 回 26 ms（CPU の経路。JSON の f32 の画像を読む分が大きい）。最初の 1 回はホストの起動とページの読み込みで約 2 秒。`text` の種類が描いた画布をキャッシュするので、同じ文字は 1 回だけ描く。

## 試す

```bash
MM_CEF_HOST=$CARGO_TARGET_DIR/release/mm-host-cef.exe CEF_PATH=D:/dev/cef/156 MM_CEF_OUT=<PNG の置き場> \
  cargo test --release -p mm-text-cef --test it cef:: -- --nocapture
```

## 残したもの

- `text` の種類（`mm-nodes-basic`）に縁取りと折り返しの位置のパラメータが無い。プロジェクトから使うには、`TextStyle::stroke` と `line_break` を作るパラメータを足す（レンダラーの担当）。
- Chromium と cosmic-text の字の幅の差で、行の端の縁が 1〜2 px 欠けることがある（OS に無い書体を指したとき、両者が別の代わりの書体を選ぶ）。
- 共有テクスチャの経路。字幕を毎フレーム描き替える用途で要るなら、`mm-nodes-cef` と同じ変換を使う。
