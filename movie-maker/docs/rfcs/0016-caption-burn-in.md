# RFC 0016: 字幕の焼き込み

状態: 採用

足すだけで、既存のインターフェースは変えない。

## 先行事例

- 字幕を絵に描く方式: ffmpeg の `subtitles` フィルター（libass で描く）、After Effects と DaVinci Resolve のテキストレイヤー（字幕ごとにレイヤーを置く）。
  字幕のファイルから描く方式は、描画系を 2 つ持つことになる。
- このプロジェクトでは、`text` ノード（cosmic-text と glyphon）が文字の描画を持つ。

## 採ったもの

テキストレイヤー方式。字幕 1 件を `text` ノードのレイヤー（影と本体の 2 つ）にして、コンポジションの最上段に足したプロジェクトのデータを作る。
描くのは既存のレンダラーなので、書き出しとプレビューは同じ関数の結果を描けば一致する。

## 足すもの

`mm-doc`（スキーマ v9）: `Delivery.burn_in_captions: bool`（`serde(default)`。v8 の JSON は焼き込まない設定で読める）と、
`Delivery::needs_transcript()`（字幕を出すか焼き込むとき真）。

`mm-export`:

```rust
pub fn burn_in_enabled(project: &Project, composition: &CompositionId) -> bool
pub fn burn_in_captions(
    project: &Project,
    composition: &CompositionId,
    transcript: &TranscriptWords,
    shaper: &dyn TextShaper, // 字の塊の高さを測り、帯の下端に合わせるため
) -> Result<Project>
```

`mm-export` が `mm-text-api` に依存する（`mm-runtime` が既に依存している口で、依存の規則の例外は要らない）。

## 影響する実装

- `mm-export`: `export_run` が、焼き込む設定のレンダーターゲットで字幕のレイヤーを足したプロジェクトのデータを描く。文字起こしが無ければ拒否する。
- `apps/movie-maker`: プレビューが、焼き込むレンダーターゲットを持つコンポジションを、同じ関数の結果で描く（`PlayerOptions::store_dir` から文字起こしの単語を読む）。
- `apps/mm`: `mm render` が焼き込む設定でも文字起こしを読む。
