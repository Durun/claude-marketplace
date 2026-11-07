# vlog のパック

DTM_Vlog（`D:/dev/DTM_Vlog/`）の作り方を、movie-maker の上のデータとして持つ（[13-vlog-pack.md](../../docs/design/13-vlog-pack.md)）。読み込みと検査と展開は `crates/mm-pack`。

## 中身

| ファイル | 中身 | 元にした DTM_Vlog の決まり |
|---|---|---|
| `theme.json` | 5 色（サファイア、黄色、墨、紙、ミクの色）と白、書体の役割、動きの上限（出る 0.8 秒、消える 0.3 秒）、本編とショートの画面 | `docs/design-guide.md` の「見た目の土台」 |
| `styles.json` | 文字のスタイル: 字幕、右上のラベル、注釈、テロップ、セクションの見出し。`text` と、プレートを描く `shader.wgsl`（角丸の地、縁、ずらした影、傾き） | `design/theme.yaml` の `overlays`、`docs/design-guide.md` の「置き場所」 |
| `macros/` | `caption`、`label`、`note`、`telop`、`section`（プレートとチャプターのマーカー）と、`web.cef` で描く部品のマクロ（下の「web の部品」） | `docs/design-guide.md` の「どの部品で伝えるか」 |
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

時刻はどれもレイヤーの始まりからの秒。リストの引数は 1 つの文字列で渡す（区切りは表の通り）。

| マクロ | 中身 | 時刻で動くもの | 元にした 7_mix の部品 |
|---|---|---|---|
| `callout` | 画面の一点を指す吹き出し。しっぽは `tail` で下・上・横、`name` で道具の名前を題字の書体で前に置く | なし | `callout` |
| `claude` | Claude の吹き出し。墨の地に白い文字を、ロゴの白い札から出す。`\n` で行を折り、`[[言葉\|yellow]]` で札の色の角丸 | 言葉が 1 文字ずつ出る | `claude`、`clBubble` |
| `figure` | 図解。見出しと行（`labels`、`comments` は `\|` 区切り、`kinds` は空白区切りの行の絵）。最後の行はねらい | 行が `row_at` に出る。`cues`（`行@始まり+長さ`）の間は行が光り、スピーカーの札と再生位置の線 | `figure_delay_reverb` |
| `lyrics` | 採点モード風の歌詞。`words`（`言葉@始まり+長さ`、`/` で行）と `notes`（`MIDI の番号@始まり+長さ`） | 言葉と棒がミクの色に染まり、棒が流れ、矢印が今の音へ動く | `lyrics` |
| `read` | 読む窓。`doc` の行頭の印で題（`# `）、節（`## `）、要点の箱（`> `）、項目（`- `）。`**…**` は青い太字、`==…==` はマーカー | `segs`（`言葉@始まり+長さ` を `\|` 区切り）の下線が伸び、読む行が見えなくなると段落の頭へ送る | `figure_doc` |
| `sticker` | ステッカー。`paper` は紙の質感の札、`oto` は描き文字（外の墨、サファイアか墨の縁、面の 3 枚） | なし | 7_mix の文法、`telop` の描き文字 |
| `tip` | 小ネタ。ワイプから出る「ちなみに」の吹き出し。見出しの `｜` の後ろは縦線で区切った添え | なし | `tip` |

### web の部品の書き方

- `web/<名前>.html` に置き、`window.mmSetTime(t, data)` で画面を作る。`data` はマクロの `web` 手順の `data`（引数を埋めたもの）に `theme`（`colors`、`fonts`、`canvas`）を足したもの。
- 1 倍の画素（コンポジションの画素）で組み、背景は透明のままにする。色と書体は `data.theme` から引き、HTML に色を直に書かない。
- 出る・消える動きは手順の `fade_in`、`fade_out`（レイヤーの不透明度）で付け、ページは時刻で動かさない（`animated: false`。パラメータの組ごとに 1 回だけ描く）。
- 時刻で中身が変わる部品（歌詞の色の進み、図解の `cues`、読む窓の下線と送り、Claude の 1 文字ずつ）だけ `animated: true` にし、`t` と `data` だけで画面を決める。時計、乱数、タイマー、CSS のアニメーションとトランジションは使わない（同じ `t` でも画素が変わる。`tests/it/vlog_web.rs` が確かめる）。
- 画像と書体は `data:` の URL で HTML に埋め込む。`web.cef` はネットワークとファイルの読み込みを CSP で禁じる（外の URL、相対のパス、`file://` は読めない）。書体は OS に入っているものか埋め込んだものを使う。
- HTML はレイヤーのパラメータにコピーされるので、プロジェクトはパックのフォルダーが無くても同じに描ける。部品の HTML を直しても、置いたレイヤーは変わらない（置き直す）。

アプリは Tauri のコマンド `pack_list`（`{dir}`）と `pack_apply`（`{dir, name, args}`）で同じことをする。

## 段階 6 に回すもの

`web.cef`（HTML と CSS を Chromium で描く）が要るもの。仕組み（`web` 手順、`web/` の部品）と、上の「web の部品」の表のものはできた。残りは `web/` に部品を足し、マクロを書けば作れる。

| もの | DTM_Vlog の今 |
|---|---|
| 見出しの ToDo（`card`）、題字（`title`、`title_thumb`）、名乗り、次回予告 | `design/templates/card.html`、`title.html` |
| 回ごとの形の図解（曲の構成、倍音、ノートの図、コードのピアノロール、アタックの図など）。行の形の図解（`figure`）にない絵 | `epNN/design/look.yaml`、`fig_*.html`、7_mix の `figure_*` |
| 描き文字のテロップの飾り（速度線、当たりの爆発、前置きと添えの分け方） | 7_mix の `telop`、`speedLines`、`burst` |
| 画面の注記と線（`marks`）、締めのお願い（`cta`）、縦の動画の終わりのカード | `overlay.mjs` |
| 巻き戻しのトランジション（`rewind`） | `design/templates/rewind.html` |

`web.cef` は要らないが、ほかの領域の機能を待つもの。

| もの | 待っているもの |
|---|---|
| WebGL のトランジション 19 種（`tools/transitions/fx/*.js`）を WGSL に | 2 つの入力を受ける `shader.wgsl` のトランジションの種類（レンダラー） |
| 字幕の縁取り、書体の指定（M PLUS Rounded 1c など）、歌詞だけミクの色にする | `text` の種類の縁と書体のパラメータ（レンダラー。文字の `TextStyle::stroke` と、縁を描く `text: cef` はある）、文字の一部の色（文字） |
| 字幕を語と文節の切れ目で折る（`tools/linebreak.py`） | `text` の種類の折り返しの位置のパラメータ（レンダラー。文字の `LineBreak::Phrase` はある） |
| 注釈が出ている間にセクションの見出しを抜く、顔の反対側に置く | 部品の重なりと被写体の位置を見る組み立て（ep04 で要るか確かめる） |
