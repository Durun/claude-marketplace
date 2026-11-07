# RFC 0097: 文字の縁取り、折り返しの位置、組んだ元の文字列

状態: 提案

## 変える理由

段階 6 の `mm-text-cef`（CSS で文字を描く実装）は、縁取り、ルビ、語の切れ目の折り返しを描く。`mm-text-api` には縁取りと折り返しの位置を頼む型が無く、行組みの結果（`ShapedText`）は元の文字列を持たないので、行ごとの文字を CSS で描き直せない。

## 足したもの（形は変えない）

どれも serde の既定値つきで、既定は今の振る舞いと同じ。`TextStyle` の literal は全部 `..TextStyle::default()` を使っていたので書き換えは要らない。

| 型 | 足したもの | 既定 |
|---|---|---|
| `TextStroke` | 縁取りの色と太さ（字の外側に出る画素） | |
| `LineBreak` | `Anywhere`（UAX #14 のどこでも）、`Phrase`（語と文節の切れ目だけ。日本語は BudouX の分け方） | `Anywhere` |
| `TextStyle` | `stroke: Option<TextStroke>`、`line_break: LineBreak` | `None`、`Anywhere` |
| `ShapedText` | `text: String`（組んだ元の文字列。グリフの `byte_offset` はこの中を指す） | 空 |

契約に書き足したもの（文書だけ）:

- シェーパーは縁の分だけ `bounds` を四方に広げる。描けない実装は縁なしで描く。
- ルビは青空文庫の注記（`｜親文字《ふりがな》`、漢字の並びの直後の `《ふりがな》`）で文字列に書く。注記を読む実装は親文字だけを組み、親文字の中で折らず、ふりがなの分だけ `bounds` を上に広げる。描けない実装は親文字だけを描く。

## 先行事例

| 先行事例 | 採ったもの |
|---|---|
| CSS の `word-break: auto-phrase`（Chromium。BudouX を使う） | 折り返しの位置を「どこでも」と「文節」の 2 つから選ぶ |
| 青空文庫の注記、小説投稿サイトのルビの書き方 | ルビを文字列の中の注記で書く。型を足さずに字幕の文に書ける |
| CSS の `-webkit-text-stroke` と `paint-order` | 縁は色と太さだけを持つ |

## 使う所

- `mm-text-glyphon` の `CosmicShaper`: 注記を外して組み、`Phrase` では文節の中で折らない。`text` を埋め、`bounds` を広げる。
- `mm-text-cef` の `CefRasterizer`: `text` と行のグリフの範囲から行ごとの文字を取り、縁とルビを CSS で描く。
- `mm-testkit` のモックのシェーパー: `text` を埋める（CSS の描画をモックのホストで試せる）。

`text` の種類（`mm-nodes-basic`）のパラメータは変えていない。プロジェクトから縁と折り返しを選べるようにするのはレンダラーの担当。
