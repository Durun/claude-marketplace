# 命令の層

- 文書を変えるのは命令だけ。命令は `mm-ops/src/ops/` に 1 つずつ置き、引数の型（serde + JSON Schema）、検証、適用、逆命令、`touches` を持つ。
- 汎用の命令: `addLayer`、`removeLayer`、`moveLayer`、`setParam`、`setKeyframe`、`removeKeyframe`、`setEase`、`parentLayer`、`attachLayer`、`lockLayer`、`precompose`、`addComposition`、`forkSequence`、`addRenderTarget`、`addMarker`。
- 編集の命令: `insert`、`overwrite`、`lift`、`extract`、`split`、`ripple`、`roll`、`slip`、`slide`、`disable`。選別: `markIn`・`markOut`、`addSelection`、`rate`、`tag`、`addTopic`。文字起こし: `deleteWords`、`fixTranscript`。
- 入口は 4 つで、どれも同じ命令を呼ぶ。wgpu の面（Rust から直接）、WebView の画面（Tauri の IPC）、Claude の CLI（`mm op <project> ripple --clip c12 --edge out --by -0.4`。アプリが動いていれば HTTP で、動いていなければ文書を直接開く）、Claude の MCP（命令ごとのツール。引数の型はスキーマから作る）。
- トランザクション: 複数の命令を 1 つのまとまりとして当てる。ドラッグの連続、マクロ、AI の対応 1 回。取り消しもまとまりの単位。
- 逆命令を記録に残し、取り消しは逆命令、やり直しは元の命令を当て直す。
- 同時書き込み: 1 本の列で順に当て、文書の版番号を進め、変わった所を開いているすべての画面に送る。古い版を元にした命令は、`touches` が重ならなければ当て、重なれば断る。CRDT は使わない。
- ロック: `lock` の付いた所に触る命令は、Claude の入口からは断る。
- 記録: `ops.jsonl`（誰が、いつ、何を、どのコメントへの対応として、逆命令）。
- マクロ: 命令の列を作る手順。vlog の編集の好みは、マクロとテンプレートの組（パック）として持つ。

