# mm-nodes-cef

`web.cef` のノードの種類。HTML と CSS と JS のページをレイヤーとして描く（[04-nodes.md](../../docs/design/04-nodes.md) の `web.cef`）。
描くのは別プロセスの CEF（[mm-host-cef](../mm-host-cef/README.md)）で、共有テクスチャを GPU のまま中間の形式（`Rgba16Float`、premultiplied、線形）に直す。

## パラメータ

| 名前 | 型 | 既定 | 意味 |
|---|---|---|---|
| `html` | 文字 | 空 | ページの HTML。空なら何も描かず、ホストも起こさない |
| `data` | JSON | `{}` | ページの `window.mmSetTime(t, data)` に渡す値 |
| `static` | 真偽 | 偽 | 真なら時刻を 0 に揃え、パラメータの組ごとに 1 回だけ描く |

ページの約束:

- `window.mmSetTime(t, data)` を定義し、`t`（レイヤーの始まりからの秒）と `data` の関数としてだけ画面を作る。
- 1 倍の画素（コンポジションの画素）で組む。プレビューの縮小は、種類が差し込む前置きのスクリプトが CSS の `zoom` で掛ける。
- 背景は透明のままにする（CEF のオフスクリーンは透過で描く）。
- 時計（`Date.now()`、`performance.now()`）、乱数、CSS のアニメーションとトランジションを使わない。どれも実時間で進み、同じ `t` でも画素が変わる。
- 画像と書体は `data:` か `blob:` で埋め込む。前置きの CSP がネットワークとファイル（外の URL、相対のパス、`file://`）の読み込みを禁じ、ホストの `RequestHandler` も同じ要求とページの遷移を拒否する（mm-host-cef）。
- 埋め込んだ Web フォントは、ホストが読み込みを待ってから描く。

## 先行事例と新規の設計

| 先行事例 | 採ったもの |
|---|---|
| OBS の browser source（CEF のオフスクリーンの共有テクスチャを、コールバックの中で自前のテクスチャへコピーする） | 共有テクスチャを開いたらすぐ自前のテクスチャへコピーし、ホストとの約束の状態（COMMON）に戻す |
| Remotion、HyperFrames（ページを時刻の関数として書き、描く時刻を外から決めて 1 フレームずつ撮る） | `mmSetTime(t, data)` だけで画面を決め、ホストが JS の反映を待ってから BeginFrame を送る（mm-host-cef の手順） |
| 04-nodes.md の決まり（パラメータの組ごとに 1 回描き、動きは合成の側） | `static` のページは時刻をキャッシュのキーから外す。vlog の部品は `static` にして、出る・消える動きはレイヤーの不透明度で付ける |

新規に設計したもの:

- 前置きのスクリプト。ページの `mmSetTime` を `Object.defineProperty` で包み、ホストが渡す `{data, scale}` から `zoom` を掛けてから `data` だけをページに渡す。ページは倍率を知らなくてよい。`<head>` の直後に差し込む（doctype より前に入れると Chromium が互換モードで組む）。先頭に CSP の `<meta>` を置き、`default-src 'none'` に inline のスクリプトとスタイル、`data:` と `blob:` だけを許す。パックやプロジェクトの HTML は他人が書いたものでもあり、外の資源は画素を決定的でなくし、ローカルのファイルを画面に出せてしまうため。
- ホストの持ち方。ページごとに読み込んだホストを最大 3 つ置き、溢れたら最後に使ったのが古いホストにページを読み直させる。ホストがクラッシュしたら 1 回だけ起こし直す。レンダラーがデバイスを作り直したら（`GpuContext` が変わったら）、古いデバイスに向けて起こしたホストを止め、新しいアダプターで起こし直す。
- キャッシュ。ページ、`data`、時刻、大きさ、倍率の組で 6 枚までキャッシュする（1080p の `Rgba16Float` は 1 枚 16 MB）。GPU の経路は自前のテクスチャを持ち、頼まれるたびにヘルパーのテクスチャへコピーする（ヘルパーのテクスチャはフレームの終わりに手放されるため）。
- 受け取り方。レンダラーが `WgpuGpu` の D3D12 なら共有テクスチャ、それ以外（契約テストの CPU のヘルパー、D3D12 でない GPU）は CPU の画像。変換の式は mm-host-cef の `pixel` と同じ（半透明は alpha で割ってから線形にし、掛け直す）。
- CEF が無い環境。`WebCefKind::mock()` が、ページと `data` で決まる色の矩形を中央に描く。組み立て（`mm-runtime`）は `mm_host_cef::find_host()` が `None` ならモックを登録する。`find_host()` は `cef` の feature 無しで作ったホストも `None` にする（`probe` で問い合わせる）。

## 速さ

1920x1080、120 フレーム（助走 20）、RTX 5090。ホストの描画、共有テクスチャの受け取り、変換、ヘルパーへのコピー、GPU の完了まで（`tests/it/cef.rs` の `フル_hd_の_1_フレームの時間を測る`）。

| 場合 | p50 | p95 |
|---|---|---|
| 毎フレーム描く（時刻で動くページ） | 5.08 ms | 5.87 ms |
| キャッシュにヒットする | 0.08 ms | |

ページは透過の背景に、半透明の矩形、動く棒、縁取りの文字（`tests/it/pages/probe.html`）。

## 試す

```bash
export CEF_PATH=D:/dev/cef/156 CARGO_TARGET_DIR=E:/dev/mmt/<枠>
cargo build --release -p mm-host-cef --features cef
MM_CEF_HOST=$CARGO_TARGET_DIR/release/mm-host-cef.exe MM_CEF_OUT=<PNG の置き場> \
  cargo test --release -p mm-nodes-cef --test it cef:: -- --nocapture
```

`CEF_PATH` か `MM_CEF_HOST` が無ければ CEF の検査はスキップする（`MM_REQUIRE_CEF=1` で失敗にする）。
`mm frame` で描くときも同じ 2 つの環境変数を付ける。

## 残したもの

- 画像の読み込み待ち。書体は待つが、`data:` の `<img>` と CSS の背景の画像のデコードは待たない（大きい画像を最初に出すフレームで欠けることがある）。
- sandbox は `bundle.sh` で組み立てたホストだけ（mm-host-cef の README）。`cargo build` の実行ファイルを `MM_CEF_HOST` で指すと sandbox なしで動く。
- 動画、`<canvas>`、WebGL のページでの 1 フレームの時間（段階 6 の注意）。
- 同じページを別の `data` で使うレイヤーは 1 つのホストを共有する。違うページが同じフレームに 4 つ以上並ぶと、毎フレームページを読み直す（上限を上げるか、静止のページを先に描いてキャッシュに入れる）。
- キャッシュは 6 枚なので、同じフレームに 7 枚以上の `web.cef` のレイヤーが並ぶと、静止のページでも毎フレーム描き直す（画素は正しいが遅い）。
