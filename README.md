# claude-marketplace

Claude Code のスキルを配布する公開マーケットプレイスです。

Claude Code のクラウドセッションは利用者のマシンの `~/.claude` を参照しません。
公開マーケットプレイスに置いたプラグインは、クラウドセッションのセットアップスクリプトから取得できます。

## 収録プラグイン

**tech-writing**：日本語技術文書の文章規範と、技術キーワードの調査・解説ページ作成。

- `japanese-tech-writing`：整形、段落と論証の構成、読み手の負荷の管理、冗長の排除を定める文章規範。
- `keyword-explainer`：ライブラリやプロトコルなどのキーワードを公式リファレンスから調査し、Notion に解説ページを作成する。Notion の MCP サーバーを前提とする。

**gankooyaji**：Claude の舐めた返答に頑固オヤジがツッコミを入れる関数フック。
回答を読んで曖昧な言葉遣いを見つけると、次の入力欄の直上にツッコミを出します。

**gh-stack**：スタックしたブランチとプルリクエストを扱う `gh` CLI 拡張のスキル。
GitHub 公式リポジトリ [github/gh-stack](https://github.com/github/gh-stack) をそのまま参照します。

## 導入

```bash
claude plugin marketplace add Durun/claude-marketplace
claude plugin install tech-writing@durun-toolbox
claude plugin install gankooyaji@durun-toolbox
claude plugin install gh-stack@durun-toolbox
```

## クラウドセッションで使う

[claude.ai/code](https://claude.ai/code) の環境設定ダイアログで、セットアップスクリプトに次を記述します。

```bash
claude plugin marketplace add Durun/claude-marketplace
claude plugin install tech-writing@durun-toolbox -y --scope user
claude plugin install gankooyaji@durun-toolbox -y --scope user
claude plugin install gh-stack@durun-toolbox -y --scope user
```

設定先は `~/.claude/settings.json` なので、対象のリポジトリに変更を加えずに済みます。
