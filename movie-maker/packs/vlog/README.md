# vlog のパック

DTM_Vlog（`D:/dev/DTM_Vlog/`）の作り方を、movie-maker の上のデータとして持つ（[13-vlog-pack.md](../../docs/design/13-vlog-pack.md)）。読み込みと検査と展開は `crates/mm-pack`。

## 中身

| ファイル | 中身 | 元にした DTM_Vlog の決まり |
|---|---|---|
| `theme.json` | 5 色（サファイア、黄色、墨、紙、ミクの色）と白、書体の役割、動きの上限（出る 0.8 秒、消える 0.3 秒）、本編とショートの画面 | `docs/design-guide.md` の「見た目の土台」 |
| `styles.json` | 文字のスタイル: 字幕、右上のラベル、注釈、テロップ、セクションの見出し。`text` と、プレートを描く `shader.wgsl`（角丸の地、縁、ずらした影、傾き） | `design/theme.yaml` の `overlays`、`docs/design-guide.md` の「置き場所」 |
| `macros/` | `caption`、`label`、`note`、`telop`、`section`（プレートとチャプターのマーカー）と、`web.cef` で描く部品（下の「web の部品」） | `docs/design-guide.md` の「どの部品で伝えるか」 |
| `web/` | `web.cef` で描く HTML の部品（下の「web の部品」）。マクロの `web` 手順がレイヤーにし、テーマの色と書体を `data.theme` で渡す | `design/templates/tip.html`、`tools/overlay.mjs`、`design/proposals/directions/7_mix` |
| `templates/episode.json` | 本編とショートのコンポジション、YouTube 向けのレンダーターゲット（-14 LUFS、チャプター） | `templates/ep/`、`tools/short.py` |
| `mixer.json` | 喋り（`voice`）が鳴ると DAW の音と BGM を 11 dB 下げるダッキング（`mm_doc::CompMixer` の形。`episode` を適用すると本編とショートに `setMixer` で入る） | `templates/ep/edit.yaml` の `levels` |
| `corrections.json` | 字幕の表記の置き換え。`caption` が適用する | `templates/ep/edit.yaml` の `corrections` |

## 使い方

```sh
mm pack check packs/vlog
mm pack list packs/vlog
mm new ep04 && mm pack apply ep04 packs/vlog episode --var n=4
mm pack apply ep04 packs/vlog telop --var id=t1 --var text=シュイーーン --var at=12.5
mm pack expand packs/vlog note --var id=n1 --var text=OTTって？ --var at=3 > note.json   # mm proposal add --txn に渡す
mm pack apply ep04 packs/vlog callout --var id=co1 --var text=このつまみで空気感が変わる --var x=1180 --var y=640 --var at=2
```

`callout` などの web の部品は `web.cef` で描く。CEF の配布物とホストが無い環境（`CEF_PATH` か `MM_CEF_HOST` が無い）では、モックの矩形で描く（[mm-nodes-cef](../../crates/mm-nodes-cef/README.md)）。

### web の部品

マクロと `web/` のページは同じ名前。引数は `mm pack list packs/vlog` が出す。

- `callout`（画面の一点を指す吹き出し）: 白い紙と墨の縁、ずらした影、しっぽ。
- `card`（見出しの ToDo）: 回の ToDo を全部並べ（`list` を「|」で区切る）、今の行（`no`）に黄色の欄と再生の印とマーカー。`plain=true` は節の題だけ。
- `cta`（締めのお願い）: 高評価、チャンネル登録、コメントのステッカー 1 枚（`icon`）。言った時刻ごとに 1 つずつ置く。`kind=tag` はおまけのオチの黄色の札。
- `marks`（画面の注記と線）: 札、枠、矢印、音名、塗り、丸い札、ねらいの一言、配線の色の線のどれか 1 つ（`kind`）。座標は `pts`（「x,y x,y」）と `box`（「x,y,w,h」）。
- `name`（名乗り）: 斜めの帯に名前、黄色の肩書きの札、顔を指す矢印。帯の中の速度線は出た後も流れる（`animated`）。
- `next`（次回予告）: 爆発と紙吹雪の上に「次回予告！」、次の ToDo の紙と回の番号。
- `short_end`（縦の動画の終わりの札）: ショートに置く。誘いの紙の札、名乗りの帯、題字、リボン。帯の速度線は流れる（`animated`）。
- `title`（題字）: サムネ風の題、番号の星形のバッジ、ミクの色のリボン。きらめきは出た後も瞬く（`animated`）。

### web の部品の書き方

- `web/<名前>.html` に置き、`window.mmSetTime(t, data)` で画面を作る。`data` はマクロの `web` 手順の `data`（引数を埋めたもの）に `theme`（`colors`、`fonts`、`canvas`）を足したもの。
- 1 倍の画素（コンポジションの画素）で組み、背景は透明のままにする。色と書体は `data.theme` から引き、HTML に色を直に書かない。
- 出る・消える動きは手順の `fade_in`、`fade_out`（レイヤーの不透明度）で付け、ページは時刻で動かさない（`animated: false`。パラメータの組ごとに 1 回だけ描く）。出た後も動く決まりの部品（題字のきらめき、名乗りの帯の速度線）だけ `animated: true` にして、`t` から形を決める。
- 乱数の要る形（爆発のぎざぎざ、紙吹雪、速度線）は、種から決まる乱数（mulberry32）で毎回同じに作る。
- 画像と書体は `data:` の URL で HTML に埋め込む。`web.cef` はネットワークとファイルの読み込みを CSP で禁じる（外の URL、相対のパス、`file://` は読めない）。書体は OS に入っているものか埋め込んだものを使う。テーマの 3 つの書体（M PLUS Rounded 1c、Dela Gothic One、Mochiy Pop One。どれも OFL）は 1 つ 2〜5 MB あり、HTML はレイヤーごとにコピーされるので埋め込まない。OS に無い環境では Yu Gothic UI で描く（題字と札の字が細く、幅が狭くなる）。太さは書体の持つ面だけで描き（`font-synthesis: none`）、1 つの太さしかない Dela Gothic One と Mochiy Pop One に合成の太字を掛けない。
- HTML はレイヤーのパラメータにコピーされるので、プロジェクトはパックのフォルダーが無くても同じに描ける。部品の HTML を直しても、置いたレイヤーは変わらない（置き直す）。

アプリは Tauri のコマンド `pack_list`（`{dir}`）と `pack_apply`（`{dir, name, args}`）で同じことをする。

## 段階 6 に回すもの

`web.cef`（HTML と CSS を Chromium で描く）が要るもの。仕組み（`web` 手順、`web/` の部品）と、上の「web の部品」はできた。残りは `web/` に部品を足し、マクロを書けば作れる。

| もの | DTM_Vlog の今 |
|---|---|
| 7_mix のステッカーの見た目（描き文字の書体、紙の質感、縁取りの文字） | `design/proposals/directions/7_mix`、`tools/overlay.mjs` |
| 題字が崩れ落ちる（`title` の `crumble`）、縦の動画の終わりの札の × 印と看板（`short.end` の `crumble`、`sign`）、終わりの札を矩形に収める（`title_box`、`cta_box`） | `7_mix` の `title`、`short_end` |
| 小ネタ（`tip`）、Claude の吹き出し | `design/templates/tip.html`、`overlay.mjs` |
| 図解（`figure`）と、その `cues`（行が光る、再生位置の線）、採点モード風の歌詞、読む窓 | `epNN/design/look.yaml`、`fig_*.html` |
| `marks` の時間で動く印（鳴っている間の流れる点と拍の脈、`play` の再生位置の線、`band` の光、`lane` の別のパートの MIDI、`ping` の波紋）、`cta` の照準と顔のまわりの印（`lock`、ハートなど） | `7_mix` の `marks`、`cta` |
| 巻き戻しのトランジション（`rewind`） | `design/templates/rewind.html` |

`web.cef` は要らないが、ほかの領域の機能を待つもの。

| もの | 待っているもの |
|---|---|
| WebGL のトランジション 19 種（`tools/transitions/fx/*.js`）を WGSL に | 2 つの入力を受ける `shader.wgsl` のトランジションの種類（レンダラー） |
| 字幕の縁取り、書体の指定（M PLUS Rounded 1c など）、歌詞だけミクの色にする | `text` の種類の縁と書体のパラメータ（レンダラー。文字の `TextStyle::stroke` と、縁を描く `text: cef` はある）、文字の一部の色（文字） |
| 字幕を語と文節の切れ目で折る（`tools/linebreak.py`） | `text` の種類の折り返しの位置のパラメータ（レンダラー。文字の `LineBreak::Phrase` はある） |
| 注釈が出ている間にセクションの見出しを抜く、顔の反対側に置く | 部品の重なりと被写体の位置を見る組み立て（ep04 で要るか確かめる） |
