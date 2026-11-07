# vlog をこの上で作る

DTM_Vlog の作り方は、movie-maker の上の「パック」として持つ。基盤は vlog を知らない。

| DTM_Vlog の今 | movie-maker での形 |
|---|---|
| `pool.yaml` の候補 | `bins` の `selection` |
| `edit.yaml` のシーケンス | 本編のコンポジションの `sequence` |
| 字幕、テロップ、図解（`overlay.mjs` と 7_mix） | 字幕は `text`。ステッカーと図解は `web.cef` のテンプレート。カットに `attachTo` |
| トランジション（`tools/transitions/fx/*.js`、WebGL2） | WGSL に書き直し、パックの `transitions/*.wgsl` に置く。`transition` のマクロが `transition.wgsl` の種類で `setTransition` する。マット、重ねる絵、映像の動きを 1 つのシェーダーで合成する仕組みは `transitions/_prelude.wgsl` |
| 巻き戻し（`design/templates/rewind.html`） | `transitions/rewind.wgsl` の絵と、レンダラーの `rewind`（出ていくカットを逆向きに描く）。`rewind` のマクロで置く |
| ショート | 本編の `sequence` を参照した縦のコンポジションと、そのレンダーターゲット |
| 編集の好み | パックのマクロと、ミキサーのダッキング |
| ディレクション UI の書き込み | `Review` のコメント |
| `seams.py`、`listen.py` | 対応のトランザクションに付く数値の点検 |

ep04 から、この形で作れるかを試す。

`web.cef` を使わずに作れる分（色と書体の決まり、文字のスタイル、エピソードのテンプレート、マクロ、ダッキングの設定）は `packs/vlog` にあり、読み込みと検査と展開は `crates/mm-pack`、使う経路は `mm pack` とアプリの `pack_list`・`pack_apply`。中身と、段階 6 に回すものの一覧は [packs/vlog/README.md](../../packs/vlog/README.md)。

`web.cef` の部品は `packs/vlog/web/*.html` に置き、マクロの `web` 手順がレイヤーにする（テーマの色と書体はページの `data.theme` で渡す）。部品の一覧は同じ README の「web の部品」、書き方は「web の部品の書き方」。時刻で中身が変わる部品（歌詞、図解の cues、読む窓、Claude の吹き出し）は `animated: true` で、レイヤーの始まりからの秒 `t` と `data` だけで画面を決める。
