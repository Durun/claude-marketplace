# RFC 0012: 段階 2 の積み残し: プレビューを読み戻さずに描く、素材ごとの複数のデコーダー、操作の適用とキャッシュ

状態: 採用

## 変える理由

- プレビューは `Engine::render_frame` で CPU に読み戻した画像を、描画スレッドの別のデバイスに上げ直している。
  10 秒の再生で落ちたフレームは、読み戻しが伸びたフレームだった（PR #39）。
- 同じ素材を 1 つのフレームで別の位置から読むと、素材に 1 つのデコーダーが毎回シークし直す。
- 操作の `touches` に頼ったキャッシュだけを捨てる `Engine::invalidate_touched`（RFC 0010）を、操作の経路が呼んでいない。
- glyphon がデバイスの消失の後も古いデバイスで描こうとする（RFC 0009 の未了）。

## 先行事例と新規の設計

- 先行事例: 描いたテクスチャを同じデバイスのスワップチェーンに直接描くのは、ゲームエンジンと NLE のプレビューの通例
  （DaVinci Resolve、Premiere の GPU のプレビュー）。素材ごとに複数のデコーダーを持つのも、同じ素材を重ねる NLE の通例。
- 新規: レンダラーがデバイスを作り直したときの、描画スレッドのサーフェスの持ち替え。フレームが持つコンテキストで判断する。

## 足したもの（インターフェースの API クレートは変えない）

| クレート | 足したもの |
|---|---|
| `mm-gpu` | `GpuContext::instance()`、`GpuContext::wgpu_adapter()`。サーフェスは同じ Instance から作る |
| `mm-runtime` | `Engine::render_frame_texture(..) -> GpuFrame { texture, context }`、`Engine::gpu_context()`、`RuntimeConfig::present`（Windows では D3D12 のアダプターを選ぶ。Vulkan のアダプターは子ウィンドウのサーフェスの形式を返さなかった） |
| `mm-render` | `SourcePool` が素材ごとに 4 つまでデコーダーを持ち、続けて読む次のフレームはシークしない |
| `mm-text-glyphon` | `GlyphonRasterizer` が消失を見て、同じ求めの共有のコンテキストに持ち替える。`GlyphonRasterizer::context()` |
| `mm-store` | `load_project`、`load_json`、`store_dir`。`mm` とアプリのプロジェクトの読み方を 1 か所にした |
| `mm-ui-layout` | `palette::Palette`、`Palette::themed`、`parse_hex`、`PaintState::palette` |
| `mm-ui-wgpu` | `Ui::set_frame_texture`、`Ui::set_palette` |

## アプリ（`apps/movie-maker`）

- 描画スレッドは再生スレッドのレンダラーと同じ `GpuContext` のデバイスでサーフェスを作り、`GpuFrame` のテクスチャをそのまま描く。
  フレームのコンテキストが替わったら（デバイスの作り直し）、サーフェスと UI を作り直す。アダプターがサーフェスに出せなければ
  自前のデバイスに戻し、再生スレッドに CPU の画像を頼む。
- WebView の `apply_op`（`ui/sdk`）を受け、再生スレッドが操作をトランザクションとして当て、`invalidate_touched` を呼び、今のフレームを描き直す。
  当てた結果はメモリの中だけで、`project.mmdb` には書かない（アプリはストアを読むだけで開いている）。

## 未了

- CLI（`mm op`）は操作ごとの別のプロセスで、`Engine` を持たない。アプリが開いているプロジェクトに CLI の操作を届けて
  `invalidate_touched` を呼ぶのは、ストアの変更をアプリに伝える経路（HTTP の転送）と一緒に作る。
- Timeline の端のつまみのドラッグ: Timeline はコンポジションのレイヤーを出すが、`mm-ops` の `ripple` と `roll` はシーケンスのカットの操作で、
  レイヤーのイン点とアウト点を動かす操作が無い。操作を足すのは `mm-ops` の担当。
- 別プロセスのホストのデバイスの持ち替え。ホストはまだ再生の経路に入っていない。
