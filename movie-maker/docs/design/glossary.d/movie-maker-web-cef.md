## web.cef と CSS の文字

| 用語 | 定義 | 識別子 |
|---|---|---|
| web の部品 | `web.cef` で描く HTML のページ 1 つ。パックの `web/` に置く | `packs/*/web/*.html` |
| web 手順 | web の部品を `web.cef` のレイヤーとして置くマクロの手順。theme を `data.theme` で渡す | `Step::Web`、`WebStep` |
| 前置きのスクリプト | `web.cef` がページの `<head>` の直後に差し込むスクリプト。倍率を CSS の `zoom` で掛け、ページの `mmSetTime` には `data` だけを渡す | `mm_nodes_cef::inject` |
| 静止のページ | 時刻によらず同じ画面のページ。時刻を 0 に揃え、パラメータの組ごとに 1 回だけ描く | `web.cef` の `static` |
| ルビの注記 | 文字列の中にふりがなを書く青空文庫の書き方（`｜親文字《ふりがな》`） | `mm-text-api` の契約 |
| 文節の折り返し | 語と文節の切れ目でだけ行を折ること。日本語は BudouX の分け方 | `LineBreak::Phrase` |
| 縁取り | 字の外側に付ける線。色と太さ | `TextStroke` |
