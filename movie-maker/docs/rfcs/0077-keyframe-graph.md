# RFC 0077: Inspector のキーフレームの編集と Graph（段階 4）

状態: 採用

## 変える理由

- Inspector はキーフレームのあるパラメータを最初のキーフレームの値で見せるだけで、足す・外す・補間を変える手段が無かった。
- Graph のドラッグは時刻と値を同時に動かす。`removeKeyframe` と `setKeyframe` の 2 つの操作に分けると、`mm diff` と記録で 2 つの編集に見え、CLI と MCP から同じ動きを出すにも 2 つ要る。

## 先行事例と新規の設計

- 先行事例:
  - After Effects のキーフレームナビゲーター（前へ、再生位置のキーの有無と切り替え、次へ）。キーの無い位置で押すと今の評価値で足す。動きのある値を入力欄で変えると、再生位置のキーフレームになる。
  - After Effects、DaVinci Resolve（Fusion のスプライン）、Blender の Graph Editor に共通の決まり: キーの時刻はフレームにそろえる。Shift のドラッグは横か縦の 1 つの向きに絞る。ハンドルは区間の外（時間の向き）へ出さない。全部のキーが収まる範囲で見せる（Blender の View All、AE の Fit All）。
- 新規: ハンドルは CSS の `cubic-bezier` を区間で正規化した制御点（データの形の `CubicBezier`）をそのまま動かす。区間の形は始点のキーフレームが持つので、補間の切り替えは再生位置の区間の始点に掛ける。ドラッグはキーを隣のキーの 1 フレーム手前で止め、並びを変えない。

## 足したもの（インターフェースの API クレートは変えない）

| 場所 | 足したもの |
|---|---|
| `mm-ops` | `moveKeyframe { target, from, keyframe }`。時刻 `from` のキーフレームを外して `keyframe` を置く。動かした先に別のキーフレームがあれば拒否する。逆操作は同じ操作で `from` と `keyframe` を入れ替えた形 |
| `ui/panels/graph` | `GraphPanel`（SVG）。キーとハンドルのドラッグ中は手元の見た目と `onPreview` に映し、離したら `moveKeyframe` か `setEase` を 1 つ送る。`mm-eval` と同じ評価（`evalParam`）と、操作を手元に適用する `applyKeyframeOp` |
| `ui/panels/inspector` | 再生位置（`time`、`fps`）を渡すと、各パラメータにキーフレームの列（前へ、足す・外す、次へ、補間）を出す。名前を押すと Graph に出す（`onSelectParam`） |
| `ui/shell`、`apps/movie-maker/web/transport.js` | 再生位置の DOM のイベント `mm:playhead { time, fps }` と、前後のキーへのシークの頼み `mm:seek { time }` |

## 未了

- `Program` へのドラッグ中のプレビュー。操作を確定せずに描き直す仕組み（記録に残さない仮の適用）がアプリに無い。`GraphPanel` の `onPreview` が仮のパラメータを渡すので、アプリにその仕組みができたらつなぐ。
- Graph は WebView（SVG）で作り、[UI](../design/08-ui.md) の表を直した。キーの数が数千になる表示（Timeline と同じ密度）が要るときに wgpu へ移す。
- ベジェとイージングの区間の途中にキーを足すと、制御点を区間ごとに持つため形が少し変わる（AE は足したときにハンドルを計算して形を保つ）。
