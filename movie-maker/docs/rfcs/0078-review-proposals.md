# RFC 0078: Claude の編集の提案と Review パネル

状態: 採用

## 変える理由

段階 5 の終わりの条件は「Claude が MCP で同じ編集をでき、承認・却下できる」。これまで Claude の操作は `POST /ops` と `mm op` で即座に適用され、人が承認する前の状態を持てなかった。Claude の対応 1 回を、プロジェクトのデータに入れずに保留し、人が見比べてから入れるか捨てるかを決める単位が要る。

## 先行事例と新規の設計

| 先行事例 | 採ったもの | 採らなかったもの |
|---|---|---|
| GitHub の PR（ブランチを元のコミットに対して作り、マージのときに衝突を判定する） | 提案は元にしたバージョン（`base_version`）と操作の列を持ち、承認のときに `Store::apply_txn` と同じ `touches` の判定をする。後の編集と触る所が重ならなければ、そのまま入る | 提案ごとの枝の上で人が続けて直すこと（提案は Claude が作り直す） |
| Google Docs の提案モード（提案ごとに承認・却下、承認は Ctrl+Z で戻る、承認した後の見た目を切り替えて見る） | 承認で普通のトランザクションとして適用し、取り消しとやり直しがそのまま効く。Program で「提案を適用したフレーム」と「元のフレーム」を切り替える。却下は保留に戻せる | 文書の中に提案の印を混ぜること（プロジェクトのデータは確定したものだけにする） |
| Figma のブランチのレビュー（変更の一覧から選んでマージ） | 操作ごとのチェックで一部だけ承認する | 衝突の箇所ごとに「どちらを採るか」を選ぶ UI（衝突したら Claude が今のデータで作り直す） |

新規に設計したもの: 提案を決めるのは人の操作経路だけにすること（MCP の経路からの承認・却下は 403）。提案した Claude 自身が承認すると保留の意味が無いため。

## データの形

- `project.mmdb` の `proposals` の表（保存のスキーマ 4）。名前、操作経路、発行元、時刻、対応するコメント、`base_version`、操作（`Op` の JSON の配列）、受け付けたときの `touches`、状態（`pending` / `approved` / `rejected`）、決めた者と時刻、承認で適用した `txn_id`、承認した操作の番号。プロジェクトのデータの JSON（`mm-doc` のスキーマ）は変えない。
- 提案を受け付けるとき、今のデータのコピーに適用して検証する（ロック、`validate`、文字起こし、`base_version` の衝突）。通らなければ保留しない。
- 承認は、提案の名前・発行元・対応するコメントのまま、`base_version` を元にしたトランザクションとして適用し、同じ SQLite のトランザクションで提案を承認済みにする。発行元は提案した者なので、取り消しの文言は「mcp の操作を取り消した」になる。
- `applied`（承認したトランザクションが取り消されていない）と `undoable`（取り消しのスタックの一番上）は、表に持たず `undo_stack` から読む。

## 経路

| 経路 | 提案を出す | 決める |
|---|---|---|
| HTTP（`mm-serve`） | `POST /proposals`（本体は `POST /ops` と同じ） | `POST /proposals/{id}/approve`（`{"ops":[0,2]}`）、`/reject`、`/reopen`。`entry: "mcp"` は 403 |
| CLI | `mm proposal add`（引数は `mm op` と同じ） | `mm proposal approve <id> --ops 0,2`、`reject`、`reopen`、`list` |
| MCP（`POST /mcp`） | `propose`（引数は `transaction` と同じ）。読むのは `proposals`、`frame` の `proposal` | —（承認・却下のツールは置かない） |
| アプリ（Review パネル） | — | `review_approve`、`review_reject`、`review_reopen`、`review_undo`、`review_preview`（`ui/sdk/README.md`） |

読むのは `GET /proposals`、`GET /proposals/{id}`（だれが、操作、触る範囲 `scope`、後の編集との `conflict`）、`GET /frame?proposal=<id>&ops=0,2`（提案を適用した 1 フレーム）。通知は SSE の `proposal` とアプリの `mm:proposals-changed`。MCP のサーバーはこの `Core` の関数（`propose`、`proposals`、`approve` など）を使う。

## 足したもの（API クレートは変えない）

| クレート | 足したもの |
|---|---|
| `mm-ops` | `select_ops`、`preview_ops`、`proposal_scope`、`ProposalScope` |
| `mm-store` | `Store::propose`、`proposals`、`proposal`、`proposal_conflict`、`preview_proposal`、`approve_proposal`、`reject_proposal`、`reopen_proposal`、`ProposalState`、`StoredProposal`。保存のスキーマ 3 → 4 |
| `mm-serve` | `Core::propose` ほか、`ProposalView`、`ProposalEvent`、`Approval`、`ServeError::Forbidden` |

## 決めていないこと

- MCP の操作を既定で提案にするか。今は呼ぶ側が選ぶ（HTTP は `POST /proposals`、MCP は `propose` のツール）。
- 取り消された承認を、もう一度保留に戻すこと。今はやり直し（Ctrl+Y）か、Claude が作り直す。
