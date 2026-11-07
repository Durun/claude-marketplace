# vlog のパック

DTM_Vlog（`D:/dev/DTM_Vlog/`）の作り方を、movie-maker の上のデータとして持つ（[13-vlog-pack.md](../../docs/design/13-vlog-pack.md)）。読み込みと検査と展開は `crates/mm-pack`。

## 中身

| ファイル | 中身 | 元にした DTM_Vlog の決まり |
|---|---|---|
| `theme.json` | 5 色（サファイア、黄色、墨、紙、ミクの色）と白、書体の役割、動きの上限（出る 0.8 秒、消える 0.3 秒）、本編とショートの画面 | `docs/design-guide.md` の「見た目の土台」 |
| `styles.json` | 文字のスタイル: 字幕、右上のラベル、注釈、テロップ、セクションの見出し。`text` と、プレートを描く `shader.wgsl`（角丸の地、縁、ずらした影、傾き） | `design/theme.yaml` の `overlays`、`docs/design-guide.md` の「置き場所」 |
| `macros/` | `caption`、`label`、`note`、`telop`、`section`（プレートとチャプターのマーカー）と、`web.cef` で描く部品（下の「web の部品」） | `docs/design-guide.md` の「どの部品で伝えるか」 |
| `transitions/` | カットの境のトランジション 20 種（下の「トランジション」）。マクロ `transition`（種類と長さ）と `rewind`（巻き戻し）が `setTransition` で置く | `tools/transitions/fx/*.js`、`design/templates/rewind.html` |
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

時刻はどれもレイヤーの始まりからの秒。

- `callout`（画面の一点を指す吹き出し）: 白い紙と墨の縁、ずらした影、しっぽ。しっぽは `tail` で下・上・横（`down`、`up`、`side`）、`name` で道具の名前を題字の書体で前に置く。
- `card`（見出しの ToDo）: 回の ToDo を全部並べ（`list` を「|」で区切る）、今の行（`no`）に黄色の欄と再生の印とマーカー。`plain=true` は節の題だけ。
- `claude`（Claude の吹き出し）: 墨の地に白い文字を、Claude のロゴの白い札から出す。`\n` で行を折り、`[[言葉|yellow]]` で札の色の角丸。言葉は 1 文字ずつ出る（`animated`）。
- `cta`（締めのお願い）: 高評価、チャンネル登録、コメントのステッカー 1 枚（`icon`）。言った時刻ごとに 1 つずつ置く。`kind=tag` はおまけのオチの黄色の札。
- `figure`（図解と cues）: まっすぐの紙に見出しと行（`labels`、`comments` は「|」、行の絵の `kinds` は空白で区切る。dry、reverb、delay、both、none）。最後の行はねらい。行は `row_at` に出て、`cues`（「行@始まり+長さ」）の間は行が光り、スピーカーの札と再生位置の線が出る（`animated`）。
- `lyrics`（採点モード風の歌詞）: `words`（「言葉@始まり+長さ」、「/」で行）と `notes`（「MIDI の番号@始まり+長さ」）。言葉と棒がミクの色に染まり、棒が流れ、矢印が今の音へ動く（`animated`）。`notes` が無ければ歌詞の行だけ。
- `marks`（画面の注記と線）: 札、枠、矢印、音名、塗り、丸い札、ねらいの一言、配線の色の線のどれか 1 つ（`kind`）。座標は `pts`（「x,y x,y」）と `box`（「x,y,w,h」）。
- `name`（名乗り）: 斜めの帯に名前、黄色の肩書きの札、顔を指す矢印。帯の中の速度線は出た後も流れる（`animated`）。
- `next`（次回予告）: 爆発と紙吹雪の上に「次回予告！」、次の ToDo の紙と回の番号。
- `read`（読む窓）: 縦長のブラウザの窓に `doc` を描き直す（行頭の `# ` は題、`## ` は節、`> ` は要点の箱、`- ` は項目。`**…**` は青い太字、`==…==` はマーカー）。`segs`（「言葉@始まり+長さ」を「|」で区切る）の下線が伸び、読む行が見えなくなると段落の頭へ送る（`animated`）。
- `short_end`（縦の動画の終わりの札）: ショートに置く。誘いの紙の札、名乗りの帯、題字、リボン。帯の速度線は流れる（`animated`）。
- `sticker`（7_mix のステッカー）: `look=paper` は紙の質感の札（`tone` で白か黄色）、`look=oto` は描き文字（外の墨、サファイアか墨の縁、面の 3 枚。最後の 3 字は黄色）。
- `tip`（小ネタ）: ワイプから出る「ちなみに」の吹き出し。見出しの「｜」の後ろは縦線で区切った添え。`tag` を空にすると札を出さない。
- `title`（題字）: サムネ風の題、番号の星形のバッジ、ミクの色のリボン。きらめきは出た後も瞬く（`animated`）。

### web の部品の書き方

- `web/<名前>.html` に置き、`window.mmSetTime(t, data)` で画面を作る。`data` はマクロの `web` 手順の `data`（引数を埋めたもの）に `theme`（`colors`、`fonts`、`canvas`）を足したもの。
- 1 倍の画素（コンポジションの画素）で組み、背景は透明のままにする。色と書体は `data.theme` から引き、HTML に色を直に書かない。
- 出る・消える動きは手順の `fade_in`、`fade_out`（レイヤーの不透明度）で付け、ページは時刻で動かさない（`animated: false`。パラメータの組ごとに 1 回だけ描く）。出た後も動く決まりの部品（題字のきらめき、名乗りの帯の速度線）と、時刻で中身が変わる部品（歌詞の色の進み、図解の `cues`、読む窓の下線と送り、Claude の 1 文字ずつ）だけ `animated: true` にして、`t` から形を決める。時計、タイマー、CSS のアニメーションとトランジションは使わない（同じ `t` でも画素が変わる。`tests/it/vlog_web.rs` が確かめる）。
- 乱数の要る形（爆発のぎざぎざ、紙吹雪、速度線）は、種から決まる乱数（mulberry32）で毎回同じに作る。
- 画像と書体は `data:` の URL で HTML に埋め込む。`web.cef` はネットワークとファイルの読み込みを CSP で禁じる（外の URL、相対のパス、`file://` は読めない）。書体は OS に入っているものか埋め込んだものを使う。テーマの 3 つの書体（M PLUS Rounded 1c、Dela Gothic One、Mochiy Pop One。どれも OFL）は 1 つ 2〜5 MB あり、HTML はレイヤーごとにコピーされるので埋め込まない。OS に無い環境では Yu Gothic UI で描く（題字と札の字が細く、幅が狭くなる）。太さは書体の持つ面だけで描き（`font-synthesis: none`）、1 つの太さしかない Dela Gothic One と Mochiy Pop One に合成の太字を掛けない。
- HTML はレイヤーのパラメータにコピーされるので、プロジェクトはパックのフォルダーが無くても同じに描ける。部品の HTML を直しても、置いたレイヤーは変わらない（置き直す）。

アプリは Tauri のコマンド `pack_list`（`{dir}`）と `pack_apply`（`{dir, name, args}`）で同じことをする。

### トランジション

```sh
mm pack apply ep04 packs/vlog transition --var cut=c12 --var kind=whip --var dur=0.7
mm pack apply ep04 packs/vlog rewind --var cut=c20 --var speed=3
```

- 種類は `transitions/<名前>.wgsl`。DTM_Vlog の WebGL2 のトランジション 19 種（boot、dip、dissolve、filter、flash、glitch、hex、leak、liquid、pianoroll、punch、slice、spectrum、stars、stepseq、vinyl、warp、waveform、whip）と、巻き戻し（rewind）。
- `_prelude.wgsl` は全部の前に付く共通のコードで、元の `engine.js` の GLSL の小道具と、`index.html` の合成（マット、重ねる絵、映像の動き、シャッターの間のモーションブラー）を移したもの。合成は元と同じく sRGB の値で行う。
- 元と違う所: 乱数の表（`data`）はセルの番号のハッシュで作るので並びが変わる。効果音（`sfx`）は移さない。前後の場面のぼかしはミップではなく円盤のタップの平均。巻き戻しの時刻の表示（t0 → t7）は文字が要るので描かない。
- 置き直すと差し替わる。外すのは操作 `setTransition`（`transition: null`）。
- 元のコードは DTM_Vlog の自作で、gl-transitions のコードは含まない。小道具のうち `hash11`、`hash21`、`hash22` は Dave Hoskins の Hash without Sine（MIT）、`sdBox`、`sdSeg`、`sdHex` は Inigo Quilez の 2D の距離関数（MIT）と同じ形。

## 段階 6 に回すもの

`web.cef`（HTML と CSS を Chromium で描く）が要るもの。仕組み（`web` 手順、`web/` の部品）と、上の「web の部品」はできた。残りは `web/` に部品を足し、マクロを書けば作れる。

| もの | DTM_Vlog の今 |
|---|---|
| 描き文字のテロップの飾り（速度線、当たりの爆発、前置きと添えの分け方） | 7_mix の `telop` |
| 題字が崩れ落ちる（`title` の `crumble`）、縦の動画の終わりの札の × 印と看板（`short.end` の `crumble`、`sign`）、終わりの札を矩形に収める（`title_box`、`cta_box`） | `7_mix` の `title`、`short_end` |
| 回ごとの形の図解（曲の構成、倍音、ノートの図、コードのピアノロール、アタックの図など）。行の形の図解（`figure`）に無い絵 | `epNN/design/look.yaml`、`fig_*.html`、7_mix の `figure_*` |
| Claude の吹き出しの番号つきの項目（`items`）、読む窓の下に重ねる図（`cover`、`vis`） | 7_mix の `claude`、`figure_doc` |
| `marks` の時間で動く印（鳴っている間の流れる点と拍の脈、`play` の再生位置の線、`band` の光、`lane` の別のパートの MIDI、`ping` の波紋）、`cta` の照準と顔のまわりの印（`lock`、ハートなど） | `7_mix` の `marks`、`cta` |

`web.cef` は要らないが、ほかの領域の機能を待つもの。

| もの | 待っているもの |
|---|---|
| 字幕の縁取り、書体の指定（M PLUS Rounded 1c など）、歌詞だけミクの色にする | `text` の種類の縁と書体のパラメータ（レンダラー。文字の `TextStyle::stroke` と、縁を描く `text: cef` はある）、文字の一部の色（文字） |
| 字幕を語と文節の切れ目で折る（`tools/linebreak.py`） | `text` の種類の折り返しの位置のパラメータ（レンダラー。文字の `LineBreak::Phrase` はある） |
| 注釈が出ている間にセクションの見出しを抜く、顔の反対側に置く | 部品の重なりと被写体の位置を見る組み立て（ep04 で要るか確かめる） |
