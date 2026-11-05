# vlog をこの上で作る

DTM_Vlog の作り方は、movie-maker の上の「パック」として持つ。基盤は vlog を知らない。

| DTM_Vlog の今 | movie-maker での形 |
|---|---|
| `pool.yaml` の候補 | `bins` の `selection` |
| `edit.yaml` の並び | 本編のコンポジションの `sequence` |
| 字幕、札、図解（`overlay.mjs` と 7_mix） | 字幕は `text`。ステッカーと図解は `web.cef` のテンプレート。カットに `attachTo` |
| トランジション（`tools/transitions/fx/*.js`、WebGL2） | `shader.wgsl` に書き換える（GLSL ES 3.0 は naga が読めないので、GLSL 440 に直すか WGSL に書き直す） |
| ショート | 本編の `sequence` を参照した縦のコンポジションと、そのレンダーターゲット |
| 編集の好み | パックのマクロと、ミキサーのダッキング |
| ディレクション UI の書き込み | `Review` のコメント |
| `seams.py`、`listen.py` | 対応のまとまりに付く数値の点検 |

ep04 から、この形で作れるかを試す。

