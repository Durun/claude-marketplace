# ノードのインターフェース

OpenFX のプラグイン API に倣う（非同期の描画、時刻で引ける入力、描く領域、恒等の宣言、画素の形の宣言）。Rust のトレイトで持つ。

```rust
pub trait NodeKind: Send + Sync {
    fn id(&self) -> &'static str;             // "footage", "text", "web.cef", "shader.wgsl", "ofx" ...
    fn version(&self) -> u32;
    fn params(&self) -> ParamSchema;          // UI の入力フィールドと MCP の引数はここから作る
    fn inputs(&self) -> InputSchema;          // 効果やトランジションが受ける映像
    fn traits(&self) -> Traits;               // realtime, deterministic, offline, time_varying, audio, vector, pixel, cost, isolation
    fn duration(&self, p: &Params) -> Option<Time>;
    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params>;
    fn prepare(&self, p: &Params, cx: &PrepareCx) -> Result<()>;      // 素材の読み込み、生成 AI、ホストの起動
    fn frames_needed(&self, cx: &FrameCx) -> Vec<TimeRange>;        // 入力のどの時刻が要るか
    fn region_of_definition(&self, cx: &FrameCx) -> Rect;
    fn is_identity(&self, cx: &FrameCx) -> bool;
    fn render(&self, cx: &mut FrameCx) -> Result<FrameRef>;   // 時刻 t のフレーム。GPU は cx.gpu（GpuHelper）で触る
    fn render_audio(&self, cx: &AudioCx) -> Result<AudioBlock>;
}
```

- `FrameCx` は、時刻 t、評価済みのパラメータ、描く大きさ、要る領域、入力の映像（時刻で引ける）、素材、GPU のヘルパー（`GpuHelper`: CPU の画像を載せる、リードバックする、確保する、破棄する、デコーダーの GPU のフレームを借用する）、ほかのインターフェースの一式（文字、色）。カメラは 3D を足すときに空ける。
- インターフェースに wgpu の型を出さない（RFC 0001）。`GpuHelper` は型を消したトレイトで、契約テストと縦の 1 本は CPU の実装（`mm-testkit`）で回る。wgpu の `CommandEncoder` が要る種類は `GpuHelper::as_any` で共通実装 `mm-gpu` の `WgpuGpu` に降りる。デコーダーが返した GPU のフレームは `GpuHelper::import` で借用し、レンダラーが合成の `submit` の前に待ちを登録し、後に返す。
- 返すフレームは `Rgba16Float`、premultiplied の線形のテクスチャ。CPU で描く種類（Lottie、SVG）はヘルパーでアップロードする。
- 隔離（`traits.isolation`）: `in_process`（Rust の種類。`render` の `Err` と panic は `catch_unwind` で受け、そのレイヤーだけ透明にしてエラーのマークを出す）、`out_of_process`（CEF、OFX、VST3、CLAP。別プロセスで動かし、クラッシュしたらマークを出して起動し直す。時間切れつき）。
- `deterministic` の自己申告は、契約テストで 2 回描いて同じかを確かめる。
- インターフェースは `mm-node-api` に置く。種類は実装のクレート（`mm-nodes-basic` など）に 1 つずつ置き、`mm-runtime` のレジストリに足すだけで使える。

最初に作る種類と、後で足す種類:

| 種類 | 内容 | 時期 |
|---|---|---|
| `footage`、`audio`、`image`、`solid` | 撮影素材（プレビューはプレビュー動画、書き出しは原本）、音、静止画、単色 | 最初 |
| `shape`、`svg` | 図形と SVG。ベクターのまま持ち、vello か resvg で描く | 最初 |
| `composition`、`sequence` | 入れ子とマグネティックシーケンス。`composition` の展開はレンダラーが行う（下の「入れ子」） | 最初 |
| `text` | 字幕と短い文字。ネイティブの文字の描画（下の「文字の描き方」）。多くの字幕はこれで足りる | 最初 |
| `shader.wgsl` | WGSL のフラグメントか compute。uniform がパラメータになる。効果とトランジションもこれで書ける | 最初 |
| `color.cdl`、`color.lut`、`color.ocio` | ASC CDL、`.cube`、OCIO の変換 | 最初 |
| `web.cef` | HTML と CSS のテンプレート、Three.js のシーンを、埋め込みの Chromium で描いて共有テクスチャで受ける。`out_of_process`、`cost: heavy` | 次 |
| `shader.glsl` | naga で WGSL に変換（GLSL 440+、Vulkan の流儀 [4]）。DTM_Vlog のトランジション（WebGL2 の GLSL ES 3.0）は書き換えが要る | 次 |
| `ofx` | OpenFX のプラグイン。別プロセスのホストが描き、共有テクスチャで受ける | 次 |
| `shader.hlsl` | DXC で SPIR-V にしてから naga で WGSL に。naga は HLSL を直接読めない [4] | 後 |
| `lottie` | Lottie（rlottie か pure Rust の実装）で CPU に描いてアップロード | 後 |
| `gen.ai` | 生成 AI。`offline`。費用のかかる API は操作の側で見積もりを強制し、自動の改善の回からは呼べない | 後 |

## 入れ子

種類 `composition`（パラメータ `composition` に中のコンポジションの ID）はインターフェースの `NodeKind` として登録するが、内容の展開はレンダラーが行う（RFC 0004）。種類のインターフェース（`FrameCx`）からはプロジェクトのデータが見えず、入れ子の中のベクターを出力の解像度で 1 回だけラスタにするには、レンダラーが親の変形を知っている必要があるため。種類自身は透明を返し、レンダラーの外で単独に呼ばれても安全。レンダラーは深さ 8 で止める（循環の疑い）。レイヤーの変形（位置・拡大・回転・アンカー）はプロジェクトのデータの `Layer.transform` にあり、種類のパラメータには混ぜない。

## ベクターの解像度

- `vector` の種類は、ラスタにする時期を最後まで遅らせる。レンダラーは、入れ子とレイヤーの変形を出力の画面まで掛け合わせた変形を `FrameCx` に渡す。種類はその大きさで描き、描いたフレームを後から拡大しない。
- 入れ子の中のベクターも、外の変形を中へ折りたたんで、出力の解像度で 1 回だけラスタにする。途中で一度ラスタにするのは、ラスタを入力に取る効果がかかるときだけ。
- プレビューではモニターの実際の画素（表示の倍率と DPI を含む）で描く。書き出しでは出力の解像度で描く。
- 拡大のキーフレームで大きさが変わる区間: プレビューでは、その区間でいちばん大きい倍率で 1 回描いてテクスチャにし、合成で縮める。書き出しでは毎フレーム実寸で描き直す。
- 大きさは画面上の倍率で描き、平行移動は合成の側でサブピクセルにかける（ヒンティングで跳ねない）。
- キャッシュのキーには画面上の大きさを入れ、段階（1、1.5、2、3、4 倍…）に丸める。

## 文字の描き方

2 つの経路を使い分ける。

| 経路 | 内容 | 使う所 |
|---|---|---|
| ネイティブ（`text`） | cosmic-text でシェーピングと行組み、glyphon（cosmic-text + etagere のアトラス + wgpu）[12] で GPU に描く。見出し程度の装飾（縁、影、帯）は WGSL で付ける。vello が成熟したらベクターの経路へ移す | 字幕、短いテロップ、タイムラインの文字。パラメータが文字と色と位置だけのもの |
| CEF（`web.cef`） | HTML と CSS のテンプレート。Chromium がオフスクリーンで描き、共有テクスチャで受ける | テンプレートの見た目が要る所（7_mix のステッカー、図解、Three.js のシーン） |

- `web.cef` は毎フレーム往復させず、パラメータの組ごとに 1 回描いてキャッシュする。動かすのは合成の側。アニメーションする HTML（CSS のトランジション、Three.js）は、`SendExternalBeginFrame` でフレームを進め、時刻 t を JS に渡して決定的に描かせる [6]。
- フォントは OpenType。プロジェクトが使うフォントはライブラリに置き、CEF にも同じフォントを読ませる。

## 変形と合成

- 変形は 4x4 の行列で掛け合わせ、素材を 1 回だけ再標本化する。縮めるときは面積の平均か Lanczos、広げるときは bicubic。線形の光で。
- マスクは被覆率のアンチエイリアス。合成モードの式は W3C Compositing and Blending Level 1。
- モーションブラーのインターフェースを `FrameCx` と `frames_needed` で持つ。最初は実装しない。
- 中間の画像は `Rgba16Float`。8 bit に落とすのは表示と書き出しの最後だけ。

## マグネティックシーケンス（`sequence`）

- 撮影素材を切って並べる編集を、コンポジションの中の 1 つのノードとして持つ（`kind: sequence`。実体は `Project.sequences` にあり、`Cut` は ID、素材の `in`・`out`、`speed`、`enabled`、`transitionIn` を持つ）。OTIO の `Track` に対応づけられる形。
- カットの時刻は前のカットのアウト点から自動で決まる（`mm_eval::place_cuts`。有効なカットの長さ `(out - in) / speed` の和）。カットの間にトランジション（効果音つき）を置ける。トランジションは境目をまたいで掛けるもので、配置は変えない。
- 端の操作（リップル、ロール、スリップ、スライド）はこの中で働く。`attachTo` したレイヤーも一緒に動く。
- 喋りの継ぎ目は自動で処理できる（短いクロスフェード、無音の詰め。マクロで）。外したカットは消さずに残す。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
