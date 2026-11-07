# vlog のパック

DTM_Vlog（`D:/dev/DTM_Vlog/`）の作り方を、movie-maker の上のデータとして持つ（[13-vlog-pack.md](../../docs/design/13-vlog-pack.md)）。読み込みと検査と展開は `crates/mm-pack`。

## 中身

| ファイル | 中身 | 元にした DTM_Vlog の決まり |
|---|---|---|
| `theme.json` | 5 色（サファイア、黄色、墨、紙、ミクの色）と白、書体の役割、動きの上限（出る 0.8 秒、消える 0.3 秒）、本編とショートの画面 | `docs/design-guide.md` の「見た目の土台」 |
| `styles.json` | 文字のスタイル: 字幕、右上のラベル、注釈、テロップ、セクションの見出し。`text` と、プレートを描く `shader.wgsl`（角丸の地、縁、ずらした影、傾き） | `design/theme.yaml` の `overlays`、`docs/design-guide.md` の「置き場所」 |
| `macros/` | `caption`、`label`、`note`、`telop`、`section`（プレートとチャプターのマーカー） | `docs/design-guide.md` の「どの部品で伝えるか」 |
| `templates/episode.json` | 本編とショートのコンポジション、YouTube 向けのレンダーターゲット（-14 LUFS、チャプター） | `templates/ep/`、`tools/short.py` |
| `mixer.json` | 喋り（`voice`）が鳴ると DAW の音と BGM を 11 dB 下げるダッキング | `templates/ep/edit.yaml` の `levels` |
| `corrections.json` | 字幕の表記の置き換え。`caption` が適用する | `templates/ep/edit.yaml` の `corrections` |

## 使い方

```sh
mm pack check packs/vlog
mm pack list packs/vlog
mm new ep04 && mm pack apply ep04 packs/vlog episode --var n=4
mm pack apply ep04 packs/vlog telop --var id=t1 --var text=シュイーーン --var at=12.5
mm pack expand packs/vlog note --var id=n1 --var text=OTTって？ --var at=3 > note.json   # mm proposal add --txn に渡す
```

アプリは Tauri のコマンド `pack_list`（`{dir}`）と `pack_apply`（`{dir, name, args}`）で同じことをする。

## 段階 6 に回すもの

`web.cef`（HTML と CSS を Chromium で描く）が要るもの。

| もの | DTM_Vlog の今 |
|---|---|
| 7_mix のステッカーの見た目（描き文字の書体、紙の質感、縁取りの文字） | `design/proposals/directions/7_mix`、`tools/overlay.mjs` |
| 見出しの ToDo（`card`）、題字（`title`、`title_thumb`）、名乗り、次回予告 | `design/templates/card.html`、`title.html` |
| 小ネタ（`tip`）、画面を指す吹き出し（`callout` のしっぽ）、Claude の吹き出し | `design/templates/tip.html`、`overlay.mjs` |
| 図解（`figure`）と、その `cues`（行が光る、再生位置の線）、採点モード風の歌詞、読む窓 | `epNN/design/look.yaml`、`fig_*.html` |
| 画面の注記と線（`marks`）、締めのお願い（`cta`）、縦の動画の終わりのカード | `overlay.mjs` |
| 巻き戻しのトランジション（`rewind`） | `design/templates/rewind.html` |

`web.cef` は要らないが、ほかの領域の機能を待つもの。

| もの | 待っているもの |
|---|---|
| WebGL のトランジション 19 種（`tools/transitions/fx/*.js`）を WGSL に | 2 つの入力を受ける `shader.wgsl` のトランジションの種類（レンダラー） |
| `mixer.json` を書き出しとプレビューに効かせる | プロジェクトのデータにミキサーの設定を持つ所（プロジェクトのデータと操作、レンダラー） |
| 字幕の縁取り、書体の指定（M PLUS Rounded 1c など）、歌詞だけミクの色にする | `text` の縁、書体、文字の一部の色のパラメータ（文字） |
| 字幕を語と文節の切れ目で折る（`tools/linebreak.py`） | 折り返しの位置の指定（文字） |
| 注釈が出ている間にセクションの見出しを抜く、顔の反対側に置く | 部品の重なりと被写体の位置を見る組み立て（ep04 で要るか確かめる） |
