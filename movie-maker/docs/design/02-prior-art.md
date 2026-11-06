# 先行事例と新しく設計するもの

## 先行事例から採るもの

| 型 | 先行事例 | この設計での形 |
|---|---|---|
| コンポジション、レイヤー、入れ子、親子づけ、キーフレームとグラフエディター | After Effects | データの骨組み |
| 書き出しの対象を並べて一度に書き出す | AE のレンダーキュー、Resolve の Deliver | レンダーターゲット |
| 素材の選別（I/O で範囲を付け、刻んで並べる） | Resolve の Cut ページ、Premiere のソースモニター、FCP のキーワードコレクション | Media と Source |
| 端の語彙（リップル、ロール、スリップ、スライド）、3 点編集、J/K/L | Avid のトリムモード | 操作とキー操作の語彙 |
| 隙間のできないシーケンスと、カットに接続したクリップ | FCP のマグネティックタイムラインと接続クリップ | `sequence` と `attach` |
| 言葉で切る | Premiere の文字ベースの編集、Descript | Story |
| 見た目を固定し、中身だけを差し替えるテンプレート | Premiere の MOGRT、Remotion の Zod スキーマ | ノードのパラメータの型から入力フィールドを作る |
| パラメータの型を宣言した描画の部品。時刻で引ける入力、描く領域、恒等の宣言 | OpenFX のプラグイン API、Nuke のノード | ノードのインターフェース |
| 有理数の時刻とバージョンつきの交換形式 | OpenTimelineIO、FCPXML | 時刻の型とプロジェクトのデータのスキーマのバージョン |
| 作業色空間を線形の浮動小数にし、入出力で変換する | OpenColorIO、ACES | 色の管理 |
| 変形を 1 つの行列にまとめ、1 回だけ再標本化する | AE の「トランスフォームを折りたたむ」、Nuke の concat | 変形の折りたたみ |
| Rust のコア + wgpu で、UI は別のツール | Gyroflow（コアは純 Rust、UI は QML、GPU は wgpu と OpenCL）[9] | `mm-render` と Tauri の分け方 |
| egui + wgpu で全部ネイティブ | Rerun（`re_renderer`）[10] | 対案 C |
| Web の UI を映像の素材にする。Chromium のオフスクリーン描画を共有テクスチャで受ける | OBS のブラウザソース、CEF の `OnAcceleratedPaint`（`shared_texture_enabled`）[6][7] | `web.cef` のノードの種類と `mm-host-cef` |
| Rust のバインディングで CEF の共有テクスチャを wgpu に取り込む | `cef` クレートの `osr_texture_import`（D3D12 は `OpenSharedHandle` → `texture_from_raw` → `create_texture_from_hal`、Vulkan は外部メモリ）[2] | 同上 |
| 先読みの描画、プレビュー動画、コンテンツハッシュで引くキャッシュ | Resolve の Smart Cache、FCP のバックグラウンドレンダリング、Premiere の Media Cache | フレームのキャッシュと `cache/` |
| プロジェクトを 1 つの SQLite の DB で持ち、変えたページだけ書く。定期的にバックアップを取る | FCP の `.fcpbundle` の中のライブラリの DB（15 分ごとの `Backups/`）、Resolve の disk database、Lightroom の `.lrcat` [81]、SQLite 公式の「アプリのファイル形式」[71] | `project.mmdb`（[保存の形](09-ingest-storage.md#保存の形)） |
| プロジェクトのデータと操作の記録を 1 つのトランザクションで書き、クラッシュは DB が巻き戻す | SQLite の WAL [73]、Redis の AOF | `ops` の表と WAL |
| 作業中は DB、バージョン管理には決定的な書き出し | Lightroom のカタログと XMP のサイドカー、Unity の `.meta` と Force Text のシリアライズ | `versions/` の JSON と `mm export` |
| 操作の記録から意味の差分を読む | Perforce の changelist、Figma の version history | `mm diff` |
| 人の UI と AI のスクリプトが同じデータを書く | Resolve の Python API、Descript の API | 操作の層 |
| インターフェース（ポート）と実装（アダプター）を分け、コアを UI やデータベースなしでテストする | ports and adapters（Cockburn）[30] | `mm-*-api` と実装のクレート、`mm-runtime` の組み立て |
| 安全でないバックエンドと、検証するコアを別のクレートに | wgpu の `wgpu-hal`（バックエンドごとの unsafe な抽象）と `wgpu-core`（検証）[31] | `mm-decode-api` のバックエンド（d3d12va、cuvid）とレンダラー |
| ホストがバージョンつきの関数の表（suite）を渡し、プラグインはホストに記号で依存しない | OpenFX の `OfxHost.fetchSuite` と `mainEntry` [32] | `mm-host-api` の別プロセスのプロトコル |
| 1 つの `App` に部品を足していく組み立て | Bevy の `Plugin::build(&self, app)` と `PluginGroup` [33] | `mm-runtime` の `assemble()` |
| 1 つのワークスペースに小さなクレートを多く置く | Zed（`crates/` に約 250 のクレート、`workspace.dependencies` でバージョンを共有）[34] | `crates/` の木 |
| インターフェースの破壊的変更を機械で見つける | `cargo-semver-checks`（rustdoc の JSON から semver 違反を検査）[35] | API クレートのバージョンの検査 |
| 時刻・範囲・描き込みつきのコメント、バージョンの比較 | Frame.io | Review |
| 要望の保存をきっかけに AI が直す。別の作業場で直して検査が通ったら取り込む | durun-dashboard の `server/improve.ts`、Dependabot | 自動の改善 |
| 役割ごとのバス、ラウドネスのメーター | FCP のロール、EBU R128 | 音のミキサー |
| Rust の関数の差し替えで、再起動なしに直す | Dioxus の subsecond（ジャンプテーブルで関数を差し替え。構造体の形を変えるとクラッシュする）[8] | 自動の改善の反映 |

## 新しく設計するもの

- ノードの 1 つのインターフェースに、撮影素材・文字・シェーダー・CEF の Web のシーン・生成 AI・OFX・入れ子のコンポジションをそろえること。
- ベクターの倍率の扱い（プレビューは範囲の最大の倍率で 1 回、書き出しは毎フレーム実寸）。
- Tauri の 1 つのウィンドウに、wgpu のサーフェスと WebView2 の画面を並べ、再生位置と選択を Rust の状態で 1 つにすること。
- プラグインのホストを別プロセスに置きつつ、GPU の共有テクスチャでリアルタイムに合成すること（`offline` はキャッシュが要るときの選択肢に下げる）。
- 操作の `touches` による同時書き込みの判定と、AI の対応 1 回をトランザクションとして承認・却下すること。
- 自動の改善を worktree で囲い、本番のプロジェクトのハッシュで守ること。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
