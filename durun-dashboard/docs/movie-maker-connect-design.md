# ムービーメーカー（movie-maker とつなぐアプリ）の設計

ダッシュボードのアプリ「ムービーメーカー」は、movie-maker（`movie-maker/apps/movie-maker`、Tauri 2）を入れて起動し、ウィンドウを開いている間だけ、そのプロジェクトをウィンドウを開いたセッションにつなぐ。
つないだセッションの Claude は movie-maker を操作でき、人間が movie-maker でした操作はフックで届く。
movie-maker の段階 5 の「MCP」と「durun-dashboard の API」を兼ねる（`movie-maker/docs/design/15-roadmap.md`）。

## 先行事例から採ったもの

| 先例 | 採ったもの |
|---|---|
| FL Connect（`server/flconnect.ts`、`~/research/fl-studio-agent-bridge/design.md`） | ウィンドウを開いている間だけつなぐ。つなぐ先はウィンドウを開いたセッションで、ウィンドウで切り替えられる。ダッシュボードの MCP は全セッションで共有するので、ツールは 3 つに絞り、広さは汎用の 1 本（`mm_tool`）で得る。人間の操作は UserPromptSubmit と PostToolUse のフックの additionalContext で届ける。ログは生の値の JSONL で、セッションの資料に足す。閉じる前に確かめる |
| movie-maker の操作経路（`docs/design/05-ops.md`） | 書き手はアプリで、`mm-serve` の HTTP（127.0.0.1、`.mm/lock.info` のポート、`%LOCALAPPDATA%\movie-maker\tokens` のトークン、Bearer）を使う。操作は `POST /ops` と同じ `Core::apply` を通り、`entry = mcp`、取り消しはアプリの Ctrl+Z と同じ列。人間の操作は `GET /events`（SSE）の `change` の `actor`（`gui` が人間、`mcp` と `cli` がエージェント）で分ける |
| movie-maker の MCP（`05-ops.md`「操作ごとのツール。引数の型はスキーマから作る」） | `mm-serve` に `POST /mcp` を足し、`mm_ops::ops_schema()` の 55 の操作をそのままツールにする。`responds_to` でコメントへの対応 1 回を 1 つのトランザクションに結ぶ（10-review-improve.md） |
| Blender MCP、Unity MCP | アプリの中にサーバーを置き、外の MCP は薄い中継にする形。汎用の実行の 1 本と、状態を読む 1 本の組 |
| OBS WebSocket | 認証つきのローカルの口と、状態の変化を購読するイベントの流れ。ダッシュボードは `/events` を購読し、読み直しは `resync` のときだけ |
| CD取り込み（`server/cdripTools.ts`） | 外部の道具の場所を環境変数、既定の置き場所の順で探し、無ければウィンドウの「次にすること」で導入を促す |

## 新しく設計したもの

- **二段の MCP**：movie-maker の MCP（55 の操作と `transaction`、`undo`、`redo`、`project`、`frame`）は movie-maker が持つ。ダッシュボードの `mm_tool` は、その `tools/call` を名前と引数のまま中継する。ツールの数は movie-maker 側で増えても、全セッションに見えるのは 3 つのまま。
- **入れ方の口（`server/movieMakerInstall.ts`）**：`Installer`（`status`、`install`）を 2 つ持つ。開発用は「ビルドする」、配布用は「ビルド済みを入れる」。どちらを使うかは `DURUN_DASHBOARD_DISTRIBUTION=1` で決める。今回は配布用の中身を作らず、「まだ用意していない」と返す。
- **起動とつなぎ**：プロジェクトのフォルダー（`project.mmdb` のあるところ）を選び、入れた `movie-maker.exe <フォルダー>` を起動する。`.mm/lock.info` を `fs.watch` で見て、書き手の pid が生きていてポートとトークンがそろったらつなぐ（時間で読み直さない）。`project.mmdb` が無いフォルダーは `mm new` で作る。

## ツール

| ツール | 中身 |
|---|---|
| `mm_state` | つながり、プロジェクトのフォルダー、バージョン、取り消しの深さ、コンポジションとレイヤー、sequence、素材の短い一覧 |
| `mm_tool` | movie-maker の MCP のツールを呼ぶ。`tool` を省くと一覧、`arguments` を省くとそのツールの引数のスキーマを返す。`frame` の画像はそのまま返す |
| `mm_changes` | 前に読んでから変わったトランザクションを、人間とエージェントに分けて時刻順に返す（フックで届いた分は除く） |

## 人間の操作をセッションへ届ける

- `GET /events` の `change` のうち `actor` が `gui` のもの（取り消しとやり直しは `of` も）を記録し、つないだセッションの次の UserPromptSubmit と PostToolUse に `additionalContext` で添える（FL Connect と同じ。`hooks.ts` が両方を合わせる）。
- `resync` が来たら `GET /project` のバージョンだけ読み直し、取りこぼしたことを 1 行添える。

## インストールとビルドと配布用モードの口

| | 開発用（今回） | 配布用（口だけ） |
|---|---|---|
| 入れる物 | `movie-maker.exe`、`mm.exe`（FFmpeg ありなら FFmpeg の DLL も） | 同じ |
| 手順 | `git fetch origin main` → 専用の作業ツリーを `origin/main` に合わせる → `ui/` で `pnpm install --frozen-lockfile` と `pnpm build` → `CARGO_INCREMENTAL=0 cargo build --release -p movie-maker -p mm` → 入れる場所へ写す | GitHub Releases の zip を落として展開する（未実装） |
| ソース | `<データの場所>/movie-maker/src`（`git worktree add --detach` で作り、`origin/main` に固定。人間の作業中の変更を混ぜない） | 無し |
| ビルドの置き場所 | `DURUN_MM_TARGET_DIR`、無ければ Windows は `D:/dev/movie-maker-dashboard/target`（`D:/dev` が無ければ `<データの場所>/movie-maker/target`） | 無し |
| 入れる場所 | `<データの場所>/movie-maker/bin` と、入れたときのコミットと機能を書いた `install.json` | 同じ |
| FFmpeg | `FFMPEG_DIR` があれば `--features ffmpeg` で作り、`FFMPEG_DIR/bin` の DLL を写す | 同梱 |

最後に fetch した `origin/main` のコミットが `install.json` と違えば、ウィンドウが「作り直す」を出す。

## 確かめ方

- テスト（`test/movieMaker.test.ts`）：記録の分け方、フックの文、`lock.info` の読み方、ツールの中継、入れ方の口の選び方と手順の組み立て。
- 試し台：`server/movieMaker.ts` を読み込み、偽の `mm-serve`（HTTP と SSE）と偽のウィンドウの SSE でつなぎ、`mm_tool` の中継と、人間の操作がフックの文になるまでを動かす。
- 本物：main を早送りしてから、人間と、セッションから `mm_state` → `mm_tool`（`addMarker` など）→ movie-maker で人間が操作 → フックで届く、を確かめる。
- movie-maker 側：`crates/mm-serve/tests/mcp.rs`（初期化、一覧、操作の適用が `mcp` の記録になる、ロックの拒否、トークンの無い要求の拒否）。
